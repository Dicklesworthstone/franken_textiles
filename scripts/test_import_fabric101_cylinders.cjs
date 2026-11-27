"use strict";

const { test } = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { acquire, calibration, listRecordings, readSamples, readTares, replay, twistIndices } = require("./import_fabric101_cylinders.cjs");

function template(warpAxis = [1, 0]) {
  return { schema: "ftx-cylinder-experiment/1", material: { id: "candidate" },
    fixture: { holder_radius_m: 0.04, nominal_free_height_m: 0.05, specimen_width_m: 0.26,
      specimen_height_m: 0.11, warp_axis: warpAxis, cells: [6, 2] },
    controls: { max_step_s: 0.5, max_steps: 1024 } };
}

test("preserves sensor units, signed reactions and repeated instrument ticks", () => {
  const header = "Time LoadPosition Load TorquePosition Torque\n(s) (mm) (N) (Degree) (N-m)\n";
  const line = "time: 0.001 loadPos: 1.1 loadForce: -2 torquePos: 3 torqueForce: 0.4\n";
  assert.deepEqual(readSamples(header + line + line), Array(2).fill([0.001, 1.1, -2, 3, 0.4]));
  assert.throws(() => readSamples(header.replace("(N-m)", "(N-mm)") + line + line), /units/);
  assert.throws(() => readSamples(header + line + line.replace("loadForce:", "unlabelled:")), /sample/);
  assert.throws(() => readSamples(header + line + line.replace("0.001", "0.000")), /clock/);
});

test("rejects modified acquisition bytes before returning measurements", t => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "ftx-cylinder-"));
  t.after(() => fs.rmSync(root, { recursive: true }));
  const folder = path.join(root, "data/L01/001");
  fs.mkdirSync(folder, { recursive: true });
  fs.writeFileSync(path.join(folder, "head.dat"), "some other specimen");
  assert.throws(() => acquire(root, "warp"), /pinned Git blob/);
  const target = path.join(root, "real-head.dat");
  fs.writeFileSync(target, "some other specimen");
  fs.unlinkSync(path.join(folder, "head.dat"));
  fs.symlinkSync(target, path.join(folder, "head.dat"));
  assert.throws(() => acquire(root, "warp"), /regular file/);
  // Also reject symlinked intermediate directories inside the release tree.
  const outsideDir = path.join(root, "outside-run");
  fs.mkdirSync(outsideDir);
  fs.writeFileSync(path.join(outsideDir, "head.dat"), "some other specimen");
  fs.rmSync(folder, { recursive: true });
  fs.symlinkSync(outsideDir, folder);
  assert.throws(() => acquire(root, "warp"), /traverse symlinks/);
});

test("native replay retains holds, reversals, mounting displacement and the template material", () => {
  const points = [[0.001, 0], [0.2, 0], [0.3, 1], [0.4, 1.2], [0.6, 4], [0.9, 4], [1.1, 0]];
  const rows = points.map(([time, angle]) => [time, 0, 2, angle, 0.3]);
  const source = { direction: "weft", phases: { twist: rows }, protocol: { measured_pretension_travel_mm: 1.1 } };
  const input = template();
  const { request, receipt } = replay(source, input, 0.1, "named-direction-axial");
  assert.ok([0, 1, 4, 5, 6].every(i => receipt.sample_indices.includes(i)));
  assert.equal(request.loading[0].time_s, 0);
  assert.equal(request.loading.at(-1).time_s, 1.1 - 0.001);
  assert.equal(request.fixture.pretension_displacement_m, 1.1 * 0.001);
  assert.equal(input.fixture.pretension_displacement_m, undefined);
  assert.deepEqual(request.material, input.material);
  let peak = 0;
  for (let j = 1; j < receipt.sample_indices.length; j++) {
    const left = receipt.sample_indices[j - 1], right = receipt.sample_indices[j];
    const [t0, x0] = points[left], [t1, x1] = points[right];
    for (let i = left + 1; i < right; i++) {
      const [time, angle] = points[i];
      peak = Math.max(peak, Math.abs(angle - (x0 + (x1 - x0) * (time - t0) / (t1 - t0))));
    }
  }
  assert.ok(peak <= 0.1 && Math.abs(peak - receipt.max_rotation_error_deg) < 1e-12);
  receipt.sample_indices.forEach((i, j) => assert.equal(request.loading[j].rotation_deg, points[i][1]));
  assert.throws(() => replay(source, { ...input, controls: { max_steps: 1 } }, 0.1, "named-direction-axial"), /budget/);
  assert.throws(() => replay(source, input, 0.1, undefined), /convention/);
  assert.throws(() => replay(source, input, 0.1, "named-direction-circumferential"), /warp_axis/);
  assert.throws(() => replay(source, { ...input, fixture: { ...input.fixture, rotating_holder: "upper" } },
    0.1, "named-direction-axial"), /lower-driven/);
  assert.equal(replay(source, { ...input, fixture: { ...input.fixture, rotating_holder: "lower" } },
    0.1, "named-direction-axial").request.fixture.rotating_holder, "lower");
  assert.deepEqual(receipt.warp_axis, [1, 0]);
  assert.equal(receipt.axial_grip_slip_assumption_m, 0);
  assert.equal(request.fixture.rotating_holder, "lower");
  assert.equal(receipt.model_rotating_holder, receipt.source_rotating_holder);
  assert.equal(receipt.rotation_scope, "physical_driven_holder");
  assert.equal(receipt.rotation_sign, 1);
});

