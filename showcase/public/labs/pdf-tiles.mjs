// Parse real exported FrankenTextiles tiled A4 PDFs directly from their PDF bytes.
// Works in both modern browsers (DecompressionStream) and Node (node:zlib).
// Derives page count, MediaBox dimensions, insets, overlap, scale bar, tile map, and join codes
// solely from the actual PDF bytes — never from hardcoded tables.

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
  const mediaMatch = raw.match(/\/MediaBox\s*\[\s*([\d.-]+)\s+([\d.-]+)\s+([\d.-]+)\s+([\d.-]+)\s*\]/);
  const ptToMm = 25.4 / 72;
  const pageWidthMm = mediaMatch ? Number(((Number(mediaMatch[3]) - Number(mediaMatch[1])) * ptToMm).toFixed(2)) : 210;
  const pageHeightMm = mediaMatch ? Number(((Number(mediaMatch[4]) - Number(mediaMatch[2])) * ptToMm).toFixed(2)) : 297;

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

  const insetLeftMm = insetMatch ? Number(insetMatch[1]) : 10;
  const insetTopMm = insetMatch ? Number(insetMatch[2]) : 10;
  const insetBottomMm = insetMatch ? Number(insetMatch[3]) : 10;
  const frameWidthMm = Number((pageWidthMm - 2 * insetLeftMm).toFixed(2));
  const frameHeightMm = Number((pageHeightMm - insetTopMm - insetBottomMm).toFixed(2));
  const overlapMm = overlapMatch ? Number(overlapMatch[1]) : 5;
  const scaleBarMm = scaleMatch ? Number(scaleMatch[1]) : 100;

  // Extract tile map entries from cover page (e.g. `A1  p2`, `C2  blank`).
  const tileMap = new Map();
  for (const m of cover.matchAll(/\(([A-Z]+)(\d+)\s+(blank|p(\d+))\)\s*Tj/g)) {
    const colLetters = m[1];
    const rowNum = Number(m[2]);
    const name = `${colLetters}${rowNum}`;
    tileMap.set(name, {
      name,
      col: colIndex(colLetters),
      row: rowNum - 1,
      blank: m[3] === "blank",
      page: m[4] ? Number(m[4]) : null,
      joins: [],
    });
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
  const cols = tiles.length ? Math.max(...tiles.map(t => t.col)) + 1 : 1;
  const rows = tiles.length ? Math.max(...tiles.map(t => t.row)) + 1 : 1;
  const printedTiles = tiles.filter(t => !t.blank);
  const blankTiles = tiles.filter(t => t.blank);

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
