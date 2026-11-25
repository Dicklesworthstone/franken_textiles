"use strict";

const { test } = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { acquire, readSamples, replay, twistIndices } = require("./import_fabric101_cylinders.cjs");

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
  receipt.sample_indices.forEach((i, j) => assert.equal(request.loading[j].rotation_deg, -points[i][1]));
  assert.throws(() => replay(source, { ...input, controls: { max_steps: 1 } }, 0.1, "named-direction-axial"), /budget/);
  assert.throws(() => replay(source, input, 0.1, undefined), /convention/);
  assert.throws(() => replay(source, input, 0.1, "named-direction-circumferential"), /warp_axis/);
  assert.deepEqual(receipt.warp_axis, [1, 0]);
  assert.equal(receipt.axial_grip_slip_assumption_m, 0);
  assert.equal(receipt.rotation_scope, "relative_holder_angle_only");
});

test("refuses to silently omit axial movement or excessive rotation reversals", () => {
  const rows = Array.from({ length: 130 }, (_, i) => [i, 0, 0, i % 2, 0]);
  assert.throws(() => twistIndices(rows, 2), /129/);
  rows[4][1] = 1;
  assert.throws(() => twistIndices(rows, 2), /fixed axial/);
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
