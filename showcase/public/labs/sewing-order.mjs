// Native-parity sewing-order planner for the 92 cm Zouave bolero jacket (ftx-jkf).
// Binds the drafted bolero to NISTIR 7282 Cotton Duck (published 1.32 mm micrometer thickness)
// and runs the exact Rust/WASM `sew_plan` and `sew_stitches` planners.

export const TODAY = "2026-10-12";
export const CREATED = "2026-10-12T00:00:00Z";
export const SEWING_MATERIAL_ID = "nist/7282/cotton-duck";

export const BOLERO_SEW_OPTIONS = {
  machine: {
    id: "synthetic-lockstitch-301-not-physically-qualified",
    stitch_mm: [1.5, 4.5],
    feed_b_over_a: [0.7, 1.4],
    max_stack_mm: 5.0,
    max_stitches_per_minute: 1200.0,
    reverse: {
      stitch_mm: [1.5, 4.5],
      feed_b_over_a: [0.7, 1.4],
      max_stitches_per_minute: 800.0,
    },
  },
  seam_tolerance_mm: 1.5,
  passes: [
    {
      id: "shoulder-l",
      seam: "shoulder_L",
      max_stitch_mm: 2.5,
      plies: [1, 1],
      minimum_allowance_mm: 8.0,
      stitches_per_minute: 700.0,
      needle: "80/12-universal",
      thread: "tex-40-poly",
      backtack: { start_stitches: 3, end_stitches: 3 },
    },
    {
      id: "shoulder-r",
      seam: "shoulder_R",
      max_stitch_mm: 2.5,
      plies: [1, 1],
      minimum_allowance_mm: 8.0,
      stitches_per_minute: 700.0,
      needle: "80/12-universal",
      thread: "tex-40-poly",
      backtack: { start_stitches: 3, end_stitches: 3 },
    },
    {
      id: "side-l",
      seam: "side_L",
      max_stitch_mm: 2.5,
      plies: [1, 1],
      minimum_allowance_mm: 8.0,
      stitches_per_minute: 700.0,
      needle: "80/12-universal",
      thread: "tex-40-poly",
      backtack: { start_stitches: 3, end_stitches: 3 },
      after: ["shoulder-l"],
    },
    {
      id: "side-r",
      seam: "side_R",
      max_stitch_mm: 2.5,
      plies: [1, 1],
      minimum_allowance_mm: 8.0,
      stitches_per_minute: 700.0,
      needle: "80/12-universal",
      thread: "tex-40-poly",
      backtack: { start_stitches: 3, end_stitches: 3 },
      after: ["shoulder-r"],
    },
    {
      id: "underarm-l",
      seam: "underarm_L",
      max_stitch_mm: 2.5,
      plies: [1, 1],
      minimum_allowance_mm: 8.0,
      stitches_per_minute: 700.0,
      needle: "80/12-universal",
      thread: "tex-40-poly",
      backtack: { start_stitches: 3, end_stitches: 3 },
    },
    {
      id: "underarm-r",
      seam: "underarm_R",
      max_stitch_mm: 2.5,
      plies: [1, 1],
      minimum_allowance_mm: 8.0,
      stitches_per_minute: 700.0,
      needle: "80/12-universal",
      thread: "tex-40-poly",
      backtack: { start_stitches: 3, end_stitches: 3 },
    },
    {
      id: "armhole-front-l",
      seam: "armhole_f_L",
      max_stitch_mm: 2.2,
      plies: [1, 1],
      minimum_allowance_mm: 8.0,
      stitches_per_minute: 500.0,
      needle: "80/12-universal",
      thread: "tex-40-poly",
      backtack: { start_stitches: 3, end_stitches: 3 },
      after: ["shoulder-l", "side-l", "underarm-l"],
    },
    {
      id: "armhole-back-l",
      seam: "armhole_b_L",
      max_stitch_mm: 2.2,
      plies: [1, 1],
      minimum_allowance_mm: 8.0,
      stitches_per_minute: 500.0,
      needle: "80/12-universal",
      thread: "tex-40-poly",
      backtack: { start_stitches: 3, end_stitches: 3 },
      after: ["armhole-front-l"],
    },
    {
      id: "armhole-front-r",
      seam: "armhole_f_R",
      max_stitch_mm: 2.2,
      plies: [1, 1],
      minimum_allowance_mm: 8.0,
      stitches_per_minute: 500.0,
      needle: "80/12-universal",
      thread: "tex-40-poly",
      backtack: { start_stitches: 3, end_stitches: 3 },
      after: ["shoulder-r", "side-r", "underarm-r"],
    },
    {
      id: "armhole-back-r",
      seam: "armhole_b_R",
      max_stitch_mm: 2.2,
      plies: [1, 1],
      minimum_allowance_mm: 8.0,
      stitches_per_minute: 500.0,
      needle: "80/12-universal",
      thread: "tex-40-poly",
      backtack: { start_stitches: 3, end_stitches: 3 },
      after: ["armhole-front-r"],
    },
  ],
};

