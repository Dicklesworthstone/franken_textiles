// Decode Tajima DST embroidery records and register them onto the solved 3D hussar dolman
// chest surface through exact flat-panel triangle barycentric coordinates (ftx-f9t).
//
// Invariants:
// - Every registered 3D stitch must lie inside a panel triangle (or within RENDER_TOLERANCE_MM = 0.25 mm
//   for 0.1 mm DST quantization on curved placement boundaries).
// - Jump / hoop-center travel records and any out-of-tolerance or unregistered designs (e.g., the
//   standalone braid sampler) stay strictly on the flat board — never invented on the 3D garment.

export const RENDER_TOLERANCE_MM = 0.25;

export function decodeDst(input) {
  const bytes = input instanceof Uint8Array ? input : new Uint8Array(input);
  if (bytes.length < 515) {
    throw new Error("DST file is truncated (expected 512-byte header plus stitch records).");
  }
  const header = new TextDecoder("latin1").decode(bytes.subarray(0, 512));
  if (!header.includes("LA:") || !header.includes("ST:")) {
    throw new Error("Input does not have a valid Tajima DST 512-byte header (missing LA:/ST: fields).");
  }
  const label = header.match(/LA:([^\r\n]+)/)?.[1]?.trim() || "DST";

  const records = [];
  let x = 0;
  let y = 0; // tenths of mm (DST native units)
  let pass = "placement";
  let stopIndex = -1;

  for (let i = 512; i + 2 < bytes.length; i += 3) {
    const b0 = bytes[i];
    const b1 = bytes[i + 1];
    const b2 = bytes[i + 2];
    if (b0 === 0 && b1 === 0 && b2 === 0xf3) break; // EOF record

    let dx = 0;
    let dy = 0;
    if (b0 & 0x01) dx += 1;
    if (b0 & 0x02) dx -= 1;
    if (b0 & 0x04) dx += 9;
    if (b0 & 0x08) dx -= 9;
    if (b0 & 0x80) dy += 1;
    if (b0 & 0x40) dy -= 1;
    if (b0 & 0x20) dy += 9;
    if (b0 & 0x10) dy -= 9;

    if (b1 & 0x01) dx += 3;
    if (b1 & 0x02) dx -= 3;
    if (b1 & 0x04) dx += 27;
    if (b1 & 0x08) dx -= 27;
    if (b1 & 0x80) dy += 3;
    if (b1 & 0x40) dy -= 3;
    if (b1 & 0x20) dy += 27;
    if (b1 & 0x10) dy -= 27;

    if (b2 & 0x04) dx += 81;
    if (b2 & 0x08) dx -= 81;
    if (b2 & 0x20) dy += 81;
    if (b2 & 0x10) dy -= 81;

    x += dx;
    y += dy;

    const isStop = (b2 & 0xc3) === 0xc3;
    const isJump = (b2 & 0x83) === 0x83 && !isStop;
    const kind = isStop ? "stop" : isJump ? "jump" : "stitch";
    if (isStop && stopIndex === -1) {
      stopIndex = records.length;
      pass = "couching";
    }
    records.push({
      index: records.length,
      xMm: Number((x / 10).toFixed(2)),
      yMm: Number((y / 10).toFixed(2)),
      kind,
      pass: isStop ? "stop" : pass,
    });
  }

  return { label, records, stopIndex };
}

