// Apply a source-qualified laundering transition (`ftx-pattern-treatment/1`) in local Rust WASM.
// Supports `treat` (post-wash shrunk garment) and `compensate` (oversized pre-wash recut)
// strictly for measured material revisions matching the transition's BEFORE snapshot.
import { getStudioEngine } from "./studio-engine.mjs";

const TODAY = "2026-10-12";
const CREATED = "2026-10-12T00:00:00Z";

export const WASH_PRESETS = {
  denim1_treat: {
    id: "denim1_treat",
    label: "Denim 1 (1.5% elastane) · Post-wash",
    garment: "zouave_bolero",
    transitionKey: "denim1",
    mode: "treat",
    mismatchMaterial: false,
  },
  denim1_compensate: {
    id: "denim1_compensate",
    label: "Denim 1 (1.5% elastane) · Pre-wash recut",
    garment: "zouave_bolero",
    transitionKey: "denim1",
    mode: "compensate",
    mismatchMaterial: false,
  },
  denim5_treat: {
    id: "denim5_treat",
    label: "Denim 5 (0.5% elastane) · Post-wash",
    garment: "zouave_bolero",
    transitionKey: "denim5",
    mode: "treat",
    mismatchMaterial: false,
  },
  muslin_refusal: {
    id: "muslin_refusal",
    label: "Cotton muslin + Denim 1 wash (refusal)",
    garment: "zouave_bolero",
    transitionKey: "denim1",
    mode: "treat",
    mismatchMaterial: true,
  },
};

export const initialWash = {
  garment: "zouave_bolero",
  transitionKey: "denim1",
  mode: "treat",
  mismatchMaterial: false,
};

let treatmentsPromise = null;
const draftCache = new Map();

async function loadTreatments() {
  if (!treatmentsPromise) {
    treatmentsPromise = fetch("/labs/wash-treatments.json").then(async r => {
      if (!r.ok) throw new Error(`Could not load wash treatments (${r.status}).`);
      return r.json();
    });
  }
  return treatmentsPromise;
}

function readRound(round) {
  try {
    const page = JSON.parse(round.page());
    if (!page.preview_svg) {
      throw new Error("Pattern export produced no preview SVG.");
    }
    const files = {};
    for (const [type, suffix] of [["pdf", "_A4.pdf"], ["svg", "_master_mm.svg"]]) {
      const f = page.files.find(entry => entry.name.endsWith(suffix));
      if (f) files[type] = { bytes: round.file(f.name), name: f.name };
    }
    const pattern = new TextDecoder("utf-8", { fatal: true }).decode(round.file("pattern.json"));
    const fitSpecBytes = round.file("fit_spec.json");
    const fitSpec = fitSpecBytes ? new TextDecoder("utf-8", { fatal: true }).decode(fitSpecBytes) : null;
    return {
      status: page.basis.flat,
      checks: page.exports.checks,
      previewSvg: page.preview_svg,
      pattern,
      fitSpec,
      files,
    };
  } finally {
    round.free();
  }
}

function draftGarment(ftx, garment) {
  let cached = draftCache.get(garment);
  if (!cached) {
    const round = readRound(
      ftx.run(JSON.stringify({ program: garment, body: { mode: "chart", stature_cm: 92 }, style: {} }), TODAY, CREATED),
    );
    cached = {
      pattern: round.pattern,
      fitSpec: round.fitSpec,
      previewSvg: round.previewSvg,
    };
    draftCache.set(garment, cached);
  }
  return cached;
}

