// Redraft the Garden Sundress after a native flat ease warning using real Rust/WASM
// `alter_pattern` and `run` (ftx-ahm).
//
// Invariants:
// - The fixed first draft (`GARDEN_SUNDRESS`) and its export names (`GARDEN_SUNDRESS_A4.pdf`,
//   `GARDEN_SUNDRESS_master_mm.svg`) are always preserved untouched.
// - An unchanged request (zero bust and waist delta) runs zero alterations and produces identical
//   outlines and seam lengths.
// - An increase in required bust or waist ease applies symmetric native `alter_pattern` `move`
//   operations across bodice & skirt panels, re-runs full native flat validation + export checks,
//   and names the checked second cut distinctly (`SUNDRESS_REDRAFT_*`).
// - Never silently clamp an invalid geometry: any refusal from `ftx` surfaces directly.
import { getStudioEngine } from "./studio-engine.mjs";

const TODAY = "2026-10-12";
const CREATED = "2026-10-12T00:00:00Z";

export const DEFAULT_BUST_BAND_MM = [20, 40, 65];
export const DEFAULT_WAIST_BAND_MM = [25, 60, 90];

export const SUNDRESS_WEARERS = [
  {
    id: "close",
    label: "Close (93.5 / 77.0 cm)",
    chestCm: 93.5,
    waistCm: 77.0,
    note: "Fixed 94 / 78 cm dress falls below minimum bust (+5 mm) and waist (+10 mm) ease.",
  },
  {
    id: "beyond",
    label: "Beyond garment (96.5 / 80.5 cm)",
    chestCm: 96.5,
    waistCm: 80.5,
    note: "Model exceeds the fixed dress by 25 mm at both bust and waist (negative ease).",
  },
  {
    id: "reference",
    label: "Reference (90.0 / 72.0 cm)",
    chestCm: 90.0,
    waistCm: 72.0,
    note: "Nominal reference form: unchanged ease request matches the fixed draft 1:1.",
  },
];

export function buildFitSpec(reqBustEaseMm = 40, reqWaistEaseMm = 60) {
  const bustBand =
    reqBustEaseMm === DEFAULT_BUST_BAND_MM[1]
      ? [...DEFAULT_BUST_BAND_MM]
      : [
          Math.max(0, Math.round(reqBustEaseMm - 20)),
          Math.round(reqBustEaseMm),
          Math.round(reqBustEaseMm + 25),
        ];
  const waistBand =
    reqWaistEaseMm === DEFAULT_WAIST_BAND_MM[1]
      ? [...DEFAULT_WAIST_BAND_MM]
      : [
          Math.max(0, Math.round(reqWaistEaseMm - 35)),
          Math.round(reqWaistEaseMm),
          Math.round(reqWaistEaseMm + 30),
        ];
  return {
    schema: "fabric-fit/1",
    garment: "Garden sundress",
    points: [
      {
        id: "bust",
        label: "Finished bodice bust",
        kind: "girth",
        instances: ["bodice_front", "bodice_back"],
        landmark: "waist",
        dy: 200,
        body: "chest",
        target_mm: bustBand,
      },
      {
        id: "waist",
        label: "Finished waist",
        kind: "path",
        refs: ["bodice_front/waist", "bodice_back/waist"],
        body: "waist",
        target_mm: waistBand,
      },
    ],
  };
}

function makeBodyRequest(chestCm, waistCm) {
  return {
    mode: "tape",
    stature_cm: 168,
    age_months: 360,
    sex: "female",
    tape: {
      chest: Number(chestCm),
      waist: Number(waistCm),
      hip: Number(Math.max(96, waistCm + 20).toFixed(1)),
      waist_height: 101.0,
      back_neck_to_waist: 42.5,
      shoulder_slope_deg: 15,
      neck_base: 35.0,
      shoulder_length: 12.5,
      armscye_depth: 21.5,
      crotch_height: 77.0,
      head: 55.0,
      knee_height: 47.0,
      arm_length: 56.5,
      hip_width: 34.5,
    },
  };
}

