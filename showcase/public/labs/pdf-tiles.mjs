// Parse real exported FrankenTextiles tiled A4 PDFs directly from their PDF bytes.
// Uses standard Web Streams `DecompressionStream("deflate")` in both browsers and Node.
// Derives page count, MediaBox dimensions, insets, overlap, scale bar, tile map, /FPTile metadata,
// and join codes solely from the actual PDF bytes — fails closed rather than falling back to defaults.

async function inflateStream(bytes, pageLabel) {
  const ds = new DecompressionStream("deflate");
  const writer = ds.writable.getWriter();
  const reading = new Response(ds.readable).arrayBuffer();
  const writing = (async () => {
    try {
      await writer.write(bytes);
      await writer.close();
    } catch (err) {
      await writer.abort(err).catch(() => {});
      throw err;
    }
  })();
  const [readResult, writeResult] = await Promise.allSettled([reading, writing]);
  if (readResult.status === "rejected" || writeResult.status === "rejected") {
    const reason = readResult.status === "rejected" ? readResult.reason : writeResult.reason;
    const detail = reason instanceof Error ? reason.message : String(reason);
    throw new Error(`Damaged ${pageLabel} deflate stream: ${detail}`);
  }
  return new Uint8Array(readResult.value);
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

function skipWsAndComments(raw, start) {
  let i = start;
  while (i < raw.length) {
    const c = raw.charCodeAt(i);
    if (c === 32 || c === 9 || c === 10 || c === 13 || c === 12 || c === 0) {
      i++;
    } else if (c === 37) {
      while (i < raw.length && raw.charCodeAt(i) !== 10 && raw.charCodeAt(i) !== 13) i++;
    } else {
      break;
    }
  }
  return i;
}

function scanPdfDict(raw, start, objNum) {
  if (!raw.startsWith("<<", start)) {
    throw new Error(`Malformed PDF dictionary in object ${objNum}.`);
  }
  let i = start + 2;
  let depth = 1;
  while (i < raw.length && depth > 0) {
    const ch = raw[i];
    if (ch === "(") {
      let strDepth = 1;
      i++;
      while (i < raw.length && strDepth > 0) {
        if (raw[i] === "\\") {
          i += 2;
        } else if (raw[i] === "(") {
          strDepth++;
          i++;
        } else if (raw[i] === ")") {
          strDepth--;
          i++;
        } else {
          i++;
        }
      }
      if (strDepth !== 0) {
        throw new Error(`Truncated PDF string literal in object ${objNum}.`);
      }
      continue;
    }
    if (ch === "<" && raw[i + 1] === "<") {
      depth++;
      i += 2;
    } else if (ch === ">" && raw[i + 1] === ">") {
      depth--;
      i += 2;
    } else {
      i++;
    }
  }
  if (depth !== 0) {
    throw new Error(`Truncated PDF dictionary in object ${objNum}.`);
  }
  return { dict: raw.slice(start, i), end: i };
}

function parseIndirectObjects(bytes, raw) {
  const objects = new Map();
  const headerRe = /(?:^|[\r\n])(\d+)\s+(\d+)\s+obj(?=[ \t\r\n<\[/(])/g;
  let pos = 0;

  while (pos < raw.length) {
    headerRe.lastIndex = pos;
    const match = headerRe.exec(raw);
    if (!match) break;

    const objNum = Number(match[1]);
    let cursor = skipWsAndComments(raw, match.index + match[0].length);

    if (raw.startsWith("<<", cursor)) {
      const { dict, end } = scanPdfDict(raw, cursor, objNum);
      cursor = skipWsAndComments(raw, end);

      if (raw.startsWith("stream", cursor)) {
        const afterStream = cursor + 6;
        let dataStart;
        if (raw.startsWith("\r\n", afterStream)) {
          dataStart = afterStream + 2;
        } else if (raw[afterStream] === "\n") {
          dataStart = afterStream + 1;
        } else {
          throw new Error(`Malformed PDF stream header in object ${objNum}.`);
        }

        let streamBytes;
        let afterEndstream;
        const lenMatch = dict.match(/\/Length\s+(\d+)(?!\s+\d+\s+R)\b/);
        if (lenMatch) {
          const declaredLen = Number(lenMatch[1]);
          if (!Number.isSafeInteger(declaredLen) || declaredLen < 0 || dataStart + declaredLen > bytes.length) {
            throw new Error(
              `Truncated PDF stream in object ${objNum}: declared /Length ${declaredLen} exceeds file bounds.`,
            );
          }
          const afterData = dataStart + declaredLen;
          let endstreamPos = afterData;
          if (raw.startsWith("\r\n", afterData)) {
            endstreamPos = afterData + 2;
          } else if (raw[afterData] === "\n" || raw[afterData] === "\r") {
            endstreamPos = afterData + 1;
          }
          if (!raw.startsWith("endstream", endstreamPos)) {
            throw new Error(
              `Damaged or truncated PDF stream in object ${objNum}: missing endstream at declared /Length ${declaredLen}.`,
            );
          }
          streamBytes = bytes.subarray(dataStart, afterData);
          afterEndstream = skipWsAndComments(raw, endstreamPos + 9);
        } else {
          const endRe = /(?:\r?\n)?endstream\b/g;
          endRe.lastIndex = dataStart;
          const endMatch = endRe.exec(raw);
          if (!endMatch) {
            throw new Error(`Truncated PDF stream in object ${objNum}: missing endstream.`);
          }
          streamBytes = bytes.subarray(dataStart, endMatch.index);
          afterEndstream = skipWsAndComments(raw, endMatch.index + endMatch[0].length);
        }

        if (!raw.startsWith("endobj", afterEndstream)) {
          throw new Error(`Truncated PDF object ${objNum}: missing endobj after endstream.`);
        }
        objects.set(objNum, { dict, streamBytes });
        pos = afterEndstream + 6;
      } else {
        if (!raw.startsWith("endobj", cursor)) {
          throw new Error(`Truncated PDF object ${objNum}: missing endobj.`);
        }
        objects.set(objNum, { dict, streamBytes: null });
        pos = cursor + 6;
      }
    } else {
      const endRe = /\bendobj\b/g;
      endRe.lastIndex = cursor;
      const endMatch = endRe.exec(raw);
      if (!endMatch) {
        throw new Error(`Truncated PDF object ${objNum}: missing endobj.`);
      }
      objects.set(objNum, { dict: raw.slice(cursor, endMatch.index).trim(), streamBytes: null });
      pos = endMatch.index + 6;
    }
  }

  return objects;
}

function collectOrderedPageIds(objects) {
  let rootPagesId = null;
  for (const [id, obj] of objects) {
    if (/\/Type\s*\/Catalog\b/.test(obj.dict)) {
      const ref = obj.dict.match(/\/Pages\s+(\d+)\s+0\s+R\b/);
      if (ref) {
        rootPagesId = Number(ref[1]);
        break;
      }
    }
  }
  if (rootPagesId === null) {
    for (const [id, obj] of objects) {
      if (/\/Type\s*\/Pages\b/.test(obj.dict) && !/\/Parent\b/.test(obj.dict)) {
        rootPagesId = id;
        break;
      }
    }
  }
  if (rootPagesId === null) return null;

  const pageIds = [];
  const visited = new Set();
  const walk = nodeId => {
    if (visited.has(nodeId)) {
      throw new Error(`Cyclic PDF /Pages tree at object ${nodeId}.`);
    }
    visited.add(nodeId);
    const node = objects.get(nodeId);
    if (!node) {
      throw new Error(`Truncated PDF /Pages tree: missing object ${nodeId}.`);
    }
    if (/\/Type\s*\/Pages\b/.test(node.dict)) {
      const kidsMatch = node.dict.match(/\/Kids\s*\[([^\]]*)\]/);
      const countMatch = node.dict.match(/\/Count\s+(\d+)\b/);
      if (!kidsMatch || !countMatch) {
        throw new Error(`Malformed PDF /Pages node in object ${nodeId}.`);
      }
      const before = pageIds.length;
      for (const ref of kidsMatch[1].matchAll(/(\d+)\s+0\s+R\b/g)) {
        walk(Number(ref[1]));
      }
      const expectedCount = Number(countMatch[1]);
      if (pageIds.length - before !== expectedCount) {
        throw new Error(
          `PDF /Pages node ${nodeId} declares /Count ${expectedCount}, found ${pageIds.length - before} page(s).`,
        );
      }
    } else if (/\/Type\s*\/Page\b(?!s)/.test(node.dict)) {
      pageIds.push(nodeId);
    } else {
      throw new Error(`PDF /Pages child ${nodeId} is neither /Pages nor /Page.`);
    }
  };
  walk(rootPagesId);
  return pageIds;
}

function parseFpTile(dict) {
  const m = dict.match(
    /\/FPTile\s*<<\s*\/Name\s*\(([A-Z]+\d+)\)\s*\/Col\s+(\d+)\s*\/Row\s+(\d+)\s*\/Frame\s*\[\s*([\d.-]+)\s+([\d.-]+)\s+([\d.-]+)\s+([\d.-]+)\s*\]\s*\/Origin\s*\[\s*([\d.-]+)\s+([\d.-]+)\s*\]\s*\/Blank\s+(true|false)\s*>>/,
  );
  if (!m) return null;
  return {
    name: m[1],
    col: Number(m[2]),
    row: Number(m[3]),
    frame: [Number(m[4]), Number(m[5]), Number(m[6]), Number(m[7])],
    origin: [Number(m[8]), Number(m[9])],
    blank: m[10] === "true",
  };
}

function parseContentRefs(pageDict, pageNum) {
  const arrayMatch = pageDict.match(/\/Contents\s*\[([^\]]+)\]/);
  if (arrayMatch) {
    const refs = [...arrayMatch[1].matchAll(/(\d+)\s+0\s+R\b/g)].map(m => Number(m[1]));
    if (refs.length) return refs;
  }
  const singleMatch = pageDict.match(/\/Contents\s+(\d+)\s+0\s+R\b/);
  if (singleMatch) return [Number(singleMatch[1])];
  throw new Error(`Page ${pageNum} is missing a /Contents stream reference.`);
}

async function decodePageStream(objects, pageDict, pageNum) {
  const refs = parseContentRefs(pageDict, pageNum);
  const parts = [];
  for (const ref of refs) {
    const obj = objects.get(ref);
    if (!obj || !(obj.streamBytes instanceof Uint8Array)) {
      throw new Error(`Page ${pageNum} content object ${ref} is missing or has no stream.`);
    }
    const filterMatch = obj.dict.match(/\/Filter\s*(?:\[\s*([^\]]+)\s*\]|(\/[A-Za-z0-9]+))/);
    const filterText = (filterMatch?.[1] ?? filterMatch?.[2] ?? "").trim();
    if (!filterText) {
      parts.push(decodeLatin1(obj.streamBytes));
    } else if (filterText === "/FlateDecode") {
      const inflated = await inflateStream(obj.streamBytes, `page ${pageNum}`);
      parts.push(decodeLatin1(inflated));
    } else {
      throw new Error(`Unsupported PDF stream filter ${filterText} on page ${pageNum}.`);
    }
  }
  return parts.join("\n");
}