test("refuses to silently omit axial movement, non-finite samples or excessive rotation reversals", () => {
  assert.throws(() => twistIndices([], 2), /multiple instrument samples/);
  assert.throws(() => twistIndices([[0, 0, 0, 0, 0]], 2), /multiple instrument samples/);
  assert.throws(() => twistIndices([[0, 0, 0, 0, 0], [1, 0, 0, Number.NaN, 0]], 2), /finite 5-column/);
  assert.throws(() => twistIndices([[0, 0, 0, 0, 0], [Number.NaN, 0, 0, 1, 0]], 2), /finite 5-column/);
  assert.throws(() => twistIndices([[0, 0, 0, 0, 0], [1, 0, 0, Number.POSITIVE_INFINITY, 0]], 2), /finite 5-column/);
  assert.throws(() => twistIndices([[0, 0, 0, 0], [1, 0, 0, 1]], 2), /finite 5-column/);
  const rows = Array.from({ length: 130 }, (_, i) => [i, 0, 0, i % 2, 0]);
  assert.throws(() => twistIndices(rows, 2), /129/);
  rows[4][1] = 1;
  assert.throws(() => twistIndices(rows, 2), /fixed axial/);
});

test("readTares rejects blank values, blank names, duplicates, prototype keys and extra columns", () => {
  const valid = "**After Pretension**\n pret_moved , -1.25 , mm \ndimensionless,4,\n";
  const parsed = readTares(valid);
  assert.deepEqual(parsed["After Pretension"].pret_moved, { value: -1.25, unit: "mm" });
  assert.deepEqual(parsed["After Pretension"].dimensionless, { value: 4, unit: null });
  for (const bad of [
    "**After Pretension**\npret_moved,   ,mm\n",
    "**After Pretension**\n ,-1.25,mm\n",
    "**After Pretension**\npret_moved,-1.25,mm,extra\n",
    "**After Pretension**\npret_moved,-1.25,mm\npret_moved,-1.0,mm\n",
    "**After Pretension**\npret_moved,-1.25,mm\n**After Pretension**\nother,1,mm\n",
    "**__proto__**\npolluted,1,mm\n**After Pretension**\npret_moved,-1.25,mm\n",
    "**After Pretension**\nconstructor,1,mm\npret_moved,-1.25,mm\n",
  ]) {
    assert.throws(() => readTares(bad), /Invalid tare measurement/);
  }
  assert.throws(() => readTares("**After Pretension**\npret_moved,-1.25,   \n"), /mounting travel/);
});