export function unpackDolmanBinary(buffer, pack) {
  const view = buffer instanceof ArrayBuffer ? buffer : buffer.buffer.slice(buffer.byteOffset, buffer.byteOffset + buffer.byteLength);
  const head = new DataView(view);
  const bodyCount = head.getUint32(0, true);
  const indexCount = head.getUint32(4, true);
  const vertices = head.getUint32(8, true);
  const frameCount = head.getUint32(12, true);
  if (vertices !== pack.vertices || frameCount !== pack.frames.length) {
    throw new Error("Mismatched dolman geometry package header.");
  }
  let at = 16;
  const bodyRaw = new Float32Array(view, at, bodyCount * 3);
  at += bodyCount * 12;
  const bodyIndex = new Uint32Array(view, at, indexCount);
  at += indexCount * 4;
  const flat = new Float32Array(view, at, vertices * 2);
  at += vertices * 8;
  const frames = new Float32Array(view, at, frameCount * vertices * 3);

  // Convert settled cloth frame and body from solver frame (z up, facing -y) to Three.js display frame (y up, facing +z).
  const settledOff = (frameCount - 1) * vertices * 3;
  const clothUpright = new Float32Array(vertices * 3);
  for (let i = 0; i < vertices * 3; i += 3) {
    clothUpright[i] = frames[settledOff + i];
    clothUpright[i + 1] = frames[settledOff + i + 2];
    clothUpright[i + 2] = -frames[settledOff + i + 1];
  }
  const bodyUpright = new Float32Array(bodyCount * 3);
  for (let i = 0; i < bodyCount * 3; i += 3) {
    bodyUpright[i] = bodyRaw[i];
    bodyUpright[i + 1] = bodyRaw[i + 2];
    bodyUpright[i + 2] = -bodyRaw[i + 1];
  }

  return { bodyUpright, bodyIndex, flat, clothUpright, vertices, frameCount };
}

export function extractPanelOutlineMm(pack, flat, panelName) {
  const bounds = pack.panels?.[panelName];
  if (!bounds) throw new Error(`Unknown panel ${panelName}`);
  const [lo, hi] = bounds;
  const twin = new Set();
  for (let t = 0; t < pack.triangles.length; t += 3) {
    const a = pack.triangles[t];
    const b = pack.triangles[t + 1];
    const c = pack.triangles[t + 2];
    if (a < lo || a >= hi || b < lo || b >= hi || c < lo || c >= hi) continue;
    twin.add(`${a}>${b}`);
    twin.add(`${b}>${c}`);
    twin.add(`${c}>${a}`);
  }
  const next = new Map();
  for (const edge of twin) {
    const [a, b] = edge.split(">").map(Number);
    if (!twin.has(`${b}>${a}`)) next.set(a, b);
  }
  const loops = [];
  while (next.size) {
    const first = next.keys().next().value;
    const loop = [];
    let v = first;
    do {
      loop.push([flat[v * 2] * 1000, flat[v * 2 + 1] * 1000]);
      const to = next.get(v);
      next.delete(v);
      v = to ?? first;
    } while (v !== first && next.has(v));
    if (loop.length >= 3) loops.push(loop);
  }
  return loops;
}

function closestPointOnTriangle2d(px, py, ax, ay, bx, by, cx, cy) {
  // Exact Euclidean closest point on triangle ABC in 2D, returning [u, v, dist] where p = (1-u-v)A + uB + vC.
  const edges = [
    [ax, ay, bx, by, 0, 0, 1, 0],
    [bx, by, cx, cy, 1, 0, 0, 1],
    [cx, cy, ax, ay, 0, 1, 0, 0],
  ];
  let bestDist = Infinity;
  let bestU = 0;
  let bestV = 0;
  for (let i = 0; i < 3; i++) {
    const [x0, y0, x1, y1, u0, v0, u1, v1] = edges[i];
    const ex = x1 - x0;
    const ey = y1 - y0;
    const len2 = ex * ex + ey * ey;
    const t = len2 > 0 ? Math.max(0, Math.min(1, ((px - x0) * ex + (py - y0) * ey) / len2)) : 0;
    const qx = x0 + t * ex;
    const qy = y0 + t * ey;
    const d = Math.hypot(px - qx, py - qy);
    if (d < bestDist) {
      bestDist = d;
      bestU = u0 + t * (u1 - u0);
      bestV = v0 + t * (v1 - v0);
    }
  }
  return [bestU, bestV, bestDist];
}

