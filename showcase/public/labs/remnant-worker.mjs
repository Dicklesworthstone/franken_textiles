// Two-job finite-stock remnant planner using the exact native Rust/WASM `stock_job` planner.
// Job 1 consumes a declared finite cotton muslin sheet and emits a frozen `residual_json` receipt
// carrying the exact stock ID and exterior cut polygons as `occupied_mm`.
// Job 2 runs directly on that returned `residual_json` — never overlapping Job 1 or assuming a fresh roll.
import { getStudioEngine } from "./studio-engine.mjs";

const TODAY = "2026-10-12";
const CREATED = "2026-10-12T00:00:00Z";
const DEFAULT_MATERIAL = "dharma/cotton-muslin-3.5oz";
const SHEET_WIDTH_MM = 1143; // 45 in bolt width matching dharma/cotton-muslin-3.5oz
// Candidate grid only. The native planner still checks the full cut polygons and clearances.
const SEARCH_STEP_MM = 30;

export const REMNANT_PRESETS = {
  bolero_fez: {
    id: "bolero_fez",
    label: "Bolero → Fez cap",
    job1Garment: "zouave_bolero",
    job2Garment: "fez",
    job2Copies: 1,
    sheetId: "muslin-sheet-01",
    sheetLengthMm: 720,
  },
  bolero_fez_2x: {
    id: "bolero_fez_2x",
    label: "Bolero → 2× Fez caps",
    job1Garment: "zouave_bolero",
    job2Garment: "fez",
    job2Copies: 2,
    sheetId: "muslin-sheet-01",
    sheetLengthMm: 720,
  },
  bolero_bolero_refusal: {
    id: "bolero_bolero_refusal",
    label: "Bolero → 2nd Bolero (refusal)",
    job1Garment: "zouave_bolero",
    job2Garment: "zouave_bolero",
    job2Copies: 1,
    sheetId: "muslin-sheet-01",
    sheetLengthMm: 720,
  },
  bolero_bolero_large: {
    id: "bolero_bolero_large",
    label: "Bolero → 2nd Bolero (1100 mm sheet)",
    job1Garment: "zouave_bolero",
    job2Garment: "zouave_bolero",
    job2Copies: 1,
    sheetId: "muslin-sheet-02",
    sheetLengthMm: 1100,
  },
};

export const initialRemnant = {
  job1Garment: "zouave_bolero",
  job2Garment: "fez",
  job2Copies: 1,
  sheetId: "muslin-sheet-01",
  sheetLengthMm: 720,
  material: DEFAULT_MATERIAL,
  stepMm: SEARCH_STEP_MM,
};

const draftedCache = new Map();

function boundPattern(ftx, garment, material, suppliedPattern) {
  let pattern = suppliedPattern;
  if (!pattern) {
    pattern = draftedCache.get(garment);
    if (!pattern) {
      const request = { body: { mode: "chart", stature_cm: 92 }, program: garment, style: {} };
      const round = ftx.run(JSON.stringify(request), TODAY, CREATED);
      try {
        pattern = new TextDecoder("utf-8", { fatal: true }).decode(round.file("pattern.json"));
        if (!pattern) throw new Error(`The size-chart draft for ${garment} produced no pattern.`);
        draftedCache.set(garment, pattern);
      } finally {
        round.free();
      }
    }
  }
  const changes = { schema: "ftx-material-bindings/1", default: { id: material }, pieces: {} };
  const bound = JSON.parse(ftx.material_bind(pattern, JSON.stringify(changes)));
  if (!bound?.pattern_json) throw new Error("Material binding returned no pattern.");
  return bound.pattern_json;
}

