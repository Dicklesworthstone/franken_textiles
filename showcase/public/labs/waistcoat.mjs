// Pure geometry, flat-pattern landmark, and layer-seam helpers for the documented
// 1780s Men's Waistcoat (Victoria and Albert Museum 256-1880 / O112019), shared by
// /wardrobe/waistcoat/ (ftx-n4x), /wardrobe/waistcoat/inside-out/ (ftx-z55), and tests.
//
// Invariants:
// - Every panel outline, internal mark, notch, and 0-based seam index comes directly from
//   waistcoat-pattern.json & waistcoat-manifest.json produced by native `ftx validate`.
// - Toggling layer visibility or separating layers radially NEVER changes any seam index.
// - Missing linings and unpaired edges are surfaced directly from the native edge audit.

export const WELT_STAGES = [
  {
    id: "mark",
    step: 1,
    title: "Mark welt box & stitch welt strip",
    summary:
      "Transfer the 136 mm slash line (y = 152 mm), stitch box (y = 140 & 164 mm), and corner clip marks onto wc_front. Fold the 136 × 48 mm silk welt lengthwise to 24 mm and stitch it along the lower line.",
    pieces: ["wc_front", "wc_welt", "wc_pocket_outer", "wc_pocket_inner"],
  },
  {
    id: "slash",
    step: 2,
    title: "Slash opening & clip corners to stitch line",
    summary:
      "Cut along the 136 mm centre slash line between x = 96 mm and x = 232 mm, stopping 12 mm from each end and clipping diagonally into all four box corners without cutting the stitching.",
    pieces: ["wc_front", "wc_welt"],
  },
  {
    id: "turn",
    step: 3,
    title: "Turn welt & linen pocket bags to the inside",
    summary:
      "Push the silk welt and upper/lower unbleached linen pocket bags (136 × 156 mm) through the slash to the inside of the front shell, press the 24 mm welt lip upward to fill the opening, and stitch the small clipped corner triangles cleanly to the welt ends.",
    pieces: ["wc_front", "wc_welt", "wc_pocket_outer", "wc_pocket_inner"],
  },
  {
    id: "finish",
    step: 4,
    title: "Close pocket bag & cover with front lining",
    summary:
      "Close the outer and inner linen pocket bags around left, bottom, and right seams (indices 30–35) so the bag hangs inside between wc_front and wc_front_lining, leaving only the crisp 136 × 24 mm silk welt visible on the front.",
    pieces: ["wc_front", "wc_welt", "wc_pocket_outer", "wc_pocket_inner", "wc_front_lining"],
  },
];

function evalCubic(a, b, c0, c1, t) {
  const dx = b[0] - a[0];
  const dy = b[1] - a[1];
  const px = -dy;
  const py = dx;
  const p1x = a[0] + c0[0] * dx + c0[1] * px;
  const p1y = a[1] + c0[0] * dy + c0[1] * py;
  const p2x = a[0] + c1[0] * dx + c1[1] * px;
  const p2y = a[1] + c1[0] * dy + c1[1] * py;
  const u = 1 - t;
  return [
    u * u * u * a[0] + 3 * u * u * t * p1x + 3 * u * t * t * p2x + t * t * t * b[0],
    u * u * u * a[1] + 3 * u * u * t * p1y + 3 * u * t * t * p2y + t * t * t * b[1],
  ];
}

export function sampleEdgePoints(piece, edge, steps = 20, mirrorX = false) {
  const a0 = piece.vertices[edge.from];
  const b0 = piece.vertices[edge.to];
  if (!a0 || !b0) throw new Error(`Missing vertex on ${piece.id}/${edge.id}`);
  const pts = [];
  if (edge.curve && edge.curve.type === "cubic" && edge.curve.c?.length === 2) {
    const [c0, c1] = edge.curve.c;
    for (let i = 0; i <= steps; i++) {
      const [x, y] = evalCubic(a0, b0, c0, c1, i / steps);
      pts.push([mirrorX ? -x : x, y]);
    }
  } else {
    pts.push([mirrorX ? -a0[0] : a0[0], a0[1]]);
    pts.push([mirrorX ? -b0[0] : b0[0], b0[1]]);
  }
  return pts;
}

