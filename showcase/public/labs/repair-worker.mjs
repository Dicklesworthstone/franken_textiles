// Walk a paired side seam, introduce a reversible synthetic mismatch, and repair it with true-seam
// using the exact native Rust/WASM pattern, alteration and export round.
import { getStudioEngine } from "./studio-engine.mjs";

const TODAY = "2026-10-12";
const CREATED = "2026-10-12T00:00:00Z";

function readRound(round) {
  try {
    const page = JSON.parse(round.page());
    if (!page.preview_svg || page.errors?.length) {
      throw new Error(
        page.errors?.map(e => (typeof e === "string" ? e : e.message)).join(" · ") ||
          "The engine could not evaluate the pattern.",
      );
    }
    const files = {};
    for (const [type, suffix] of [["pdf", "_A4.pdf"], ["svg", "_master_mm.svg"]]) {
      const source = page.files.find(file => file.name.endsWith(suffix));
      if (!source) continue;
      files[type] = { bytes: round.file(source.name), name: source.name };
    }
    const pattern = new TextDecoder("utf-8", { fatal: true }).decode(round.file("pattern.json"));
    const fitSpecBytes = round.file("fit_spec.json");
    const fitSpec = fitSpecBytes ? new TextDecoder("utf-8", { fatal: true }).decode(fitSpecBytes) : null;
    const seams = page.validation.seams;
    const sideR = seams.find(s => s.seam === "side_R");
    const sideL = seams.find(s => s.seam === "side_L");
    return {
      status: page.basis.flat,
      passedSeams: seams.filter(s => s.status === "PASS").length,
      totalSeams: seams.length,
      checks: page.exports.checks,
      sideR,
      sideL,
      seams,
      previewSvg: page.preview_svg,
      pattern,
      fitSpec,
      files,
    };
  } finally {
    round.free();
  }
}

export function runSeamRepair(ftx, { mismatchMm }) {
  const delta = Number(mismatchMm);
  if (!Number.isFinite(delta) || delta === 0 || Math.abs(delta) > 40) {
    throw new Error("Mismatch offset must be a nonzero value between -40 and +40 mm.");
  }

  // 1. Original drafted bolero on the 92 cm chart wearer.
  const original = readRound(
    ftx.run(JSON.stringify({ program: "zouave_bolero", body: { mode: "chart", stature_cm: 92 }, style: {} }), TODAY, CREATED),
  );
  const fitSpecObj = JSON.parse(original.fitSpec);

  // 2. Reversible synthetic alteration: move back:side_hem along y (pattern y-up: dy = -delta lengthens back/side).
  const breakOp = { op: "move", piece: "back", vertex: "side_hem", dx: 0, dy: -delta };
  const brokenAlter = JSON.parse(ftx.alter_pattern(original.pattern, JSON.stringify(breakOp), null));
  const broken = readRound(
    ftx.run(
      JSON.stringify({
        body: { mode: "chart", stature_cm: 92 },
        pattern: JSON.parse(brokenAlter.pattern_json),
        fit_spec: fitSpecObj,
      }),
      TODAY,
      CREATED,
    ),
  );

  // 3. Repair the mismatched seam using native `true-seam` on `side_R` at its hem (`start`) end.
  const repairOp = { op: "true-seam", seam: "side_R", end: "start" };
  const repairedAlter = JSON.parse(ftx.alter_pattern(brokenAlter.pattern_json, JSON.stringify(repairOp), null));
  const repaired = readRound(
    ftx.run(
      JSON.stringify({
        body: { mode: "chart", stature_cm: 92 },
        pattern: JSON.parse(repairedAlter.pattern_json),
        fit_spec: fitSpecObj,
      }),
      TODAY,
      CREATED,
    ),
  );

  return {
    mismatchMm: delta,
    breakWhy: brokenAlter.why,
    repairWhy: repairedAlter.why,
    stages: {
      original: {
        status: original.status,
        passedSeams: original.passedSeams,
        totalSeams: original.totalSeams,
        checks: original.checks,
        sideR: original.sideR,
        sideL: original.sideL,
        previewSvg: original.previewSvg,
        files: original.files,
      },
      broken: {
        status: broken.status,
        passedSeams: broken.passedSeams,
        totalSeams: broken.totalSeams,
        checks: broken.checks,
        sideR: broken.sideR,
        sideL: broken.sideL,
        previewSvg: broken.previewSvg,
        files: broken.files,
      },
      repaired: {
        status: repaired.status,
        passedSeams: repaired.passedSeams,
        totalSeams: repaired.totalSeams,
        checks: repaired.checks,
        sideR: repaired.sideR,
        sideL: repaired.sideL,
        previewSvg: repaired.previewSvg,
        files: repaired.files,
      },
    },
  };
}

if (typeof DedicatedWorkerGlobalScope !== "undefined" && self instanceof DedicatedWorkerGlobalScope) {
  let latest;
  self.onmessage = async ({ data }) => {
    latest = data.id;
    try {
      const ftx = await getStudioEngine();
      if (latest !== data.id) return;
      const result = runSeamRepair(ftx, { mismatchMm: data.mismatchMm });
      if (latest !== data.id) return;
      const transfers = [];
      for (const stage of Object.values(result.stages)) {
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