test("replays complete pinned linen recordings in both directions", { skip: !process.env.FABRIC101_RELEASE }, () => {
  for (const [direction, count, start, waypoints, travel] of [
    ["warp", 787, 0, 60, 1.14125], ["weft", 786, 0.001, 62, 2.005],
  ]) {
    const a = acquire(process.env.FABRIC101_RELEASE, direction);
    assert.equal(Object.keys(a.sources).length, 5);
    assert.equal(a.phases.twist.length, count);
    assert.ok(a.phases.compression.length > 2000 && a.phases.friction.length > 30_000);
    assert.ok(Math.abs(a.protocol.measured_pretension_travel_mm - travel) < 1e-12);
    const { request, receipt } = replay(a, template(direction === "warp" ? [0, 1] : [1, 0]), 0.01, "named-direction-axial");
    assert.equal(request.loading.length, waypoints);
    assert.equal(receipt.stage_clock_origin_s, start);
    assert.ok(receipt.max_rotation_error_deg <= 0.01);
    assert.equal(request.loading.at(-1).time_s, a.phases.twist.at(-1)[0] - start);
  }
});

test("an unknown specimen cannot select local paths or inherit another specimen's pins", () => {
  for (const name of ["Drape 4", "../L01", "constructor"]) {
    assert.throws(() => acquire(".", "warp", name), /Unknown Fabric-101 specimen/);
  }
});

test("imports and replays distinct linen, jersey and rib recordings", { skip: !process.env.FABRIC101_COHORT_RELEASE }, () => {
  for (const [id, instrument, expected] of [
    ["L02", "124_L02_001", [[789, 66], [786, 62]]],
    ["KN01", "149_KN01_001", [[792, 59], [772, 60]]],
    ["KN02", "150_KN02_001", [[776, 67], [777, 68]]],
  ]) {
    for (const [index, direction] of ["warp", "weft"].entries()) {
      const acquisition = acquire(process.env.FABRIC101_COHORT_RELEASE, direction, id);
      assert.equal(acquisition.schema, "ftx-fabric101-acquisition/1");
      assert.equal(acquisition.instrument_specimen, instrument);
      assert.equal(acquisition.material_id, "fabric101-2026/" + id.toLowerCase());
      assert.equal(Object.keys(acquisition.sources).length, 5);
      assert.ok(Object.keys(acquisition.sources).every(p => p.startsWith("data/" + id + "/001/")));
      const { request, receipt } = replay(acquisition,
        template(direction === "warp" ? [0, 1] : [1, 0]), 0.01, "named-direction-axial");
      assert.deepEqual([acquisition.phases.twist.length, request.loading.length], expected[index]);
      assert.equal(request.fixture.rotating_holder, "lower");
      assert.ok(receipt.max_rotation_error_deg <= 0.01);
    }
  }
});


function inverseInputs() {
  // This synthetic baseline and these illustrative instrument conventions exercise transport only.
  const base = require("../examples/cylinder-maxwell-calibration.json");
  const material = structuredClone(base.material);
  delete material.membrane_relaxation;
  material.id = "synthetic/kn01-parameterization";
  material.sources.convention = { reference: "synthetic:reader-convention",
    accessed_on: "2026-11-25", note: "Assumed signs, residual scales, mounting and zero offsets for software testing" };
  const policy = { ...base.policy, max_forward_solves: 1 };
  delete policy.max_time_scaled;
  const request = { ...base, material, policy, schema: "ftx-cylinder-calibration/1", experiments: [],
    targets: [{ parameter: "shear_stiffness", bounds: [100, 400] }],
    controls: { ...base.controls, max_step_s: 0.5, max_steps: 1024 }, replace_existing: true };
  const recording = (direction, partition, axis) => ({ direction, partition, evidence: "estimate",
    fixture: { ...template(axis).fixture, tare_force_n: 0.15, tare_torque_nm: -0.003 },
    mounting: "named-direction-axial", mounting_source: "convention", angle_error_deg: 0.01,
    rows: [1, 150, 367], channels: {
      force_n: { sign: -1, offset: 0.2, scale: 0.5, source: "convention" },
      torque_nm: { sign: 1, offset: -0.001, scale: 0.02, source: "convention" },
    } });
  return [request, { schema: "ftx-fabric101-twist-map/1", specimen: "KN01", candidate_material_id: material.id,
    accessed_on: "2026-11-25", recordings: [recording("warp", "fit", [0, 1]), recording("weft", "check", [1, 0])] }];
}