export const OPERATION_LABELS = {
  "shoulder-l": { title: "Left shoulder seam", stage: "Torso assembly" },
  "shoulder-r": { title: "Right shoulder seam", stage: "Torso assembly" },
  "side-l": { title: "Left side seam", stage: "Torso assembly" },
  "side-r": { title: "Right side seam", stage: "Torso assembly" },
  "underarm-l": { title: "Left sleeve underarm", stage: "Sleeve tube" },
  "underarm-r": { title: "Right sleeve underarm", stage: "Sleeve tube" },
  "armhole-front-l": { title: "Set left sleeve · front armscye", stage: "Sleeve setting" },
  "armhole-back-l": { title: "Set left sleeve · back armscye", stage: "Sleeve setting" },
  "armhole-front-r": { title: "Set right sleeve · front armscye", stage: "Sleeve setting" },
  "armhole-back-r": { title: "Set right sleeve · back armscye", stage: "Sleeve setting" },
};

function parseCutExteriorFromSewSvg(svgText) {
  // Extract exact native Rust-evaluated `<path class="cut-exterior" d="..."/>` for side-0 and side-1
  // directly from `ftx.sew_svg` — zero JS curve reimplementation, exact same mm frame as `stations`.
  const matches = [...svgText.matchAll(/<path class="cut-exterior" d="([^"]+)"/g)];
  return matches.map(m => {
    const nums = m[1].match(/-?\d+(?:\.\d+)?/g) || [];
    const pts = [];
    for (let i = 0; i + 1 < nums.length; i += 2) {
      pts.push([Number(Number(nums[i]).toFixed(2)), Number(Number(nums[i + 1]).toFixed(2))]);
    }
    // Keep every 3rd vertex on dense curved runs so the starter JSON stays crisp and small while faithful to <0.1 mm.
    if (pts.length <= 160) return pts;
    const thinned = [];
    for (let i = 0; i < pts.length; i++) {
      if (i === 0 || i === pts.length - 1 || i % 3 === 0) thinned.push(pts[i]);
    }
    return thinned;
  });
}

function roundPt([x, y]) {
  return [Number(x.toFixed(2)), Number(y.toFixed(2))];
}

