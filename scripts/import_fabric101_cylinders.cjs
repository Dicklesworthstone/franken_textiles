#!/usr/bin/env node
"use strict";

// Offline reader for the authors' Fabric-101 L01 release. All channels keep instrument signs.
const fs = require("node:fs");
const path = require("node:path");
const { createHash } = require("node:crypto");

const revision = "510519be0657a341a1a84fdb5c24be156ac83c9f";
const base = "data/L01/001/";
const stages = ["twist", "compression", "friction"];
const pins = {
  "head.dat": "02872ba3be75fd3e3f3a01367289b8314ecba93b",
  "warp/s1/tarevalues.csv": "207edb65aaf011a24b9169eaae1857d0cbc5515a",
  "warp/s1/twist/twist.dat": "8e386b5b9473fc021db8af50cc12893c57852c92",
  "warp/s1/compression/compression.dat": "24e40d5af41e0709510f41911878c310bb166414",
  "warp/s1/friction/friction.dat": "fdac4c9fa64304790e58f06deff77344b869562a",
  "weft/s1/tarevalues.csv": "300552a673e0788ff2019b8806bf7c7e4fc842b6",
  "weft/s1/twist/twist.dat": "28f956e1c8e3cd52fb41768b36acc7dce80e3d69",
  "weft/s1/compression/compression.dat": "fee8a6c940b5f5acc98987c457892e39ca0bab9d",
  "weft/s1/friction/friction.dat": "992a622f03cb840aa4e473351f467d268e247228",
};

function loadPinned(root, name, sources) {
  const file = path.join(root, base, name);
  if (fs.statSync(file).size > 4_000_000) throw Error("Recording exceeds pinned file size limit: " + name);
  const raw = fs.readFileSync(file);
  const blob = createHash("sha1").update("blob " + raw.length + "\0").update(raw).digest("hex");
  if (blob !== pins[name]) throw Error("Recording differs from the pinned Git blob: " + name);
  sources[base + name] = { git_blob: blob, sha256: createHash("sha256").update(raw).digest("hex") };
  return raw.toString("ascii");
}

const number = "([-+]?\\d+(?:\\.\\d+)?(?:[eE][-+]?\\d+)?)";
const sample = new RegExp("^" + ["time", "loadPos", "loadForce", "torquePos", "torqueForce"]
  .map(label => label + ":\\s*" + number).join("\\s+") + "$");

function readSamples(text) {
  const lines = text.split(/\r?\n/);
  const header = lines.indexOf("Time LoadPosition Load TorquePosition Torque");
  if (header < 0 || lines[header + 1]?.trim().split(/\s+/).join(" ") !== "(s) (mm) (N) (Degree) (N-m)") {
    throw Error("Unsupported instrument channel order or units");
  }
  const rows = [];
  for (const line of lines.slice(header + 2)) {
    if (!line.trim()) continue;
    const match = sample.exec(line.trim());
    if (!match) throw Error("Incomplete or unrecognized instrument sample");
    const row = match.slice(1).map(Number);
    if (!row.every(Number.isFinite) || row[0] < 0 || (rows.length && row[0] < rows.at(-1)[0])) {
      throw Error("Nonfinite measurement or decreasing stage clock");
    }
    rows.push(row);
  }
  if (rows.length < 2) throw Error("Expected multiple instrument samples");
  return rows;
}

function readTares(text) {
  const result = {};
  let stage;
  for (const line of text.split(/\r?\n/)) {
    if (!line.trim()) continue;
    const title = /^\*\*(.+)\*\*$/.exec(line);
    if (title) {
      stage = {};
      result[title[1]] = stage;
    } else {
      const [name, value, unit] = line.split(",");
      if (!stage || !value || !Number.isFinite(Number(value))) throw Error("Invalid tare measurement");
      stage[name] = { value: Number(value), unit: unit ?? null };
    }
  }
  if (result["After Pretension"]?.pret_moved?.unit !== "mm") throw Error("Missing measured mounting travel");
  return result;
}

function medianPositiveRate(rows, axis) {
  const rates = [];
  for (let i = 1; i < rows.length; i++) {
    const diff = rows[i][axis] - rows[i - 1][axis], dt = rows[i][0] - rows[i - 1][0];
    if (diff > 0 && dt > 0) rates.push(diff / dt);
  }
  rates.sort((a, b) => a - b);
  if (!rates.length) throw Error("No positive actuator travel in the recording");
  const mid = Math.floor(rates.length / 2);
  return rates.length % 2 ? rates[mid] : (rates[mid - 1] + rates[mid]) / 2;
}

function acquire(root, direction) {
  if (!["warp", "weft"].includes(direction)) throw Error("Choose warp or weft");
  const sources = {}, prefix = direction + "/s1/";
  loadPinned(root, "head.dat", sources);
  const tares = readTares(loadPinned(root, prefix + "tarevalues.csv", sources));
  const phases = {};
  for (const stage of stages) {
    phases[stage] = readSamples(loadPinned(root, prefix + stage + "/" + stage + ".dat", sources));
  }
  return {
    schema: "ftx-fabric101-l01-acquisition/1", release_commit: revision,
    material_id: "fabric101-2026/l01", instrument_specimen: "200_L01_001", direction, repeat: "s1",
    sources, tare_stages: tares,
    columns: ["stage_time_s", "instrument_load_position_mm", "instrument_load_n",
      "instrument_rotation_deg", "instrument_torque_nm"],
    phase_order: [...stages], phases,
    protocol: {
      median_positive_twist_rate_deg_s: medianPositiveRate(phases.twist, 3),
      median_positive_compression_rate_mm_s: medianPositiveRate(phases.compression, 1),
      peak_instrument_compression_mm: Math.max(...phases.compression.map(row => row[1])),
      measured_pretension_travel_mm: -tares["After Pretension"].pret_moved.value,
    },
  };
}