test("refuses unsupported calibration templates and ambiguous acquisition identities", t => {
  const [template, map] = inverseInputs();
  const run = (t, m) => calibration(".", JSON.stringify(t), JSON.stringify(m));
  assert.throws(() => run(null, map), /memoryless/);
  assert.throws(() => run([], map), /memoryless/);
  assert.throws(() => run({ ...template, material: { ...template.material, sources: ["bad"] } }, map), /memoryless/);
  assert.throws(() => run({ ...template, material: { ...template.material, sources: "bad" } }, map), /memoryless/);
  assert.throws(() => run({ ...template, material: { ...template.material, structure: [] } }, map), /memoryless/);
  assert.throws(() => run({ ...template, controls: [] }, map), /memoryless/);
  assert.throws(() => run({ ...template, policy: [] }, map), /memoryless/);
  assert.throws(() => run({ ...template, schema: "ftx-cylinder-calibration/2" }, map), /memoryless/);
  assert.throws(() => run({ ...template, experiments: [{}] }, map), /empty experiments/);
  assert.throws(() => run({ ...template, policy: { ...template.policy, sensitivity: "adjoint" } }, map), /finite differences/);
  assert.throws(() => run(template, { ...map, candidate_material_id: "other" }), /exact candidate/);
  assert.throws(() => run({ ...template, material: { ...template.material, id: "fabric101-2026/l01" } },
    { ...map, candidate_material_id: "fabric101-2026/l01" }), /different catalog specimen/);
  const recording = map.recordings[0];
  const withRow = changes => ({ ...map, recordings: [{ ...recording, ...changes }] });
  assert.throws(() => run(template, withRow({ partition: null })), /assignment/);
  assert.throws(() => run(template, withRow({ rows: [2, 2] })), /distinct raw/);
  assert.throws(() => run(template, withRow({ rows: Array.from({length: 65}, (_, i) => i) })), /64 total/);
  assert.throws(() => run(template, withRow({ mounting_source: "unknown" })), /retained source/);
  assert.throws(() => run({ ...template, allow_estimated_observations: false }, map), /admit estimates/);

  // CLI --calibration rejects symlinked template/map inputs before parsing.
  const folder = fs.mkdtempSync(path.join(os.tmpdir(), "ftx-cal-link-"));
  t.after(() => fs.rmSync(folder, { recursive: true }));
  const realTpl = path.join(folder, "real-template.json");
  const realMap = path.join(folder, "real-map.json");
  const linkTpl = path.join(folder, "link-template.json");
  fs.writeFileSync(realTpl, JSON.stringify(template));
  fs.writeFileSync(realMap, JSON.stringify(map));
  fs.symlinkSync(realTpl, linkTpl);
  const res = require("node:child_process").spawnSync(
    process.execPath,
    [__filename.replace("test_import", "import"), "--calibration", ".", linkTpl, realMap, path.join(folder, "out.json")],
    { encoding: "utf8" },
  );
  assert.notEqual(res.status, 0);
  assert.match(res.stderr, /regular file/);
});