export function buildBoleroSewingBundle(ftx) {
  const round = ftx.run(
    JSON.stringify({ body: { mode: "chart", stature_cm: 92 }, program: "zouave_bolero", style: {} }),
    TODAY,
    CREATED,
  );
  let rawPatternJson;
  try {
    rawPatternJson = new TextDecoder("utf-8", { fatal: true }).decode(round.file("pattern.json"));
  } finally {
    round.free();
  }

  const bound = JSON.parse(
    ftx.material_bind(
      rawPatternJson,
      JSON.stringify({
        schema: "ftx-material-bindings/1",
        default: { id: SEWING_MATERIAL_ID },
        pieces: {},
      }),
    ),
  );
  const patternJson = bound.pattern_json;
  const optionsJson = JSON.stringify(BOLERO_SEW_OPTIONS);
  const planJson = ftx.sew_plan(patternJson, optionsJson);
  const programJson = ftx.sew_stitches(patternJson, optionsJson);
  const plan = JSON.parse(planJson);
  const program = JSON.parse(programJson);

  // Extract all 5 exact cut-exterior outlines directly from native `sew_svg` outputs.
  const [backPoly, frontLPoly] = parseCutExteriorFromSewSvg(ftx.sew_svg(patternJson, optionsJson, "shoulder-l"));
  const [, frontRPoly] = parseCutExteriorFromSewSvg(ftx.sew_svg(patternJson, optionsJson, "shoulder-r"));
  const [sleeveLPoly] = parseCutExteriorFromSewSvg(ftx.sew_svg(patternJson, optionsJson, "underarm-l"));
  const [sleeveRPoly] = parseCutExteriorFromSewSvg(ftx.sew_svg(patternJson, optionsJson, "underarm-r"));
  const instances = {
    back: backPoly,
    "front.R": frontRPoly,
    "front.L": frontLPoly,
    "sleeve.R": sleeveRPoly,
    "sleeve.L": sleeveLPoly,
  };

  // Compute reverse dependency map (`unlocks`: which later operations depend on this operation).
  const unlocksMap = new Map(plan.seams.map(s => [s.pass.id, []]));
  for (const s of plan.seams) {
    for (const dep of s.pass.after || []) {
      if (unlocksMap.has(dep)) unlocksMap.get(dep).push(s.pass.id);
    }
  }

  const operations = plan.seams.map((s, index) => {
    const id = s.pass.id;
    const meta = OPERATION_LABELS[id] || { title: id, stage: "Assembly" };
    const panelA = s.sides[0].reference.split("/")[0];
    const panelB = s.sides[1].reference.split("/")[0];
    const ratioMin = Math.min(...s.feed_b_over_a);
    const ratioMax = Math.max(...s.feed_b_over_a);
    return {
      index,
      id,
      seam: s.pass.seam,
      title: meta.title,
      stage: meta.stage,
      after: s.pass.after || [],
      unlocks: unlocksMap.get(id) || [],
      plies: s.pass.plies,
      maxStitchMm: s.pass.max_stitch_mm,
      spm: s.pass.stitches_per_minute,
      needle: s.pass.needle,
      thread: s.pass.thread,
      stackMm: Number(s.stack_mm.toFixed(3)),
      stitchIntervals: s.stitch_intervals,
      feedSeconds: Number(s.feed_seconds.toFixed(3)),
      feedRatioRange: [Number(ratioMin.toFixed(4)), Number(ratioMax.toFixed(4))],
      notchMatch: s.stations.notch_match,
      sideA: {
        panel: panelA,
        reference: s.sides[0].reference,
        lengthMm: Number(s.stations.len_a.toFixed(2)),
        thicknessMm: s.sides[0].thickness_mm,
        allowanceMm: s.sides[0].allowance_mm,
        startMm: roundPt(s.stations.a[0]),
        endMm: roundPt(s.stations.a[s.stations.a.length - 1]),
        stations: s.stations.a.map(roundPt),
      },
      sideB: {
        panel: panelB,
        reference: s.sides[1].reference,
        lengthMm: Number(s.stations.len_b.toFixed(2)),
        thicknessMm: s.sides[1].thickness_mm,
        allowanceMm: s.sides[1].allowance_mm,
        startMm: roundPt(s.stations.b[0]),
        endMm: roundPt(s.stations.b[s.stations.b.length - 1]),
        stations: s.stations.b.map(roundPt),
      },
      runs: s.runs.map(r => ({
        phase: r.phase,
        direction: r.direction,
        fromStation: r.from_station,
        toStation: r.to_station,
        stitchIntervals: r.stitch_intervals,
        feedSeconds: Number(r.feed_seconds.toFixed(3)),
      })),
    };
  });

  const material = Object.values(plan.materials)[0]?.authored;
  const sourceRef = material?.sources?.nist7282?.reference ?? "https://nvlpubs.nist.gov/nistpubs/Legacy/IR/nistir7282.pdf";

  return {
    starter: {
      schema: plan.schema,
      garment: "zouave_bolero",
      sizeLabel: "92 cm chart wearer",
      patternSha256: plan.pattern_sha256,
      material: {
        id: material?.id ?? SEWING_MATERIAL_ID,
        name: material?.name ?? "NISTIR 7282 Cotton Duck",
        thicknessMm: material?.properties?.thickness?.value ?? 1.32,
        method: material?.properties?.thickness?.method ?? "Table 2 mean micrometer thickness",
        sourceUrl: sourceRef,
      },
      machineId: plan.options.machine.id,
      totalOperations: operations.length,
      totalStitchIntervals: plan.stitch_intervals,
      totalFeedSeconds: Number(plan.feed_seconds.toFixed(3)),
      totalDirectedStitches: program.stitches.length,
      hardwareExecutionAuthorized: plan.hardware_execution_authorized,
      physicalSeamQualityValidated: plan.physical_seam_quality_validated,
      instances,
      operations,
    },
    patternJson,
    optionsJson,
    planJson,
    programJson,
  };
}