function fnvHex(bytes) {
  // Deterministic lightweight fingerprint for quick byte-identity checks in worker & UI.
  let h1 = 0x811c9dc5;
  let h2 = 0x9e3779b9;
  for (let i = 0; i < bytes.length; i++) {
    const b = bytes[i];
    h1 = Math.imul(h1 ^ b, 0x01000193) >>> 0;
    h2 = Math.imul(h2 ^ ((b + i) & 0xff), 0x85ebca6b) >>> 0;
  }
  return h1.toString(16).padStart(8, "0") + h2.toString(16).padStart(8, "0");
}

function readRound(round) {
  try {
    const page = JSON.parse(round.page());
    if (page.errors?.length) {
      throw new Error(page.errors.map(e => (typeof e === "string" ? e : e.message)).join(" · "));
    }
    const files = {};
    for (const [type, suffix] of [
      ["pdf", "_A4.pdf"],
      ["svg", "_master_mm.svg"],
    ]) {
      const source = page.files?.find(file => file.name.endsWith(suffix));
      if (!source) continue;
      const bytes = round.file(source.name);
      files[type] = { bytes, name: source.name, byteLength: bytes.length, fingerprint: fnvHex(bytes) };
    }
    const patBytes = round.file("pattern.json");
    const patternJson = patBytes ? new TextDecoder("utf-8", { fatal: true }).decode(patBytes) : null;
    const seams = page.validation?.seams ?? [];
    const points = page.validation?.points ?? [];
    return {
      status: page.basis?.flat ?? "UNKNOWN",
      points,
      seams,
      passedSeams: seams.filter(s => s.status === "PASS").length,
      totalSeams: seams.length,
      checks: page.exports?.checks ?? null,
      patternJson,
      files,
    };
  } finally {
    round.free();
  }
}

// Evaluate chord-relative edge curves using the exact formula from `ftx-core/src/curve.rs`:
// `[t, h]` is `p0 + t*d + h*perp(d)` with `d = p1 - p0`, `perp(d) = (-d_y, d_x)`.
function samplePiecePolyline(piece, stepsPerCurve = 24) {
  const verts = piece.vertices;
  const pts = [];
  for (const edge of piece.edges) {
    const p0 = verts[edge.from];
    const p1 = verts[edge.to];
    if (!p0 || !p1) continue;
    const dx = p1[0] - p0[0];
    const dy = p1[1] - p0[1];
    const px = -dy;
    const py = dx;
    const curve = edge.curve;
    if (curve?.type === "cubic" && Array.isArray(curve.c) && curve.c.length === 2) {
      const [t1, h1] = curve.c[0];
      const [t2, h2] = curve.c[1];
      const c1x = p0[0] + t1 * dx + h1 * px;
      const c1y = p0[1] + t1 * dy + h1 * py;
      const c2x = p0[0] + t2 * dx + h2 * px;
      const c2y = p0[1] + t2 * dy + h2 * py;
      for (let i = 0; i < stepsPerCurve; i++) {
        const u = i / stepsPerCurve;
        const m = 1 - u;
        const x = m * m * m * p0[0] + 3 * m * m * u * c1x + 3 * m * u * u * c2x + u * u * u * p1[0];
        const y = m * m * m * p0[1] + 3 * m * m * u * c1y + 3 * m * u * u * c2y + u * u * u * p1[1];
        pts.push([x, y]);
      }
    } else if (curve?.type === "quad" && Array.isArray(curve.c) && curve.c.length === 1) {
      const [t1, h1] = curve.c[0];
      const c1x = p0[0] + t1 * dx + h1 * px;
      const c1y = p0[1] + t1 * dy + h1 * py;
      for (let i = 0; i < stepsPerCurve; i++) {
        const u = i / stepsPerCurve;
        const m = 1 - u;
        const x = m * m * p0[0] + 2 * m * u * c1x + u * u * p1[0];
        const y = m * m * p0[1] + 2 * m * u * c1y + u * u * p1[1];
        pts.push([x, y]);
      }
    } else {
      pts.push([p0[0], p0[1]]);
    }
  }
  return pts;
}