test("emits signed sampled FIT and CHECK experiments directly from two pinned knit recordings",
  { skip: !process.env.FABRIC101_COHORT_RELEASE }, t => {
  const [template, map] = inverseInputs(), root = process.env.FABRIC101_COHORT_RELEASE;
  const folder = fs.mkdtempSync(path.join(os.tmpdir(), "ftx-inverse-"));
  t.after(() => fs.rmSync(folder, { recursive: true }));
  const paths = ["template.json", "map.json", "new.json"].map(p => path.join(folder, p));
  fs.writeFileSync(paths[0], JSON.stringify(template)); fs.writeFileSync(paths[1], JSON.stringify(map));
  const args = [__filename.replace("test_import", "import"), "--calibration", root, ...paths];
  const run = () => require("node:child_process").spawnSync(process.execPath, args, {encoding: "utf8"});
  const first = run(); assert.equal(first.status, 0, first.stderr);
  const output = fs.readFileSync(paths[2]), request = JSON.parse(output), receipt = JSON.parse(first.stdout);
  const digest = bytes => require("node:crypto").createHash("sha256").update(bytes).digest("hex");
  assert.equal(receipt.request_sha256, digest(output));
  assert.equal(receipt.template_sha256, digest(fs.readFileSync(paths[0])));
  assert.equal(receipt.map_sha256, digest(fs.readFileSync(paths[1])));
  assert.deepEqual(request.targets, template.targets);
  assert.deepEqual(request.material.properties, template.material.properties);
  assert.deepEqual(request.policy, template.policy);
  assert.equal(request.material.id, template.material.id);
  assert.equal(request.experiments.length, 2);
  for (const [i, e] of request.experiments.entries()) {
    const decl = map.recordings[i], record = receipt.recordings[i], a = acquire(root, decl.direction, "KN01");
    assert.deepEqual(JSON.parse(request.material.sources[e.source].note), record);
    assert.deepEqual(record.sources, a.sources);
    assert.equal(e.fixture.rotating_holder, "lower"); assert.equal(record.material_id, a.material_id);
    assert.equal(e.partition, decl.partition); assert.equal(e.evidence, "estimate");
    assert.ok(record.max_rotation_error_deg <= 0.01);
    for (const [j, obs] of e.observations.entries()) {
      const raw = a.phases.twist[decl.rows[j]], w = e.loading[obs.waypoint_index];
      assert.deepEqual(record.measured_rows[j].raw, raw);
      assert.equal(record.sample_indices[obs.waypoint_index], decl.rows[j]);
      assert.equal(w.time_s, raw[0] - a.phases.twist[0][0]); assert.equal(w.rotation_deg, raw[3]);
      assert.deepEqual(obs.force_n, { value: -raw[2] + 0.2 - 0.15, scale: 0.5 });
      assert.deepEqual(obs.torque_nm, { value: raw[4] - 0.001 + 0.003, scale: 0.02 });
    }
  }
  assert.match(run().stderr, /EEXIST/);
  const change = f => { const m = structuredClone(map); f(m.recordings); return () =>
    calibration(root, JSON.stringify(template), JSON.stringify(m)); };
  assert.throws(change(r => { r[1].direction = "warp"; }), /distinct explicit/);
  assert.throws(change(r => { delete r[0].fixture.tare_force_n; }), /fixture tare/);
  assert.throws(change(r => { r[0].channels.force_n.sign = 2; }), /sign/);
  assert.throws(change(r => { r[0].channels.force_n.source = "missing"; }), /retained source/);
  assert.throws(change(r => { r[0].rows = [99999]; }), /outside/);
});