// Bound angle errors at sensor times; preserve both ends of every hold and every reversal.
function twistIndices(rows, tolerance) {
  if (!Number.isFinite(tolerance) || tolerance <= 0) throw Error("Positive rotation error tolerance required");
  if (rows[0][3] !== 0 || rows.some((r, i) => r[1] !== 0 || (i && r[0] <= rows[i - 1][0]))) {
    throw Error("Twist replay needs zero starting rotation, fixed axial position and distinct times");
  }
  const indices = [0];
  let sign = Math.sign(rows[1][3] - rows[0][3]);
  for (let i = 1; i < rows.length - 1; i++) {
    const next = Math.sign(rows[i + 1][3] - rows[i][3]);
    if (next !== sign) indices.push(i);
    sign = next;
  }
  indices.push(rows.length - 1);
  const segments = indices.slice(1).map((right, i) => [indices[i], right]);
  let maxError = 0;
  while (segments.length) {
    if (indices.length > 129) throw Error("Measured twist needs more than 129 native waypoints");
    const [left, right] = segments.pop(), a = rows[left], b = rows[right];
    const slope = (b[3] - a[3]) / (b[0] - a[0]);
    let worst = 0, split = left;
    for (let i = left + 1; i < right; i++) {
      const error = Math.abs(rows[i][3] - (a[3] + slope * (rows[i][0] - a[0])));
      if (error > worst) { worst = error; split = i; }
    }
    if (worst <= tolerance) maxError = Math.max(maxError, worst);
    else {
      indices.push(split);
      segments.push([left, split], [split, right]);
    }
  }
  return { indices: indices.sort((a, b) => a - b), maxError };
}

function replay(acquisition, template, tolerance, mounting) {
  if (template?.schema !== "ftx-cylinder-experiment/1" || !template.material || !template.fixture) {
    throw Error("Expected an ftx-cylinder-experiment/1 template with a material and fixture");
  }
  if (!["named-direction-axial", "named-direction-circumferential"].includes(mounting)) {
    throw Error("Declare a mounting convention for the source directory direction");
  }
  const fixture = template.fixture;
  if (fixture.holder_radius_m !== 0.04 || fixture.nominal_free_height_m !== 0.05 ||
      fixture.specimen_width_m !== 0.26 || fixture.specimen_height_m !== 0.11) {
    throw Error("Template must match the 40 mm radius, 50 mm gauge, 260 by 110 mm specimen");
  }
  const namedAxial = mounting === "named-direction-axial";
  const warpAxial = (acquisition.direction === "warp") === namedAxial;
  const axis = fixture.warp_axis;
  if (!Array.isArray(axis) || axis.length !== 2 || Math.abs(axis[0]) !== (warpAxial ? 0 : 1) ||
      Math.abs(axis[1]) !== (warpAxial ? 1 : 0) || !axis.every(Number.isFinite)) {
    throw Error("Template warp_axis disagrees with the declared mounting for " + acquisition.direction);
  }
  const rows = acquisition.phases.twist;
  const { indices, maxError } = twistIndices(rows, tolerance);
  const loading = indices.map(i => ({ time_s: rows[i][0] - rows[0][0], rotation_deg: -rows[i][3],
    axial_displacement_m: 0, clamp_slip_m: 0 }));
  const step = template.controls?.max_step_s ?? 0.05, limit = template.controls?.max_steps ?? 1024;
  if (!Number.isFinite(step) || step <= 0 || !Number.isInteger(limit) || limit < 1 || limit > 2048) {
    throw Error("Invalid template integration controls");
  }
  const steps = loading.slice(1).reduce((n, p, i) => n + Math.ceil((p.time_s - loading[i].time_s) / step), 0);
  if (steps > limit) throw Error("Template integration budget cannot cover the measured twist");
  const travel = acquisition.protocol.measured_pretension_travel_mm * 0.001;
  const request = { ...template, fixture: { ...fixture, pretension_displacement_m: travel }, loading };
  // The native positive-angle rate would describe the return stroke in this reversed frame.
  delete request.nominal_protocol;
  const receipt = { sample_indices: indices, mounting_convention: mounting, warp_axis: [...axis],
    source_rotating_holder: "lower", model_rotating_holder: "upper",
    rotation_scope: "relative_holder_angle_only", rotation_sign: -1,
    axial_grip_slip_assumption_m: 0, stage_clock_origin_s: rows[0][0],
    tolerance_deg: tolerance, max_rotation_error_deg: maxError, integration_steps: steps };
  return { request, receipt };
}

function main(args) {
  const [root, direction, templatePath, requestPath, angleError, mounting] = args;
  if (args.length !== 2 && args.length !== 6) {
    throw Error("Usage: node scripts/import_fabric101_cylinders.cjs RELEASE warp|weft [TEMPLATE.json NEW_REQUEST.json ANGLE_ERROR_DEG named-direction-axial|named-direction-circumferential]");
  }
  const result = acquire(root, direction);
  if (templatePath) {
    const template = JSON.parse(fs.readFileSync(templatePath, "utf8"));
    const { request, receipt } = replay(result, template, Number(angleError), mounting);
    const encoded = JSON.stringify(request, null, 2) + "\n";
    receipt.request_sha256 = createHash("sha256").update(encoded).digest("hex");
    fs.writeFileSync(requestPath, encoded, { flag: "wx" });
    result.twist_replay = receipt;
  }
  process.stdout.write(JSON.stringify(result) + "\n");
}

module.exports = { acquire, readSamples, replay, twistIndices };
if (require.main === module) {
  try { main(process.argv.slice(2)); }
  catch (error) { process.stderr.write(error.message + "\n"); process.exitCode = 1; }
}