function pieceMetrics(piece) {
  const poly = samplePiecePolyline(piece, 24);
  let minX = Infinity, maxX = -Infinity, minY = Infinity, maxY = -Infinity;
  let area2 = 0;
  for (let i = 0; i < poly.length; i++) {
    const [x, y] = poly[i];
    const [nx, ny] = poly[(i + 1) % poly.length];
    if (x < minX) minX = x;
    if (x > maxX) maxX = x;
    if (y < minY) minY = y;
    if (y > maxY) maxY = y;
    area2 += x * ny - nx * y;
  }
  const path =
    poly.map(([x, y], idx) => `${idx === 0 ? "M" : "L"}${x.toFixed(2)},${(-y).toFixed(2)}`).join(" ") + " Z";
  const bustWidthMm =
    piece.vertices.bl && piece.vertices.br
      ? Number((piece.vertices.br[0] - piece.vertices.bl[0]).toFixed(2))
      : null;
  const waistWidthMm =
    piece.vertices.wl && piece.vertices.wr
      ? Number((piece.vertices.wr[0] - piece.vertices.wl[0]).toFixed(2))
      : null;
  return {
    id: piece.id,
    name: piece.name,
    path,
    bounds: [minX, -maxY, maxX, -minY], // SVG y-down bounds
    widthMm: Number((maxX - minX).toFixed(2)),
    heightMm: Number((maxY - minY).toFixed(2)),
    bustWidthMm,
    waistWidthMm,
    areaCm2: Number((Math.abs(area2) / 200).toFixed(2)),
  };
}

function comparePanels(firstPatternObj, secondPatternObj) {
  const panels = [];
  for (const p1 of firstPatternObj.pieces) {
    const p2 = secondPatternObj.pieces.find(p => p.id === p1.id);
    if (!p2) continue;
    const m1 = pieceMetrics(p1);
    const m2 = pieceMetrics(p2);
    let maxVertexShiftMm = 0;
    const movedVertices = [];
    for (const [vid, v1] of Object.entries(p1.vertices)) {
      const v2 = p2.vertices[vid];
      if (!v2) continue;
      const d = Math.hypot(v2[0] - v1[0], v2[1] - v1[1]);
      if (d > maxVertexShiftMm) maxVertexShiftMm = d;
      if (d > 1e-4) {
        movedVertices.push({
          id: vid,
          from: [Number(v1[0].toFixed(2)), Number(v1[1].toFixed(2))],
          to: [Number(v2[0].toFixed(2)), Number(v2[1].toFixed(2))],
          shiftMm: Number(d.toFixed(2)),
        });
      }
    }
    panels.push({
      id: p1.id,
      name: p1.name,
      firstPath: m1.path,
      secondPath: m2.path,
      bounds: [
        Math.min(m1.bounds[0], m2.bounds[0]),
        Math.min(m1.bounds[1], m2.bounds[1]),
        Math.max(m1.bounds[2], m2.bounds[2]),
        Math.max(m1.bounds[3], m2.bounds[3]),
      ],
      firstWidthMm: m1.widthMm,
      secondWidthMm: m2.widthMm,
      deltaWidthMm: Number((m2.widthMm - m1.widthMm).toFixed(2)),
      firstBustWidthMm: m1.bustWidthMm,
      secondBustWidthMm: m2.bustWidthMm,
      deltaBustWidthMm:
        m1.bustWidthMm !== null && m2.bustWidthMm !== null
          ? Number((m2.bustWidthMm - m1.bustWidthMm).toFixed(2))
          : null,
      firstWaistWidthMm: m1.waistWidthMm,
      secondWaistWidthMm: m2.waistWidthMm,
      deltaWaistWidthMm:
        m1.waistWidthMm !== null && m2.waistWidthMm !== null
          ? Number((m2.waistWidthMm - m1.waistWidthMm).toFixed(2))
          : null,
      firstAreaCm2: m1.areaCm2,
      secondAreaCm2: m2.areaCm2,
      deltaAreaCm2: Number((m2.areaCm2 - m1.areaCm2).toFixed(2)),
      maxVertexShiftMm: Number(maxVertexShiftMm.toFixed(3)),
      movedVertices,
    });
  }
  return panels;
}