test("every listed run, header identity and catalog ID is tied to the native catalog row", () => {
  const rows = fs.readFileSync(path.join(__dirname, "../crates/ftx-pattern/src/material_catalog/fabric101.rs"), "utf8");
  const catalog = new Map([...rows.matchAll(/^\s*Row\((".*?")\s*,.*?,\s*"([^"]+\/head\.dat)",\s*"([a-f0-9]{40})",\s*"([^"]+)"/gm)]
    .map(([, name, header, sha, instrument]) => [JSON.parse(name), { header, sha, instrument }]));
  const pins = require("./fabric101-cylinders.json"), inventory = listRecordings();
  assert.deepEqual(inventory.map(r => r.specimen).sort(), [...catalog.keys()].sort());
  assert.equal(inventory.length, 96);
  let recordings = 0;
  for (const r of inventory) {
    const row = catalog.get(r.specimen), entry = pins[r.specimen];
    assert.equal(row.header, [r.specimen, r.run, "head.dat"].join("/"));
    assert.equal(row.sha, entry.pins["head.dat"]); assert.equal(row.instrument, r.instrument_specimen);
    assert.equal(r.material_id, "fabric101-2026/" + r.specimen.toLowerCase().replaceAll(" ", "-"));
    const paths = ["head.dat"];
    for (const dir of ["warp", "weft"]) for (const repeat of r[dir]) {
      recordings++;
      for (const phase of ["tarevalues.csv", "twist/twist.dat", "compression/compression.dat", "friction/friction.dat"]) {
        const name = [dir, repeat, phase].join("/");
        assert.match(entry.pins[name], /^[a-f0-9]{40}$/); paths.push(name);
      }
    }
    assert.deepEqual(paths.sort(), Object.keys(entry.pins).sort());
  }
  assert.equal(recordings, 192);
  assert.equal(inventory.flatMap(r => r.unresolved_twist_clocks).length, 47);
  const byId = Object.fromEntries(inventory.map(r => [r.specimen, r]));
  assert.deepEqual(byId["L03"].warp, ["s1", "s2"]);
  assert.deepEqual(byId["S10"].weft, ["s9"]);
  assert.deepEqual(byId["C06"].unresolved_twist_clocks, ["warp/s1", "weft/s1"]);
  assert.deepEqual(byId["C01"].unresolved_twist_clocks, []);
  for (const id of ["E-C02", "E-F02", "Paper 1"]) {
    assert.deepEqual(byId[id].weft, []);
    assert.throws(() => acquire(".", "weft", id), /No released weft/);
  }
  assert.throws(() => acquire(".", "warp", "C10"), /explicit repeat/);
  assert.throws(() => acquire(".", "warp", "L03", "s99"), /Unavailable repeat/);
});

test("keeps non-001 runs, names with spaces and repeated acquisitions in the native experiment",
  { skip: !process.env.FABRIC101_CATALOG_RELEASE }, () => {
  const root = process.env.FABRIC101_CATALOG_RELEASE;
  const [tpl, original] = inverseInputs();
  for (const [id, run] of [["C01", "002"], ["Drape 12", "002"], ["L03", "001"]]) {
    const map = structuredClone(original); map.specimen = id;
    if (id === "L03") {
      map.recordings[1].direction = "warp"; map.recordings[1].fixture.warp_axis = [0, 1];
      map.recordings[0].repeat = "s1"; map.recordings[1].repeat = "s2";
    }
    const { request, receipt } = calibration(root, JSON.stringify(tpl), JSON.stringify(map));
    for (const [j, e] of request.experiments.entries()) {
      const d = map.recordings[j], repeat = d.repeat || "s1", record = receipt.recordings[j];
      const name = "fabric101-2026/" + id.toLowerCase().replaceAll(" ", "-");
      const a = acquire(root, d.direction, name, repeat);
      assert.equal(a.run, run); assert.equal(a.repeat, repeat); assert.equal(a.material_id, name);
      assert.deepEqual(record.sources, a.sources);
      const expected = [id, run, d.direction, repeat, "twist"].join("-");
      assert.equal(e.id, expected); assert.equal(e.partition, d.partition);
      assert.ok(request.material.sources[e.source].reference.endsWith(
        ["data", encodeURIComponent(id), run, d.direction, repeat, "twist/twist.dat"].join("/")));
      assert.deepEqual(record.measured_rows.map(r => r.raw), d.rows.map(k => a.phases.twist[k]));
    }
    if (id === "L03") {
      map.recordings[1].repeat = "s1";
      assert.throws(() => calibration(root, JSON.stringify(tpl), JSON.stringify(map)), /distinct explicit/);
    }
  }
});

test("keeps unresolved source ticks and refuses timed forward and inverse authoring even with a large step",
  { skip: !process.env.FABRIC101_CATALOG_RELEASE }, () => {
  const root = process.env.FABRIC101_CATALOG_RELEASE, a = acquire(root, "warp", "C06");
  assert.equal(a.schema, "ftx-fabric101-acquisition/2");
  assert.equal(a.columns[0], "instrument_stage_clock");
  for (const stage of ["twist", "compression", "friction"]) {
    assert.deepEqual(a.stage_clocks[stage], { header_unit: "s", interpretation: "unresolved" });
  }
  assert.equal(a.phases.twist[0][0], 1); assert.equal(a.phases.twist.at(-1)[0], 61801);
  assert.equal(a.protocol.median_positive_twist_rate_deg_s, undefined);
  assert.equal(a.protocol.median_positive_compression_rate_mm_s, undefined);
  assert.ok(a.protocol.median_positive_twist_rate_deg_per_clock_unit > 0);
  assert.ok(a.protocol.median_positive_compression_rate_mm_per_clock_unit > 0);
  const forward = template([0, 1]);
  forward.controls = { max_step_s: 1000, max_steps: 2048 };
  assert.throws(() => replay(a, forward, 0.01, "named-direction-axial"), /Unresolved twist clock/);
  const [inverse, map] = inverseInputs();
  inverse.controls = forward.controls; map.specimen = "C06";
  assert.throws(() => calibration(root, JSON.stringify(inverse), JSON.stringify(map)), /Unresolved twist clock/);
});