function triangleNormal(xyz, a, b, c) {
  const ax = xyz[a * 3], ay = xyz[a * 3 + 1], az = xyz[a * 3 + 2];
  const bx = xyz[b * 3] - ax, by = xyz[b * 3 + 1] - ay, bz = xyz[b * 3 + 2] - az;
  const cx = xyz[c * 3] - ax, cy = xyz[c * 3 + 1] - ay, cz = xyz[c * 3 + 2] - az;
  const nx = by * cz - bz * cy;
  const ny = bz * cx - bx * cz;
  const nz = bx * cy - by * cx;
  const len = Math.hypot(nx, ny, nz) || 1;
  return [nx / len, ny / len, nz / len];
}

export function registerDstOnDolmanPanel(dst, pack, unpacked, panelName, toleranceMm = RENDER_TOLERANCE_MM) {
  const bounds = pack.panels?.[panelName];
  if (!bounds) {
    // Explicit refusal/unregistered mode (e.g. standalone flat sampler without panel correspondence).
    return {
      panelName: null,
      registered: false,
      toleranceMm,
      centreFlatMm: [0, 0],
      flatBoundsMm: null,
      registeredStitches: 0,
      unmatchedStitches: dst.records.filter(r => r.kind === "stitch").length,
      flatOnlyJumps: dst.records.filter(r => r.kind !== "stitch").length,
      maxResidualMm: null,
      points: dst.records.map(r => ({
        ...r,
        flatMm: [r.xMm, r.yMm],
        onSurface: false,
        reason: "no-panel-correspondence",
        tri: null,
        bary: null,
        residualMm: null,
        pos3d: null,
        normal3d: null,
      })),
    };
  }

  const [lo, hi] = bounds;
  const { flat, clothUpright } = unpacked;
  let fMinX = Infinity, fMaxX = -Infinity, fMinY = Infinity, fMaxY = -Infinity;
  for (let v = lo; v < hi; v++) {
    const x = flat[v * 2] * 1000;
    const y = flat[v * 2 + 1] * 1000;
    if (x < fMinX) fMinX = x;
    if (x > fMaxX) fMaxX = x;
    if (y < fMinY) fMinY = y;
    if (y > fMaxY) fMaxY = y;
  }
  const cx = (fMinX + fMaxX) / 2;
  const cy = (fMinY + fMaxY) / 2;

  // Gather panel triangles with flat mm bounding boxes for fast exact barycentric lookup.
  const tris = [];
  for (let t = 0; t < pack.triangles.length; t += 3) {
    const a = pack.triangles[t];
    const b = pack.triangles[t + 1];
    const c = pack.triangles[t + 2];
    if (a < lo || a >= hi || b < lo || b >= hi || c < lo || c >= hi) continue;
    const ax = flat[a * 2] * 1000, ay = flat[a * 2 + 1] * 1000;
    const bx = flat[b * 2] * 1000, by = flat[b * 2 + 1] * 1000;
    const cx0 = flat[c * 2] * 1000, cy0 = flat[c * 2 + 1] * 1000;
    const v0x = bx - ax, v0y = by - ay;
    const v1x = cx0 - ax, v1y = cy0 - ay;
    const denom = v0x * v1y - v1x * v0y;
    if (Math.abs(denom) < 1e-12) continue;
    const normal = triangleNormal(clothUpright, a, b, c);
    // Ensure triangle normal points outward away from torso center (x=0, z=0 in upright frame).
    const mx = (clothUpright[a * 3] + clothUpright[b * 3] + clothUpright[c * 3]) / 3;
    const mz = (clothUpright[a * 3 + 2] + clothUpright[b * 3 + 2] + clothUpright[c * 3 + 2]) / 3;
    const flip = normal[0] * mx + normal[2] * mz < 0 ? -1 : 1;
    tris.push({
      ti: t / 3,
      a, b, c,
      ax, ay, bx, by, cx0, cy0, v0x, v0y, v1x, v1y, denom,
      minX: Math.min(ax, bx, cx0),
      maxX: Math.max(ax, bx, cx0),
      minY: Math.min(ay, by, cy0),
      maxY: Math.max(ay, by, cy0),
      normal: [normal[0] * flip, normal[1] * flip, normal[2] * flip],
    });
  }

  let registeredStitches = 0;
  let unmatchedStitches = 0;
  let flatOnlyJumps = 0;
  let maxResidualMm = 0;

  const points = dst.records.map(r => {
    const fx = cx + r.xMm;
    const fy = cy + r.yMm;
    if (r.kind !== "stitch") {
      flatOnlyJumps++;
      return {
        ...r,
        flatMm: [Number(fx.toFixed(2)), Number(fy.toFixed(2))],
        onSurface: false,
        reason: r.kind === "stop" ? "braid-stop" : "flat-hoop-jump",
        tri: null,
        bary: null,
        residualMm: null,
        pos3d: null,
        normal3d: null,
      };
    }

    let best = null;
    let bestDist = Infinity;
    let bestU = 0;
    let bestV = 0;
    for (let i = 0; i < tris.length; i++) {
      const tr = tris[i];
      if (fx < tr.minX - toleranceMm || fx > tr.maxX + toleranceMm || fy < tr.minY - toleranceMm || fy > tr.maxY + toleranceMm) {
        continue;
      }
      const dx = fx - tr.ax;
      const dy = fy - tr.ay;
      const u = (dx * tr.v1y - dy * tr.v1x) / tr.denom;
      const v = (tr.v0x * dy - tr.v0y * dx) / tr.denom;
      const w = 1 - u - v;
      if (u >= -1e-6 && v >= -1e-6 && w >= -1e-6) {
        best = tr;
        bestDist = 0;
        bestU = Math.max(0, Math.min(1, u));
        bestV = Math.max(0, Math.min(1 - bestU, v));
        break;
      }
      const [cu, cv, d] = closestPointOnTriangle2d(fx, fy, tr.ax, tr.ay, tr.bx, tr.by, tr.cx0, tr.cy0);
      if (d < bestDist) {
        bestDist = d;
        best = tr;
        bestU = cu;
        bestV = cv;
      }
    }

    if (!best || bestDist > toleranceMm) {
      unmatchedStitches++;
      return {
        ...r,
        flatMm: [Number(fx.toFixed(2)), Number(fy.toFixed(2))],
        onSurface: false,
        reason: "outside-panel-tolerance",
        tri: null,
        bary: null,
        residualMm: Number.isFinite(bestDist) ? Number(bestDist.toFixed(4)) : null,
        pos3d: null,
        normal3d: null,
      };
    }

    registeredStitches++;
    if (bestDist > maxResidualMm) maxResidualMm = bestDist;
    const w0 = 1 - bestU - bestV;
    const { a, b, c, normal } = best;
    const px = w0 * clothUpright[a * 3] + bestU * clothUpright[b * 3] + bestV * clothUpright[c * 3];
    const py = w0 * clothUpright[a * 3 + 1] + bestU * clothUpright[b * 3 + 1] + bestV * clothUpright[c * 3 + 1];
    const pz = w0 * clothUpright[a * 3 + 2] + bestU * clothUpright[b * 3 + 2] + bestV * clothUpright[c * 3 + 2];

    return {
      ...r,
      flatMm: [Number(fx.toFixed(2)), Number(fy.toFixed(2))],
      onSurface: true,
      reason: "registered",
      tri: best.ti,
      bary: [Number(w0.toFixed(5)), Number(bestU.toFixed(5)), Number(bestV.toFixed(5))],
      residualMm: Number(bestDist.toFixed(4)),
      pos3d: [px, py, pz],
      normal3d: normal,
    };
  });

  return {
    panelName,
    registered: true,
    toleranceMm,
    centreFlatMm: [Number(cx.toFixed(3)), Number(cy.toFixed(3))],
    flatBoundsMm: [fMinX, fMinY, fMaxX, fMaxY],
    registeredStitches,
    unmatchedStitches,
    flatOnlyJumps,
    maxResidualMm: Number(maxResidualMm.toFixed(4)),
    points,
  };
}
