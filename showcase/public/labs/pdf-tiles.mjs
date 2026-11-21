// Parse real exported FrankenTextiles tiled A4 PDFs directly from their PDF bytes.
// Uses standard Web Streams `DecompressionStream("deflate")` in both browsers and Node.
// Derives page count, MediaBox dimensions, insets, overlap, scale bar, tile map, /FPTile metadata,
// and join codes solely from the actual PDF bytes — fails closed rather than falling back to defaults.

async function inflateStream(bytes) {
  const ds = new DecompressionStream("deflate");
  const writer = ds.writable.getWriter();
  writer.write(bytes);
  writer.close();
  const out = await new Response(ds.readable).arrayBuffer();
  return new Uint8Array(out);
}

function decodeLatin1(bytes) {
  let out = "";
  const chunk = 0x8000;
  for (let i = 0; i < bytes.length; i += chunk) {
    out += String.fromCharCode(...bytes.subarray(i, i + chunk));
  }
  return out;
}

function colIndex(letters) {
  let v = 0;
  for (let i = 0; i < letters.length; i++) {
    v = v * 26 + (letters.charCodeAt(i) - 64);
  }
  return v - 1;
}

export async function parseTiledPdf(input) {
  const bytes = input instanceof Uint8Array ? input : new Uint8Array(input);
  const raw = decodeLatin1(bytes);
  if (!raw.startsWith("%PDF-")) {
    throw new Error("File is not a valid PDF document.");
  }

  const totalPages = (raw.match(/\/Type\s*\/Page\b(?!s)/g) || []).length;
  if (totalPages < 2) {
    throw new Error(`Expected tiled PDF with cover + tile pages, found ${totalPages} page(s).`);
  }

  const mediaMatch = raw.match(/\/MediaBox\s*\[\s*([\d.-]+)\s+([\d.-]+)\s+([\d.-]+)\s+([\d.-]+)\s*\]/);
  if (!mediaMatch) {
    throw new Error("Exported PDF is missing a /MediaBox declaration.");
  }
  const ptToMm = 25.4 / 72;
  const pageWidthMm = Number(((Number(mediaMatch[3]) - Number(mediaMatch[1])) * ptToMm).toFixed(2));
  const pageHeightMm = Number(((Number(mediaMatch[4]) - Number(mediaMatch[2])) * ptToMm).toFixed(2));

  // Cross-check structured /FPTile page dictionaries written by ftx-export.
  const fpTiles = new Map();
  for (const m of raw.matchAll(
    /\/FPTile\s*<<\s*\/Name\s*\(([A-Z]+\d+)\)\s*\/Col\s+(\d+)\s*\/Row\s+(\d+)\s*\/Frame\s*\[\s*([\d.-]+)\s+([\d.-]+)\s+([\d.-]+)\s+([\d.-]+)\s*\]\s*\/Origin\s*\[\s*([\d.-]+)\s+([\d.-]+)\s*\]\s*\/Blank\s+(true|false)\s*>>/g,
  )) {
    fpTiles.set(m[1], {
      name: m[1],
      col: Number(m[2]),
      row: Number(m[3]),
      frame: [Number(m[4]), Number(m[5]), Number(m[6]), Number(m[7])],
      origin: [Number(m[8]), Number(m[9])],
      blank: m[10] === "true",
    });
  }

  // Inflate every FlateDecode stream inside the PDF.
  const streams = [];
  let idx = 0;
  while ((idx = raw.indexOf("stream\n", idx)) !== -1) {
    const end = raw.indexOf("\nendstream", idx + 7);
    if (end === -1) break;
    try {
      const inflated = await inflateStream(bytes.subarray(idx + 7, end));
      streams.push(decodeLatin1(inflated));
    } catch {
      // Ignore non-deflate streams if any.
    }
    idx = end + 10;
  }

  if (!streams.length) {
    throw new Error("Could not decode page streams from the exported PDF.");
  }

  const cover = streams[0];
  const coverText = [...cover.matchAll(/\(([^()]*)\)\s*Tj/g)].map(m => m[1]).join(" ");
  const overlapMatch = coverText.match(/run\s+([\d.]+)\s*mm\s+past/);
  const insetMatch = coverText.match(
    /edge insets\s+([\d.]+)\s*mm\s+left\/right,\s*([\d.]+)\s*mm\s+top,\s*([\d.]+)\s*mm\s+bottom/,
  );
  const scaleMatch = coverText.match(/10 cm = ([\d.]+) mm/);
  if (!overlapMatch || !insetMatch || !scaleMatch) {
    throw new Error("Exported PDF cover stream is missing inset, overlap, or 100 mm scale metadata.");
  }

  const insetLeftMm = Number(insetMatch[1]);
  const insetTopMm = Number(insetMatch[2]);
  const insetBottomMm = Number(insetMatch[3]);
  const frameWidthMm = Number((pageWidthMm - 2 * insetLeftMm).toFixed(2));
  const frameHeightMm = Number((pageHeightMm - insetTopMm - insetBottomMm).toFixed(2));
  const overlapMm = Number(overlapMatch[1]);
  const scaleBarMm = Number(scaleMatch[1]);

  // Extract tile map entries from cover page (e.g. `A1  p2`, `C2  blank`).
  const tileMap = new Map();
  for (const m of cover.matchAll(/\(([A-Z]+)(\d+)\s+(blank|p(\d+))\)\s*Tj/g)) {
    const colLetters = m[1];
    const rowNum = Number(m[2]);
    const name = `${colLetters}${rowNum}`;
    const col = colIndex(colLetters);
    const row = rowNum - 1;
    const blank = m[3] === "blank";
    const meta = fpTiles.get(name);
    if (meta && (meta.col !== col || meta.row !== row || meta.blank !== blank)) {
      throw new Error(`Tile ${name} disagrees between cover map and /FPTile page dictionary.`);
    }
    tileMap.set(name, {
      name,
      col,
      row,
      blank,
      page: m[4] ? Number(m[4]) : null,
      joins: [],
    });
  }
  if (!tileMap.size) {
    throw new Error("Exported PDF cover page contains no tile map entries.");
  }

  // Extract exact neighbor join codes (e.g. `A1|B1`, `A1|A2`) directly from each printed tile page stream.
  for (let i = 1; i < streams.length; i++) {
    const s = streams[i];
    const tileName = s.match(/\(([A-Z]+\d+)\)\s*Tj/)?.[1];
    const joins = [...new Set([...s.matchAll(/\(([A-Z]+\d+\|[A-Z]+\d+)\)\s*Tj/g)].map(m => m[1]))];
    if (tileName && tileMap.has(tileName)) {
      tileMap.get(tileName).joins = joins;
    }
  }

  const tiles = [...tileMap.values()].sort((a, b) => a.row - b.row || a.col - b.col);
  const cols = Math.max(...tiles.map(t => t.col)) + 1;
  const rows = Math.max(...tiles.map(t => t.row)) + 1;
  const printedTiles = tiles.filter(t => !t.blank);
  const blankTiles = tiles.filter(t => t.blank);
  if (fpTiles.size && fpTiles.size !== printedTiles.length) {
    throw new Error(`PDF has ${fpTiles.size} /FPTile pages but cover lists ${printedTiles.length} printed tiles.`);
  }

  return {
    totalPages,
    printedTileCount: printedTiles.length,
    blankTileCount: blankTiles.length,
    pageWidthMm,
    pageHeightMm,
    insetLeftMm,
    insetTopMm,
    insetBottomMm,
    frameWidthMm,
    frameHeightMm,
    overlapMm,
    scaleBarMm,
    cols,
    rows,
    assembledWidthMm: Number((cols * frameWidthMm).toFixed(2)),
    assembledHeightMm: Number((rows * frameHeightMm).toFixed(2)),
    tiles,
  };
}