export function buildSheetRequest({
  sheetId = "muslin-sheet-01",
  sheetLengthMm = 720,
  material = DEFAULT_MATERIAL,
  copies = 1,
  gapMm = 4,
  boundaryMarginMm = 10,
  stepMm = SEARCH_STEP_MM,
  workLimit = 5000000,
}) {
  return {
    schema: "ftx-finite-stock-request/1",
    stocks: [
      {
        id: sheetId,
        material: { id: material },
        outline_mm: [
          [0, 0],
          [SHEET_WIDTH_MM, 0],
          [SHEET_WIDTH_MM, sheetLengthMm],
          [0, sheetLengthMm],
        ],
        defects_mm: [],
        occupied_mm: [],
        grain_mm: [
          [0, 0],
          [0, 100],
        ],
        geometry_source: {
          reference: `showcase:finite-sheet/${sheetId}`,
          accessed_on: TODAY,
          note: `Declared ${SHEET_WIDTH_MM} x ${sheetLengthMm} mm single-ply cut sheet in cutting-axis frame`,
        },
      },
    ],
    options: {
      copies,
      gap_mm: gapMm,
      boundary_margin_mm: boundaryMarginMm,
      step_mm: stepMm,
      work_limit: workLimit,
    },
  };
}

function summarizePlacement(p) {
  return {
    piece: p.piece,
    instance: p.instance,
    copy: p.copy,
    bounds_mm: p.bounds_mm,
    cut_mm: p.cut_mm,
    grain_mm: p.grain_mm,
    internal_cut_mm: p.internal_cut_mm || [],
    drills_mm: p.drills_mm || [],
  };
}