export function runSundressRedraft(
  ftx,
  { basePatternJson, cut = "dress", chestCm = 93.5, waistCm = 77.0, reqBustEaseMm = 40, reqWaistEaseMm = 60 },
) {
  if (!basePatternJson || typeof basePatternJson !== "string") {
    throw new Error("Base sundress pattern JSON is required.");
  }
  const chest = Number(chestCm);
  const waist = Number(waistCm);
  const reqBust = Number(reqBustEaseMm);
  const reqWaist = Number(reqWaistEaseMm);
  if (![chest, waist, reqBust, reqWaist].every(Number.isFinite)) {
    throw new Error("Wearer measurements and ease targets must be finite numbers.");
  }
  if (chest < 78 || chest > 112 || waist < 58 || waist > 98) {
    throw new Error("Wearer bust (78–112 cm) or waist (58–98 cm) is outside this study's range.");
  }
  if (reqBust < 0 || reqBust > 120 || reqWaist < 0 || reqWaist > 140) {
    throw new Error("Requested bust or waist ease is outside this study's range.");
  }

  const baseObj = JSON.parse(basePatternJson);
  const fitSpec = buildFitSpec(reqBust, reqWaist);
  const body = makeBodyRequest(chest, waist);

  // 1. Evaluate the fixed first draft completely untouched — preserving its exact geometry & export names.
  const first = readRound(
    ftx.run(JSON.stringify({ body, pattern: baseObj, fit_spec: fitSpec }), TODAY, CREATED),
  );

  const firstBustPt = first.points.find(p => p.id === "bust");
  const firstWaistPt = first.points.find(p => p.id === "waist");
  if (!firstBustPt || !firstWaistPt || firstBustPt.finished_mm == null || firstWaistPt.finished_mm == null) {
    throw new Error("Could not read finished bust and waist points from fixed sundress validation.");
  }

  const targetFinishedBustMm = Number((chest * 10 + reqBust).toFixed(3));
  const targetFinishedWaistMm = Number((waist * 10 + reqWaist).toFixed(3));
  const deltaBustMm = Number((targetFinishedBustMm - firstBustPt.finished_mm).toFixed(3));
  const deltaWaistMm = Number((targetFinishedWaistMm - firstWaistPt.finished_mm).toFixed(3));
  const unchanged = Math.abs(deltaBustMm) < 1e-6 && Math.abs(deltaWaistMm) < 1e-6;

  // Read the authoritative gather ratio straight from the pattern document (1.3 gathered vs 1.05 lightly eased).
  const gatherSeam = baseObj.seams?.find(s => s.id === "gather_front");
  const gatherRatio = Number(gatherSeam?.ratio ?? 1.3);

  // 2. Apply real native `ftx.alter_pattern` operations symmetrically across front/back bodice and skirt.
  let alteredJson = basePatternJson;
  const appliedOps = [];
  if (!unchanged) {
    const db = Number((deltaBustMm / 4).toFixed(4));
    const dw = Number((deltaWaistMm / 4).toFixed(4));
    const ds = Number((dw * gatherRatio).toFixed(4));
    const ops = [];
    for (const piece of ["bodice_front", "bodice_back"]) {
      if (Math.abs(db) >= 1e-6) {
        ops.push({ op: "move", piece, vertex: "br", dx: db, dy: 0 });
        ops.push({ op: "move", piece, vertex: "ur", dx: db, dy: 0 });
        ops.push({ op: "move", piece, vertex: "bl", dx: -db, dy: 0 });
        ops.push({ op: "move", piece, vertex: "ul", dx: -db, dy: 0 });
      }
      if (Math.abs(dw) >= 1e-6) {
        ops.push({ op: "move", piece, vertex: "wr", dx: dw, dy: 0 });
        ops.push({ op: "move", piece, vertex: "wl", dx: -dw, dy: 0 });
      }
    }
    if (Math.abs(ds) >= 1e-6) {
      for (const piece of ["skirt_front", "skirt_back"]) {
        ops.push({ op: "move", piece, vertex: "wr", dx: ds, dy: 0 });
        ops.push({ op: "move", piece, vertex: "wl", dx: -ds, dy: 0 });
      }
    }
    for (const op of ops) {
      const res = JSON.parse(ftx.alter_pattern(alteredJson, JSON.stringify(op), null));
      alteredJson = res.pattern_json;
      appliedOps.push({ op, why: res.why });
    }
  }

  const secondObj = JSON.parse(alteredJson);
  // Keep a distinct export stem (`SUNDRESS_REDRAFT`) so second-cut files never overwrite the preserved first draft.
  secondObj.style = "SUNDRESS_REDRAFT";
  secondObj.meta = {
    ...(secondObj.meta ?? {}),
    title: unchanged ? "Garden sundress (unchanged check)" : "Garden sundress (redrafted)",
    size_label: `Synthetic adult ${chest.toFixed(1)}/${waist.toFixed(1)} cm · ease +${reqBust}/+${reqWaist} mm`,
  };

  const second = readRound(
    ftx.run(JSON.stringify({ body, pattern: secondObj, fit_spec: fitSpec }), TODAY, CREATED),
  );

  const firstObjResolved = JSON.parse(first.patternJson ?? basePatternJson);
  const secondObjResolved = JSON.parse(second.patternJson ?? JSON.stringify(secondObj));
  const panels = comparePanels(firstObjResolved, secondObjResolved);
  const seamDeltas = first.seams.map(s1 => {
    const s2 = second.seams.find(x => x.seam === s1.seam);
    return {
      seam: s1.seam,
      a: s1.a,
      b: s1.b,
      firstLenAMm: s1.len_a_mm,
      firstLenBMm: s1.len_b_mm,
      secondLenAMm: s2?.len_a_mm ?? s1.len_a_mm,
      secondLenBMm: s2?.len_b_mm ?? s1.len_b_mm,
      deltaAMm: Number(((s2?.len_a_mm ?? s1.len_a_mm) - s1.len_a_mm).toFixed(2)),
      deltaBMm: Number(((s2?.len_b_mm ?? s1.len_b_mm) - s1.len_b_mm).toFixed(2)),
      firstStatus: s1.status,
      secondStatus: s2?.status ?? "MISSING",
    };
  });

  const maxPanelShiftMm = panels.reduce((m, p) => Math.max(m, p.maxVertexShiftMm), 0);
  const maxSeamDeltaMm = seamDeltas.reduce(
    (m, s) => Math.max(m, Math.abs(s.deltaAMm), Math.abs(s.deltaBMm)),
    0,
  );

  return {
    request: { cut, chestCm: chest, waistCm: waist, reqBustEaseMm: reqBust, reqWaistEaseMm: reqWaist },
    fitSpec,
    gatherRatio,
    unchanged,
    outlinesIdentical: maxPanelShiftMm === 0 && maxSeamDeltaMm === 0,
    deltaBustMm,
    deltaWaistMm,
    maxPanelShiftMm: Number(maxPanelShiftMm.toFixed(3)),
    maxSeamDeltaMm: Number(maxSeamDeltaMm.toFixed(2)),
    appliedOpsCount: appliedOps.length,
    history: appliedOps.map(a => a.why),
    panels,
    seamDeltas,
    first,
    second,
  };
}