export function buildPieceOutline(pattern, pieceId, side = "R") {
  const piece = pattern.pieces.find(p => p.id === pieceId);
  if (!piece) throw new Error(`Unknown piece: ${pieceId}`);
  const mirrorX = side === "L" && piece.cut?.qty === 2;
  const edgeGeoms = [];
  const allPts = [];
  let minX = Infinity;
  let maxX = -Infinity;
  let minY = Infinity;
  let maxY = -Infinity;

  for (const edge of piece.edges) {
    const pts = sampleEdgePoints(piece, edge, edge.curve ? 22 : 1, mirrorX);
    edgeGeoms.push({ id: edge.id, pts });
    for (let i = 0; i < pts.length; i++) {
      const [x, y] = pts[i];
      if (x < minX) minX = x;
      if (x > maxX) maxX = x;
      if (y < minY) minY = y;
      if (y > maxY) maxY = y;
      if (i > 0 || allPts.length === 0) allPts.push([x, y]);
    }
  }

  const internals = (piece.internal ?? []).map(item => ({
    kind: item.kind,
    label: item.label,
    pts: item.pts.map(([x, y]) => [mirrorX ? -x : x, y]),
  }));

  const notches = (piece.notches ?? []).map(n => {
    const edgeObj = edgeGeoms.find(e => e.id === n.edge);
    if (!edgeObj || edgeObj.pts.length < 2) return null;
    // Compute cumulative arc length to interpolate exact notch point at parameter s.
    const pts = edgeObj.pts;
    let total = 0;
    const cum = [0];
    for (let i = 1; i < pts.length; i++) {
      total += Math.hypot(pts[i][0] - pts[i - 1][0], pts[i][1] - pts[i - 1][1]);
      cum.push(total);
    }
    const target = n.s * total;
    let pt = pts[0];
    for (let i = 1; i < pts.length; i++) {
      if (cum[i] >= target || i === pts.length - 1) {
        const seg = Math.max(1e-9, cum[i] - cum[i - 1]);
        const frac = Math.min(1, Math.max(0, (target - cum[i - 1]) / seg));
        pt = [
          pts[i - 1][0] + frac * (pts[i][0] - pts[i - 1][0]),
          pts[i - 1][1] + frac * (pts[i][1] - pts[i - 1][1]),
        ];
        break;
      }
    }
    return { id: n.id, edge: n.edge, s: n.s, type: n.type, pt };
  }).filter(Boolean);

  return {
    id: piece.id,
    instance: piece.cut?.qty === 2 ? `${piece.id}.${side}` : piece.id,
    name: piece.name,
    material: piece.cut?.material ?? "",
    qty: piece.cut?.qty ?? 1,
    note: piece.note ?? "",
    widthMm: Number((maxX - minX).toFixed(1)),
    heightMm: Number((maxY - minY).toFixed(1)),
    bounds: { minX, maxX, minY, maxY },
    polygon: allPts,
    edges: edgeGeoms,
    internals,
    notches,
    grain: piece.grain ? piece.grain.map(([x, y]) => [mirrorX ? -x : x, y]) : [],
  };
}

export function getSeamsForInstance(manifest, instanceId) {
  return manifest.seams.filter(s => s.instanceA === instanceId || s.instanceB === instanceId);
}

export function getUnpairedForInstance(manifest, instanceId) {
  return manifest.unpairedAndMissing.filter(u => u.instance === instanceId);
}

/**
 * Pure state selector for the inside-out layered garment explorer (ftx-z55).
 * Proves that toggling shell/facing/lining visibility or turning layers apart (explodeMm)
 * NEVER mutates or renumbers any native seam index.
 */
export function buildInsideOutState(
  manifest,
  { showShell = true, showFacing = true, showLining = true, showPockets = true, explodeMm = 0, selectedSeamIndex = null, selectedUnpairedRef = null } = {},
) {
  const layerVisible = {
    shell: Boolean(showShell),
    facing: Boolean(showFacing),
    lining: Boolean(showLining),
    pocket: Boolean(showPockets),
  };

  // Radial offsets (mm) around base elliptical torso: turning layers apart moves shell outward,
  // pocket tucked just inside shell, facing inside shell front, and lining inward.
  const sep = Math.max(0, Math.min(90, Number(explodeMm) || 0));
  const radialOffsetMm = {
    shell: 6 + sep * 1.15,
    pocket: 3 + sep * 0.45,
    facing: 1.5 + sep * 0.25,
    lining: -2 - sep * 0.85,
  };

  // Preserve exact 0-based native seam indices unconditionally!
  const seams = manifest.seams.map(s => ({
    ...s,
    index: s.index,
    visibleA: layerVisible[s.layerA],
    visibleB: layerVisible[s.layerB],
    bothVisible: layerVisible[s.layerA] && layerVisible[s.layerB],
    isSelected: selectedSeamIndex === s.index,
  }));

  const selectedSeam = selectedSeamIndex !== null ? seams.find(s => s.index === selectedSeamIndex) ?? null : null;
  const selectedUnpaired =
    selectedUnpairedRef !== null
      ? manifest.unpairedAndMissing.find(u => u.ref === selectedUnpairedRef) ?? null
      : null;

  return {
    layerVisible,
    explodeMm: sep,
    radialOffsetMm,
    seams,
    unpairedAndMissing: manifest.unpairedAndMissing,
    selectedSeam,
    selectedUnpaired,
    missingLiningCount: manifest.unpairedAndMissing.filter(u => u.kind === "MISSING_LINING").length,
    unpairedEdgeCount: manifest.unpairedAndMissing.filter(u => u.kind === "UNPAIRED_EDGE").length,
    internalSlashCount: manifest.unpairedAndMissing.filter(u => u.kind === "INTERNAL_SLASH_ATTACHMENT").length,
  };
}