export async function parseTiledPdf(input) {
  const bytes = input instanceof Uint8Array ? input : new Uint8Array(input);
  const raw = decodeLatin1(bytes);
  if (!raw.startsWith("%PDF-")) {
    throw new Error("File is not a valid PDF document.");
  }

  const objects = parseIndirectObjects(bytes, raw);
  const orderedPageIds = collectOrderedPageIds(objects);
  const pageCount = orderedPageIds
    ? orderedPageIds.length
    : (raw.match(/\/Type\s*\/Page\b(?!s)/g) || []).length;
  if (pageCount < 2) {
    throw new Error(`Expected tiled PDF with cover + tile pages, found ${pageCount} page(s).`);
  }
  if (!orderedPageIds) {
    throw new Error("Exported PDF is missing a valid /Pages page tree.");
  }
  if (!/%%EOF\b/.test(raw)) {
    throw new Error("Truncated PDF document: missing %%EOF marker.");
  }

  const pageObjects = orderedPageIds.map((id, index) => {
    const obj = objects.get(id);
    if (!obj || !/\/Type\s*\/Page\b(?!s)/.test(obj.dict)) {
      throw new Error(`Missing or malformed page ${index + 1} dictionary (object ${id}).`);
    }
    return obj;
  });
  const totalPages = pageObjects.length;

  const mediaMatch =
    pageObjects[0].dict.match(/\/MediaBox\s*\[\s*([\d.-]+)\s+([\d.-]+)\s+([\d.-]+)\s+([\d.-]+)\s*\]/) ||
    raw.match(/\/MediaBox\s*\[\s*([\d.-]+)\s+([\d.-]+)\s+([\d.-]+)\s+([\d.-]+)\s*\]/);
  if (!mediaMatch) {
    throw new Error("Exported PDF is missing a /MediaBox declaration.");
  }
  const ptToMm = 25.4 / 72;
  const pageWidthMm = Number(((Number(mediaMatch[3]) - Number(mediaMatch[1])) * ptToMm).toFixed(2));
  const pageHeightMm = Number(((Number(mediaMatch[4]) - Number(mediaMatch[2])) * ptToMm).toFixed(2));
  if (!(pageWidthMm > 0) || !(pageHeightMm > 0)) {
    throw new Error("Exported PDF declares a non-positive /MediaBox.");
  }

  // Cross-check structured /FPTile page dictionaries written by ftx-export and decode every page stream.
  const fpTiles = new Map();
  const pageFpTiles = [];
  const streams = [];
  for (let i = 0; i < totalPages; i++) {
    const pageNum = i + 1;
    const fp = parseFpTile(pageObjects[i].dict);
    pageFpTiles.push(fp);
    if (fp) {
      if (fpTiles.has(fp.name)) {
        throw new Error(`Duplicate /FPTile declaration for tile ${fp.name}.`);
      }
      fpTiles.set(fp.name, { ...fp, page: pageNum });
    }
    streams.push(await decodePageStream(objects, pageObjects[i].dict, pageNum));
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
    const page = m[4] ? Number(m[4]) : null;
    const meta = fpTiles.get(name);
    if (meta && (meta.col !== col || meta.row !== row || meta.blank !== blank || meta.page !== page)) {
      throw new Error(`Tile ${name} disagrees between cover map and /FPTile page dictionary.`);
    }
    tileMap.set(name, {
      name,
      col,
      row,
      blank,
      page,
      joins: [],
    });
  }
  if (!tileMap.size) {
    throw new Error("Exported PDF cover page contains no tile map entries.");
  }

  const tiles = [...tileMap.values()].sort((a, b) => a.row - b.row || a.col - b.col);
  const printedTiles = tiles.filter(t => !t.blank);
  const blankTiles = tiles.filter(t => t.blank);
  if (printedTiles.length + 1 !== totalPages) {
    throw new Error(
      `PDF has ${totalPages - 1} tile page(s) but cover lists ${printedTiles.length} printed tiles.`,
    );
  }
  if (fpTiles.size && fpTiles.size !== printedTiles.length) {
    throw new Error(`PDF has ${fpTiles.size} /FPTile pages but cover lists ${printedTiles.length} printed tiles.`);
  }

  // Extract exact neighbor join codes (e.g. `A1|B1`, `A1|A2`) directly from each printed tile page stream.
  const seenTilePages = new Set();
  for (let i = 1; i < streams.length; i++) {
    const pageNum = i + 1;
    const s = streams[i];
    const fp = pageFpTiles[i];
    const streamTileNames = [...s.matchAll(/\(([A-Z]+\d+)\)\s*Tj/g)].map(m => m[1]);
    const tileName = fp
      ? (streamTileNames.includes(fp.name) ? fp.name : null)
      : (streamTileNames.find(name => tileMap.has(name)) ?? streamTileNames[0] ?? null);
    if (!tileName) {
      throw new Error(`Tile page ${pageNum} stream is missing its tile identifier.`);
    }
    const entry = tileMap.get(tileName);
    if (!entry || entry.blank) {
      throw new Error(`Tile page ${pageNum} declares unexpected or blank tile ${tileName}.`);
    }
    if (entry.page !== pageNum) {
      throw new Error(`Tile ${tileName} appears on PDF page ${pageNum}, expected page ${entry.page}.`);
    }
    if (seenTilePages.has(tileName)) {
      throw new Error(`Duplicate page stream for tile ${tileName}.`);
    }
    seenTilePages.add(tileName);
    entry.joins = [...new Set([...s.matchAll(/\(([A-Z]+\d+\|[A-Z]+\d+)\)\s*Tj/g)].map(m => m[1]))];
  }
  if (seenTilePages.size !== printedTiles.length) {
    throw new Error(
      `Decoded ${seenTilePages.size} of ${printedTiles.length} printed tile page streams from the exported PDF.`,
    );
  }

  const cols = Math.max(...tiles.map(t => t.col)) + 1;
  const rows = Math.max(...tiles.map(t => t.row)) + 1;

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