if (typeof DedicatedWorkerGlobalScope !== "undefined" && self instanceof DedicatedWorkerGlobalScope) {
  let latest;
  const cache = new Map();
  async function loadBasePattern(cut) {
    if (cache.has(cut)) return cache.get(cut);
    const url = cut === "light" ? "/exhibits/sundress/light-pattern.json" : "/exhibits/sundress/dress-pattern.json";
    const res = await fetch(url);
    if (!res.ok) throw new Error(`Could not load ${url} (${res.status}).`);
    const text = await res.text();
    cache.set(cut, text);
    return text;
  }
  self.onmessage = async ({ data }) => {
    latest = data.id;
    try {
      const [ftx, basePatternJson] = await Promise.all([getStudioEngine(), loadBasePattern(data.cut)]);
      if (latest !== data.id) return;
      const result = runSundressRedraft(ftx, {
        basePatternJson,
        cut: data.cut,
        chestCm: data.chestCm,
        waistCm: data.waistCm,
        reqBustEaseMm: data.reqBustEaseMm,
        reqWaistEaseMm: data.reqWaistEaseMm,
      });
      if (latest !== data.id) return;
      const transfers = [];
      for (const stage of [result.first, result.second]) {
        for (const file of Object.values(stage.files)) {
          if (file?.bytes?.buffer) transfers.push(file.bytes.buffer);
        }
      }
      self.postMessage({ id: data.id, result }, transfers);
    } catch (error) {
      if (latest === data.id) {
        self.postMessage({ id: data.id, error: error instanceof Error ? error.message : String(error) });
      }
    }
  };
}