/**
 * Map a 2D pattern point (xMm, yMm) of a waistcoat instance onto a 3D elliptical torso surface
 * (metres, Y-up for Three.js, front facing +Z, wearer's right at -X so viewer sees right side on left).
 */
export function projectWaistcoatPointTo3D(instanceId, xMm, yMm, radialOffsetMm = 0) {
  // Base elliptical torso semi-axes matching 1075 mm finished chest circumference:
  // Circumference of ellipse (a=0.188 m, b=0.148 m) ≈ 1.062 m (plus ease).
  const aM = 0.188 + radialOffsetMm / 1000;
  const bM = 0.148 + radialOffsetMm / 1000;
  const halfChestMm = 537.5; // 275 (front) + 262.5 (back)
  const yCenterM = (yMm - 345) / 1000; // center garment vertically around y = 0 in Three.js

  const basePiece = instanceId.split(".")[0];
  const isLeft = instanceId.endsWith(".L");

  // Compute signed arc position sMm around torso where sMm = 0 is Centre Front (+Z),
  // +275 is Wearer's Right Side (-X), +537.5 is Centre Back (-Z), -275 is Wearer's Left Side (+X).
  let sMm = 0;
  let yAdjustedM = yCenterM;

  if (basePiece === "wc_front" || basePiece === "wc_facing" || basePiece === "wc_front_lining") {
    // xMm is [0..275] on .R (and [-275..0] on .L after mirrorX).
    // Wait: xMm passed here is already mirrored for .L (so xMm in [-275..0]), so sMm = xMm directly!
    sMm = xMm;
  } else if (basePiece === "wc_back" || basePiece === "wc_back_lining") {
    // On .R, xMm is in [-262.5..0] where x=-262.5 is right side (s=+275) and x=0 is CB (s=+537.5).
    // On .L (mirrored), xMm is in [0..+262.5] where x=+262.5 is left side (s=-275) and x=0 is CB (s=-537.5).
    if (!isLeft) {
      sMm = 537.5 + xMm; // x=-262.5 -> +275; x=0 -> +537.5
    } else {
      sMm = -537.5 + xMm; // x=+262.5 -> -275; x=0 -> -537.5
    }
  } else if (basePiece === "wc_collar") {
    // Collar spans xMm in [-229.8..+229.8] centered at CB (x=0 -> theta=PI)!
    // Scale onto a tighter neck cylinder (aNeck=0.078 m, bNeck=0.072 m) at y = +0.315..+0.363 m!
    const aNeck = 0.078 + radialOffsetMm / 1000;
    const bNeck = 0.072 + radialOffsetMm / 1000;
    const theta = Math.PI - (xMm / 229.8) * Math.PI * 0.88;
    const x3 = -aNeck * Math.sin(theta);
    const z3 = bNeck * Math.cos(theta);
    const y3 = (648 + yMm - 345) / 1000;
    return [x3, y3, z3];
  } else if (basePiece === "wc_welt") {
    // Welt sits directly on the front pocket slash: x in [96..232] (R) or [-232..-96] (L), y in [140..164].
    const u = isLeft ? -xMm : xMm; // [0..136]
    const sLocal = 96 + u;
    sMm = isLeft ? -sLocal : sLocal;
    // Folded welt spans y = 140..164 mm (map 0..48 mm to 140..164 mm lip).
    yAdjustedM = (140 + yMm * 0.5 - 345) / 1000;
  } else if (basePiece === "wc_pocket_outer" || basePiece === "wc_pocket_inner") {
    // Pocket bags hang directly below the 136 mm slash (y = 152 down to y = -4 mm, x in [96..232]).
    const u = isLeft ? -xMm : xMm; // [0..136]
    const sLocal = 96 + u;
    sMm = isLeft ? -sLocal : sLocal;
    yAdjustedM = (152 - (156 - yMm) - 345) / 1000;
  }

  // Taper torso gently inward toward high shoulder & neckline above armscye (y > 435 mm)
  // so shoulder and neck curves hug the upper chest naturally instead of sticking out as a flat tube.
  const upperT = Math.max(0, Math.min(1, (yMm - 435) / 255));
  const taper = 1 - upperT * 0.24;
  const theta = (sMm / halfChestMm) * Math.PI; // 0 = front (+Z), +PI = back (-Z)
  const x3 = -aM * taper * Math.sin(theta);
  const z3 = bM * taper * Math.cos(theta);
  return [x3, yAdjustedM, z3];
}