export function runWashShrink(ftx, treatments, params = {}) {
  const cfg = { ...initialWash, ...params };
  const transition = treatments[cfg.transitionKey];
  if (!transition) throw new Error(`Unknown wash transition: ${cfg.transitionKey}`);

  const base = draftGarment(ftx, cfg.garment);
  const fitSpecObj = base.fitSpec ? JSON.parse(base.fitSpec) : undefined;
  const materialBinding = cfg.mismatchMaterial
    ? { id: "dharma/cotton-muslin-3.5oz" }
    : transition.before;

  const bound = JSON.parse(
    ftx.material_bind(
      base.pattern,
      JSON.stringify({ schema: "ftx-material-bindings/1", default: materialBinding, pieces: {} }),
    ),
  );
  if (!bound?.pattern_json) throw new Error("Material binding returned no pattern.");

  const boundDoc = JSON.parse(bound.pattern_json);
  const piecesMap = Object.fromEntries(boundDoc.pieces.map(p => [p.id, "wash5"]));
  const reqObj = {
    schema: "ftx-pattern-treatment/1",
    mode: cfg.mode,
    transitions: { wash5: transition },
    pieces: piecesMap,
  };

  const rawReport = ftx.material_treatment_pattern(bound.pattern_json, JSON.stringify(reqObj));
  const report = JSON.parse(rawReport);

  // Render the revised pattern through the exact same native exporter (`ftx.run`) for SVG/PDF parity.
  const revisedRound = readRound(
    ftx.run(
      JSON.stringify({
        body: { mode: "chart", stature_cm: 92 },
        pattern: JSON.parse(report.pattern_json),
        ...(fitSpecObj ? { fit_spec: fitSpecObj } : {}),
      }),
      TODAY,
      CREATED,
    ),
  );

  return {
    config: cfg,
    transition: {
      key: cfg.transitionKey,
      specimen: transition.specimen,
      beforeId: transition.before.id,
      beforeName: transition.before.record.name,
      afterId: transition.after.id,
      afterName: transition.after.record.name,
      elastanePercent: transition.before.record.structure?.source_initial_elastane_ratio_percent ?? null,
      warpShrinkPct: transition.shrinkage_warp.value,
      weftShrinkPct: transition.shrinkage_weft.value,
      cycles: transition.cycles,
      process: transition.process,
      conditioningBefore: transition.conditioning_before,
      conditioningAfter: transition.conditioning_after,
      method: transition.shrinkage_warp.method,
      sourceRef: transition.sources?.paper?.reference ?? null,
      sourceNote: transition.sources?.paper?.note ?? null,
    },
    mode: report.mode,
    scope: report.scope,
    sourcePatternSha256: report.source_pattern_sha256,
    patternSha256: report.pattern_sha256,
    requestSha256: report.request_sha256,
    pieces: report.pieces,
    seamsBefore: report.seams_before,
    seamsAfter: report.seams_after,
    originalSvg: base.previewSvg,
    revisedSvg: revisedRound.previewSvg,
    revisedStatus: revisedRound.status,
    revisedChecks: revisedRound.checks,
    frozenRequestJson: report.frozen_request_json,
    files: revisedRound.files,
  };
}

if (typeof DedicatedWorkerGlobalScope !== "undefined" && self instanceof DedicatedWorkerGlobalScope) {
  let latest;
  self.onmessage = async ({ data }) => {
    latest = data.id;
    let ftx;
    let treatments;
    try {
      [ftx, treatments] = await Promise.all([getStudioEngine(), loadTreatments()]);
    } catch (error) {
      if (latest === data.id) {
        self.postMessage({ id: data.id, loadError: error instanceof Error ? error.message : String(error) });
      }
      return;
    }
    if (latest !== data.id) return;
    try {
      const result = runWashShrink(ftx, treatments, data.params);
      if (latest !== data.id) return;
      const transfer = [];
      for (const file of Object.values(result.files || {})) {
        if (file?.bytes instanceof Uint8Array) transfer.push(file.bytes.buffer);
      }
      self.postMessage({ id: data.id, result }, transfer);
    } catch (error) {
      if (latest === data.id) {
        self.postMessage({ id: data.id, error: error instanceof Error ? error.message : String(error) });
      }
    }
  };
}