export function planRemnant(ftx, params = {}) {
  const config = { ...initialRemnant, ...params };
  const {
    job1Garment,
    job2Garment,
    job2Copies,
    sheetId,
    sheetLengthMm,
    material,
    stepMm,
    suppliedJob1Pattern,
  } = config;

  const pat1 = boundPattern(ftx, job1Garment, material, suppliedJob1Pattern);
  const pat2 = boundPattern(ftx, job2Garment, material, undefined);

  const req1Obj = buildSheetRequest({
    sheetId,
    sheetLengthMm,
    material,
    copies: 1,
    stepMm,
  });
  const req1Json = JSON.stringify(req1Obj);

  // Job 1: cut first garment from the intact finite sheet.
  const job1 = JSON.parse(ftx.stock_job(pat1, req1Json));
  if (job1.schema !== "ftx-stock-job/1" || !job1.plan_json || !job1.residual_json) {
    throw new Error("Job 1 finite-stock planner returned an unexpected payload.");
  }
  const plan1 = JSON.parse(job1.plan_json);
  const marker1 = plan1.markers[job1.stock_index];
  const residual1Obj = JSON.parse(job1.residual_json);
  const residualStock1 = residual1Obj.stocks[job1.stock_index];

  // Baseline fresh-roll cost for Job 2 alone (to quantify fabric saved by cutting from the remnant).
  const roll2Opts = {
    copies: job2Copies,
    gap_mm: 4,
    edge_margin_mm: 10,
    end_margin_mm: 10,
    nesting: { passes: 2, work_limit: 2500000 },
  };
  const roll2Job = JSON.parse(ftx.roll_job(pat2, JSON.stringify(roll2Opts), 0));
  const roll2Plan = JSON.parse(roll2Job.plan_json);
  const roll2 = roll2Plan.rolls[0];
  const materialRecord = roll2.material.authored;
  const sourceUrl = materialRecord.sources?.[materialRecord.properties?.width?.source]?.reference ?? null;

  // Job 2: feed the exact returned residual request (preserving stock ID, material snapshot,
  // occupied_mm exterior cut polygons, and occupied_basis receipt chain).
  let req2Json = job1.residual_json;
  if (job2Copies !== 1) {
    const adjusted = JSON.parse(job1.residual_json);
    adjusted.options.copies = job2Copies;
    req2Json = JSON.stringify(adjusted);
  }

  let job2Result = null;
  let job2Refusal = null;
  try {
    const job2 = JSON.parse(ftx.stock_job(pat2, req2Json));
    if (job2.schema !== "ftx-stock-job/1" || !job2.plan_json || !job2.residual_json) {
      throw new Error("Job 2 finite-stock planner returned an unexpected payload.");
    }
    const plan2 = JSON.parse(job2.plan_json);
    const marker2 = plan2.markers[job2.stock_index];
    const residual2Obj = JSON.parse(job2.residual_json);
    const residualStock2 = residual2Obj.stocks[job2.stock_index];
    if (marker2.id !== marker1.id) {
      throw new Error(`Stock identity drift: Job 1 used ${marker1.id}, Job 2 reported ${marker2.id}.`);
    }
    if (marker2.occupied_mm.length !== marker1.placements.length) {
      throw new Error("Job 2 occupied polygons do not match Job 1 placements.");
    }
    job2Result = {
      stockId: marker2.id,
      requestSha256: plan2.request_sha256,
      occupiedBasisIn: residualStock1.occupied_basis,
      occupiedBasisOut: residualStock2.occupied_basis,
      availableAreaM2: marker2.available_area_m2,
      externalCutAreaM2: marker2.external_cut_area_m2,
      utilizationOnSheet: marker2.utilization,
      combinedCutAreaM2: marker1.external_cut_area_m2 + marker2.external_cut_area_m2,
      combinedUtilization:
        (marker1.external_cut_area_m2 + marker2.external_cut_area_m2) / marker1.gross_area_m2,
      workUsed: plan2.work_used,
      candidates: plan2.candidates,
      occupiedPolygons: marker2.occupied_mm,
      placements: marker2.placements.map(summarizePlacement),
      svg: job2.preview_svg,
      residualJson: job2.residual_json,
    };
  } catch (err) {
    job2Refusal = err instanceof Error ? err.message : String(err);
  }

  return {
    config: {
      job1Garment,
      job2Garment,
      job2Copies,
      sheetId: marker1.id,
      sheetWidthMm: SHEET_WIDTH_MM,
      sheetLengthMm,
      material,
      stepMm,
    },
    stock: {
      id: marker1.id,
      outlineMm: marker1.outline_mm,
      grossAreaM2: marker1.gross_area_m2,
      sourceUrl,
      materialName: materialRecord.name,
    },
    job1: {
      garment: job1Garment,
      requestSha256: plan1.request_sha256,
      externalCutAreaM2: marker1.external_cut_area_m2,
      utilization: marker1.utilization,
      remainingAreaM2: marker1.gross_area_m2 - marker1.external_cut_area_m2,
      workUsed: plan1.work_used,
      candidates: plan1.candidates,
      placements: marker1.placements.map(summarizePlacement),
      occupiedBasis: residualStock1.occupied_basis,
      occupiedCount: residualStock1.occupied_mm.length,
      svg: job1.preview_svg,
      residualJson: job1.residual_json,
    },
    freshRollJob2: {
      widthMm: roll2.width_m * 1000,
      lengthMm: roll2.length_m * 1000,
      stockAreaM2: roll2.stock_area_m2,
      externalCutAreaM2: roll2.external_cut_area_m2,
      utilization: roll2.utilization,
    },
    job2: job2Result,
    job2PatternJson: pat2,
    job2RequestJson: req2Json,
    job2Refusal,
  };
}

if (typeof DedicatedWorkerGlobalScope !== "undefined" && self instanceof DedicatedWorkerGlobalScope) {
  let latest;
  self.onmessage = async ({ data }) => {
    latest = data.id;
    const ftx = await getStudioEngine().catch(error => {
      if (latest === data.id) {
        self.postMessage({ id: data.id, loadError: error instanceof Error ? error.message : String(error) });
      }
      return null;
    });
    if (!ftx || latest !== data.id) return;
    try {
      const result = planRemnant(ftx, data.params);
      if (latest === data.id) self.postMessage({ id: data.id, result });
    } catch (error) {
      if (latest === data.id) {
        self.postMessage({ id: data.id, error: error instanceof Error ? error.message : String(error) });
      }
    }
  };
}
