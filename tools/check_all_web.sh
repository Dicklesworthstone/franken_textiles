#!/bin/sh
# Reproducible runner for web/pkg and all 19 tools/check_*.cjs frontend, direct-WASM, and
# headless Chromium Playwright E2E suites (ftx-onn.29 / ftx-onn.31 / ftx-onn.14).
#
# Pass --with-replays to populate native->portable/browser replay directories via Cargo
# workflow tests and exercise the opt-in VM and Chromium replay lanes in addition to the
# default suites.
#
# Respects CARGO_TARGET_DIR, TMPDIR, FTX_PLAYWRIGHT_MODULE, CHROMIUM_EXECUTABLE, and NODE_PATH.
set -eu
cd "$(dirname "$0")/.." || exit 1
ROOT="$(pwd)"

WITH_REPLAYS=0
for arg in "$@"; do
  case "$arg" in
    --with-replays) WITH_REPLAYS=1 ;;
    *) echo "Usage: sh tools/check_all_web.sh [--with-replays]" >&2; exit 2 ;;
  esac
done

case "${CARGO_TARGET_DIR:-}" in
  /*) TARGET_DIR="$CARGO_TARGET_DIR" ;;
  ?*) TARGET_DIR="$ROOT/$CARGO_TARGET_DIR" ;;
  *)  TARGET_DIR="$ROOT/target" ;;
esac

WORK="${FTX_WEB_CHECK_DIR:-$(mktemp -d "${TMPDIR:-/tmp}/ftx-check-all-web.XXXXXX")}"

echo "[check_all_web] 1/5 Building web/pkg and Node WASM bindings..."
sh tools/build_web.sh
WASM_FILE="$TARGET_DIR/wasm32-unknown-unknown/release/ftx_wasm.wasm"
wasm-bindgen --target nodejs --out-dir "$WORK/node-pkg" "$WASM_FILE"

echo "[check_all_web] 2/5 Building native release ftx and ftx-materials..."
cargo build --locked --release -p ftx-cli
FTX_BIN="$TARGET_DIR/release/ftx"
FTX_MATERIALS_BIN="$TARGET_DIR/release/ftx-materials"

echo "[check_all_web] 3/5 Preparing synthetic offline browser editor fixtures..."
FTX_BODY_START_DIR="$WORK/body-starts" cargo test --locked --release -p ftx-cli --test body_fit_workflow start_edits_editor_fixture_replays_the_complete_capture_through_the_fitter
FTX_BODY_TARGET_DIR="$WORK/body-targets" cargo test --locked --release -p ftx-cli --test body_fit_workflow targets_editor_fixture_replays_a_corrected_capture_through_the_ordinary_fitter
FTX_CAPTURE_EDITOR_DIR="$WORK/capture-editor" cargo test --locked --release -p ftx-cli --test body_fit_workflow capture_editor_fixture_rejects_bad_check_and_keeps_a_positive_native_reference
FTX_CAPTURE_SETUP_DIR="$WORK/capture-setup" cargo test --locked --release -p ftx-cli --test body_fit_workflow capture_setup_constructs_the_first_request_and_reaches_the_ordinary_fitter

if [ "$WITH_REPLAYS" -eq 1 ]; then
  echo "[check_all_web] 3b/5 Populating opt-in native->browser replay directories (--with-replays)..."
  FTX_PHOTO_WEB_FIXTURES="$WORK/photo-web-fixtures" \
    cargo test --locked --release -p ftx-wasm --test photo retained_camera_uncertainty_reaches_browser_photo_measurements_without_resampling
  FTX_CAMERA_WEB_FIXTURES="$WORK/camera-web-fixtures" \
    cargo test --locked --release -p ftx-wasm --test photo local_images_calibrate_through_shared_acquisition_and_replay_into_photo_measurement
  FTX_EVIDENCE_REPLAY_DIR="$WORK/evidence-replay" \
    cargo test --locked --release -p ftx-wasm --test page supplemental_photo_records_join_tape_through_ordinary_fusion_and_the_full_round
  FTX_BODY_FIT_PORTABLE_DIR="$WORK/body-fit-portable" \
    cargo test --locked --release -p ftx-cli --test body_fit_workflow prepared_body_fit_replays_native_acceptance_and_failures_in_the_portable_engine
  FTX_DESIGN_SCREENING_REPLAY="$WORK/design-screening" \
    cargo test --locked --release -p ftx-cli --test accepted_body_workflow fit_screening_reaches_a_late_feasible_design_with_one_downstream_slot
  FTX_BODY_CHECK_CAPTURE_DIR="$WORK/body-check" \
    cargo test --locked --release -p ftx-cli --test accepted_body_workflow standalone_body_check_reuses_the_exact_shape_without_a_garment_or_tape_fallback
  FTX_MOTION_REPLAY="$WORK/motion-replay" \
    cargo test --locked --release -p ftx-cli --test accepted_body_workflow scenario_binaries_replay_accepted_shape_and_pose_with_real_drape_and_heat
  FTX_JOINT_DESIGN_CAPTURE_DIR="$WORK/joint-design" \
    cargo test --locked --release -p ftx-cli --test accepted_body_workflow independent_joint_polling_reaches_both_commands_and_exports_browser_replay
  FTX_DESIGN_ADAPTIVE_REPLAY="$WORK/design-adaptive" \
    cargo test --locked --release -p ftx-cli --test accepted_body_workflow adaptive_fit_finds_an_intermediate_design_in_native_and_portable_scenarios
  FTX_CONSUMER_ADAPTIVE_CAPTURE_DIR="$WORK/consumer-adaptive" \
    cargo test --locked --release -p ftx-cli --test accepted_body_workflow consumer_adaptive_uses_real_scenario_mass_and_preserves_cached_incumbent_on_budget_refusal
  FTX_CONSUMER_ADAPTIVE_CAPTURE_DIR="$WORK/consumer-adaptive" \
    cargo test --locked --release -p ftx-cli --test accepted_body_workflow independent_case_gates_preserve_signed_heat_tradeoffs_and_exact_native_outputs
  FTX_MOVING_BRIDGES_REPLAY="$WORK/moving-bridges" \
    cargo test --locked --release -p ftx-cli --test accepted_body_workflow qualified_moving_bridges_reach_real_heat_objectives_native_portable_and_atomic_refusals
  FTX_THREAD_COST_REPLAY="$WORK/thread-cost" \
  FTX_CUT_AND_SEW_REPLAY="$WORK/cut-and-sew" \
    cargo test --locked --release -p ftx-cli --test material_study_consumers -- --test-threads=1
  FTX_FINITE_KNIFE_REPLAY="$WORK/finite-knife" \
    cargo test --locked --release -p ftx-cli --test finite_stock_workflow qualified_finite_and_mixed_knife_programs_replay_through_both_binaries
  FTX_KNIFE_PROGRAM_REPLAY="$WORK/knife-program" \
    cargo test --locked --release -p ftx-cli --test cutting_workflow qualified_knife_paths_replay_through_both_commands_portable_api_and_material_studies
  FTX_ROUTED_FIXTURE_REPLAY="$WORK/routed-fixtures" \
  FTX_ROUTING_REPLAY="$WORK/routing-replay" \
    cargo test --locked --release -p ftx-export --test sewing_routing -- --test-threads=1
  FTX_ROUTED_FIXTURE_REPLAY="$WORK/routed-fixtures" \
  FTX_ELIGIBLE_ROUTING_REPLAY="$WORK/eligible-routing" \
    cargo test --locked --release -p ftx-cli --test sewing_production_workflow -- --test-threads=1
  FTX_COUPLED_WATER_ALTERNATIVES_REPLAY="$WORK/coupled-water-alternatives" \
  FTX_DEPTH_PROFILE_WATER_REPLAY="$WORK/depth-profile-water" \
  FTX_FINITE_VAPOR_RESERVOIR_REPLAY="$WORK/finite-vapor-reservoir" \
    cargo test --locked --release -p ftx-cli --test accepted_body_workflow surface_water

  node - "$FTX_BIN" "$WORK" "$ROOT" <<'EOF'
const fs = require("node:fs"), path = require("node:path"), zlib = require("node:zlib");
const { spawnSync } = require("node:child_process");
const [ftx, work, root] = process.argv.slice(2);

function runFtx(args, allowedCodes = [0], cwd = root) {
  const r = spawnSync(ftx, args, { cwd, encoding: "utf8", maxBuffer: 64 * 1024 * 1024 });
  if (!allowedCodes.includes(r.status)) {
    throw new Error(`ftx ${args.join(" ")} exited ${r.status}: ${r.stdout}\n${r.stderr}`);
  }
  return r.stdout;
}

function crc32(buf) {
  let c = 0xffffffff;
  for (let i = 0; i < buf.length; i++) {
    c ^= buf[i];
    for (let k = 0; k < 8; k++) c = (c >>> 1) ^ (0xedb88320 & -(c & 1));
  }
  return (c ^ 0xffffffff) >>> 0;
}

function chunk(type, data) {
  const t = Buffer.from(type, "ascii"), len = Buffer.alloc(4), crc = Buffer.alloc(4);
  len.writeUInt32BE(data.length, 0);
  crc.writeUInt32BE(crc32(Buffer.concat([t, data])), 0);
  return Buffer.concat([len, t, data, crc]);
}

function encodePngGray(w, h, pixels, colorType = 0) {
  const bpp = colorType === 6 ? 4 : 1;
  const raw = Buffer.alloc(h * (1 + w * bpp));
  for (let y = 0; y < h; y++) {
    raw[y * (1 + w * bpp)] = 0;
    for (let x = 0; x < w; x++) {
      const v = pixels[y * w + x];
      const off = y * (1 + w * bpp) + 1 + x * bpp;
      if (colorType === 6) { raw[off] = v; raw[off + 1] = v; raw[off + 2] = v; raw[off + 3] = 128; }
      else { raw[off] = v; }
    }
  }
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(w, 0); ihdr.writeUInt32BE(h, 4); ihdr[8] = 8; ihdr[9] = colorType;
  return Buffer.concat([
    Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]),
    chunk("IHDR", ihdr),
    chunk("IDAT", zlib.deflateSync(raw)),
    chunk("IEND", Buffer.alloc(0)),
  ]);
}

// 1. Printable & generated calibration sheet references for check_web.cjs
const sheetsDir = path.join(work, "calib-sheets");
fs.mkdirSync(sheetsDir, { recursive: true });
for (const paper of ["letter", "a4"]) {
  const sub = path.join(work, `calib-sheet-tmp-${paper}`);
  runFtx(["measure", "calib-sheet", "--paper", paper, "--out", sub]);
  for (const ext of ["png", "pdf", "json"]) {
    const name = `calib-sheet-${paper}.${ext}`;
    fs.copyFileSync(path.join(sub, name), path.join(sheetsDir, name));
  }
}
const genDir = path.join(work, "generated-sheets");
fs.mkdirSync(genDir, { recursive: true });
for (const [id, paper, sq, mk, dict] of [
  ["default-letter", "letter", 22, 16, "DICT_4X4_50"],
  ["default-a4", "a4", 22, 16, "DICT_4X4_50"],
  ["aruco4-large", "letter", 23, 15, "DICT_4X4_1000"],
  ["aruco5", "letter", 23, 15, "DICT_5X5_50"],
  ["aruco6", "a4", 18, 11, "DICT_6X6_250"],
  ["aruco7", "letter", 23, 15, "DICT_7X7_1000"],
  ["original", "a4", 18, 11, "DICT_ARUCO_ORIGINAL"],
  ["tag16", "letter", 22, 16, "DICT_APRILTAG_16H5"],
  ["tag25", "a4", 18, 11, "DICT_APRILTAG_25h9"],
  ["tag36-10", "letter", 23, 15, "DICT_APRILTAG_36H10"],
  ["tag36-11", "a4", 18, 11, "DICT_APRILTAG_36h11"],
  ["mip36", "letter", 22, 16, "DICT_ARUCO_MIP_36h12"],
]) {
  runFtx(["measure", "calib-sheet", "--paper", paper, "--square-mm", String(sq), "--marker-mm", String(mk), "--dictionary", dict, "--out", path.join(genDir, id)]);
}

// 2. Plain & JPEG photo fixtures for check_photo_web.cjs default + photoHandoff
const plainDir = path.join(work, "photo-plain");
fs.mkdirSync(plainDir, { recursive: true });
{
  const w = 720, h = 960, px = new Uint8Array(w * h);
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      const dx = Math.max(0, Math.min(x + 0.5, 150.2 + 215.9 * 1.5) - Math.max(x - 0.5, 150.2));
      const dy = Math.max(0, Math.min(y + 0.5, 170.4 + 279.4 * 1.5) - Math.max(y - 0.5, 170.4));
      px[y * w + x] = Math.round(90 + 152 * dx * dy);
    }
  }
  fs.writeFileSync(path.join(plainDir, "photo.png"), encodePngGray(w, h, px));
  fs.copyFileSync(path.join(root, "tests/fixtures/photo/plain-letter-exif6.jpg"), path.join(plainDir, "photo.jpg"));
  fs.writeFileSync(path.join(plainDir, "points-png.json"), '{"across_back":[[30,400],[630,400]]}');
  fs.writeFileSync(path.join(plainDir, "points-jpeg.json"), '{"across_back":[[30,400],[530,400]]}');
  fs.writeFileSync(path.join(plainDir, "blank.png"), encodePngGray(100, 100, new Uint8Array(10000).fill(90)));
  fs.writeFileSync(path.join(plainDir, "transparent.png"), encodePngGray(64, 64, new Uint8Array(4096).fill(120), 6));

  // Tilted sheet at axis=0 producing PAPER_AMBIGUOUS under --paper auto
  const aw = 960, ah = 960, apx = new Uint8Array(aw * ah).fill(70);
  const tilt = 32 * Math.PI / 180, c = Math.cos(tilt), s = Math.sin(tilt);
  const corners = [
    [-215.9 / 2, -279.4 / 2], [215.9 / 2, -279.4 / 2],
    [215.9 / 2, 279.4 / 2], [-215.9 / 2, 279.4 / 2],
  ].map(([rx, ry]) => {
    const z = 1200 + s * ry;
    return [479.5 + 1800 * rx / z, 479.5 + 1800 * (c * ry) / z];
  });
  const insideQuad = (pxX, pxY) => {
    for (let i = 0; i < 4; i++) {
      const [x1, y1] = corners[i], [x2, y2] = corners[(i + 1) % 4];
      if ((x2 - x1) * (pxY - y1) - (y2 - y1) * (pxX - x1) < 0) return false;
    }
    return true;
  };
  for (let y = 0; y < ah; y++) {
    for (let x = 0; x < aw; x++) if (insideQuad(x + 0.5, y + 0.5)) apx[y * aw + x] = 242;
  }
  // 3x3 box blur matching photo_detection.rs im.blur()
  const blurred = new Uint8Array(aw * ah);
  for (let y = 0; y < ah; y++) {
    for (let x = 0; x < aw; x++) {
      let sum = 0;
      for (let dy = -1; dy <= 1; dy++) {
        for (let dx = -1; dx <= 1; dx++) {
          const yy = Math.min(ah - 1, Math.max(0, y + dy)), xx = Math.min(aw - 1, Math.max(0, x + dx));
          sum += apx[yy * aw + xx];
        }
      }
      blurred[y * aw + x] = Math.round(sum / 9);
    }
  }
  fs.writeFileSync(path.join(plainDir, "ambiguous.png"), encodePngGray(aw, ah, blurred));

  fs.writeFileSync(path.join(plainDir, "native-scale-png.json"), runFtx(["measure", "photo-scale", "--image", "photo.png", "--paper", "letter", "--json"], [0], plainDir));
  fs.writeFileSync(path.join(plainDir, "native-scale-jpeg.json"), runFtx(["measure", "photo-scale", "--image", "photo.jpg", "--paper", "letter", "--json"], [0], plainDir));
  fs.writeFileSync(path.join(plainDir, "native-measure-png.json"), runFtx(["measure", "measure-photo", "--image", "photo.png", "--paper", "letter", "--points", "points-png.json", "--json"], [0], plainDir));
  fs.writeFileSync(path.join(plainDir, "native-measure-jpeg.json"), runFtx(["measure", "measure-photo", "--image", "photo.jpg", "--paper", "letter", "--points", "points-jpeg.json", "--json"], [0], plainDir));
}

// 3. ChArUco printed-dictionary fixtures for check_photo_web.cjs --charuco
const charucoDir = path.join(work, "photo-charuco");
runFtx(["measure", "calib-sheet", "--paper", "letter", "--out", charucoDir]);
{
  const sheetPath = path.join(charucoDir, "calib-sheet-letter.json");
  const imgPath = path.join(charucoDir, "calib-sheet-letter.png");
  const b = JSON.parse(fs.readFileSync(sheetPath, "utf8")).board;
  const pt = (x, y) => [b.origin_px[0] - 0.5 + x * b.size_px[0] / b.size_mm[0], b.origin_px[1] - 0.5 + y * b.size_px[1] / b.size_mm[1]];
  fs.writeFileSync(path.join(charucoDir, "points.json"), JSON.stringify({ across_back: [pt(44, 44), pt(132, 44)] }));
  fs.writeFileSync(path.join(charucoDir, "native-scale.json"), runFtx(["measure", "photo-scale", "--image", imgPath, "--sheet", sheetPath, "--json"], [0], charucoDir));
  fs.writeFileSync(path.join(charucoDir, "native-measurements.json"), runFtx(["measure", "measure-photo", "--image", imgPath, "--sheet", sheetPath, "--points", "points.json", "--json"], [0], charucoDir));
}

// 4. Triangulation fixtures for FTX_TRIANGULATION_FIXTURES & check_photo_web.cjs --triangulate
const triDir = path.join(work, "triangulation-fixtures");
fs.mkdirSync(triDir, { recursive: true });
{
  const plainText = fs.readFileSync(path.join(root, "docs/examples/triangulation.json"), "utf8");
  const plain = JSON.parse(plainText);
  const noiseText = JSON.stringify({
    schema: "ftx-click-noise/1",
    evidence: "synthetic one-pixel clicks",
    observations: [
      { frame: { stream: "synthetic-left", timestamp_us: 1000000 }, sigma_px: 1 },
      { frame: { stream: "synthetic-right", timestamp_us: 1000000 }, sigma_px: 1 },
    ],
  }, null, 2);
  fs.writeFileSync(path.join(triDir, "plain-input.json"), plainText);
  runFtx(["measure", "triangulate", "plain-input.json", "--out", "plain-native.json"], [0], triDir);

  fs.writeFileSync(path.join(triDir, "noise-input.json"), plainText);
  fs.writeFileSync(path.join(triDir, "noise-noise.json"), noiseText);
  runFtx(["measure", "triangulate", "noise-input.json", "--click-noise", "noise-noise.json", "--out", "noise-native.json"], [0], triDir);

  const unavailNoise = JSON.stringify({
    schema: "ftx-click-noise/1",
    evidence: "synthetic underflowing clicks to exercise unavailable covariance",
    observations: [
      { frame: { stream: "synthetic-left", timestamp_us: 1000000 }, sigma_px: 1e-200 },
      { frame: { stream: "synthetic-right", timestamp_us: 1000000 }, sigma_px: 1e-200 },
    ],
  }, null, 2);
  fs.writeFileSync(path.join(triDir, "unavailable-input.json"), plainText);
  fs.writeFileSync(path.join(triDir, "unavailable-noise.json"), unavailNoise);
  runFtx(["measure", "triangulate", "unavailable-input.json", "--click-noise", "unavailable-noise.json", "--out", "unavailable-native.json"], [2], triDir);

  const weak = structuredClone(plain);
  weak.observations[0].camera.center_m = [-0.01, -3, 1];
  weak.observations[0].pixel_px = [503, 310];
  weak.observations[1].camera.center_m = [0.01, -3, 1];
  weak.observations[1].pixel_px = [497, 310];
  fs.writeFileSync(path.join(triDir, "weak-input.json"), JSON.stringify(weak, null, 2));
  runFtx(["measure", "triangulate", "weak-input.json", "--out", "weak-native.json"], [2], triDir);

  const heldOut = structuredClone(plain);
  heldOut.observations[2].pixel_px = [540, 310];
  fs.writeFileSync(path.join(triDir, "held-out-input.json"), JSON.stringify(heldOut, null, 2));
  runFtx(["measure", "triangulate", "held-out-input.json", "--out", "held-out-native.json"], [2], triDir);

  const fwInput = plainText.replaceAll("1000000", "18446744073709551615");
  const fwNoise = noiseText.replaceAll("1000000", "18446744073709551615");
  fs.writeFileSync(path.join(triDir, "full-width-input.json"), fwInput);
  fs.writeFileSync(path.join(triDir, "full-width-noise.json"), fwNoise);
  runFtx(["measure", "triangulate", "full-width-input.json", "--click-noise", "full-width-noise.json", "--out", "full-width-native.json"], [0], triDir);
}

// 5. Bundle fixtures for FTX_BUNDLE_FIXTURES & check_photo_web.cjs --bundle
const bundleDir = path.join(work, "bundle-fixtures");
fs.mkdirSync(bundleDir, { recursive: true });
{
  const accText = fs.readFileSync(path.join(root, "docs/examples/bundle.json"), "utf8");
  const acc = JSON.parse(accText);
  fs.writeFileSync(path.join(bundleDir, "accepted-input.json"), accText);
  runFtx(["measure", "bundle-adjust", "accepted-input.json", "--iterations", "50", "--out", "accepted-native.json"], [0], bundleDir);

  fs.writeFileSync(path.join(bundleDir, "budget-input.json"), accText);
  runFtx(["measure", "bundle-adjust", "budget-input.json", "--iterations", "1", "--out", "budget-native.json"], [4], bundleDir);

  const held = structuredClone(acc);
  held.observations[17].pixel_px[1] = 360;
  fs.writeFileSync(path.join(bundleDir, "held-out-input.json"), JSON.stringify(held, null, 2));
  runFtx(["measure", "bundle-adjust", "held-out-input.json", "--iterations", "50", "--out", "held-out-native.json"], [2], bundleDir);

  const deg = structuredClone(acc);
  deg.controls = [
    { id: "c0", position_m: [-0.4, 0, 1.0] },
    { id: "c1", position_m: [-0.1, 0, 1.0] },
    { id: "c2", position_m: [0.2, 0, 1.0] },
    { id: "c3", position_m: [0.4, 0, 1.0] },
    { id: "c4", position_m: [0.0, 0, 1.0] },
  ];
  fs.writeFileSync(path.join(bundleDir, "degenerate-input.json"), JSON.stringify(deg, null, 2));
  runFtx(["measure", "bundle-adjust", "degenerate-input.json", "--iterations", "50", "--out", "degenerate-native.json"], [2], bundleDir);

  const fwBundle = accText.replaceAll("100000", "18446744073709551615").replace('"start_us": 0', '"start_us": 18446744073709451615').replaceAll('"timestamp_us": 0', '"timestamp_us": 18446744073709451615').replaceAll('"timestamp_us": 50000', '"timestamp_us": 18446744073709501615');
  fs.writeFileSync(path.join(bundleDir, "full-width-input.json"), fwBundle);
  runFtx(["measure", "bundle-adjust", "full-width-input.json", "--iterations", "50", "--out", "full-width-native.json"], [0], bundleDir);

  fs.copyFileSync(path.join(triDir, "plain-input.json"), path.join(bundleDir, "triangulation-input.json"));
  fs.copyFileSync(path.join(triDir, "plain-native.json"), path.join(bundleDir, "triangulation-native.json"));
}

// 6. Still-window fixtures for FTX_STILL_FIXTURES & check_photo_web.cjs --still-window
const stillDir = path.join(work, "still-fixtures");
fs.mkdirSync(stillDir, { recursive: true });
{
  const makeStill = (xs, fitMotion, heldMotion) => {
    const landmarks = [
      ["left", [-0.2, 0.0, 0.9], "fit"],
      ["right", [0.2, 0.0, 0.9], "fit"],
      ["waist", [0.0, 0.0, 0.6], "fit"],
      ["held", [0.0, 0.0, 1.1], "held_out"],
    ];
    const frames = xs.map((x, i) => {
      const late = i >= 2;
      const pts = landmarks.map(([name, [lx, ly, lz], part]) => {
        const dx = late ? (part === "held_out" ? heldMotion : fitMotion) : 0.0;
        const px = 640 + 900 * (lx + dx - x) / (ly + 2.0);
        const py = 480 - 900 * (lz - 0.7) / (ly + 2.0);
        return { name, pixel_px: [px, py], partition: part };
      });
      return {
        frame: { stream: "synthetic-clip", timestamp_us: i * 50000 },
        camera: {
          intrinsics: { focal_px: [900, 900], principal_px: [640, 480] },
          world_to_camera: { rows: [[1, 0, 0], [0, 0, -1], [0, 1, 0]] },
          center_m: [x, -2.0, 0.7],
        },
        landmarks: pts,
      };
    });
    return {
      schema: "ftx-still-window-input/1",
      metric_scale_source: "synthetic stationary reference",
      observation_evidence: "analytically projected synthetic landmarks",
      max_motion_mm: 5.0,
      frames,
    };
  };
  for (const [name, obj, codes] of [
    ["stationary", makeStill([-0.45, -0.15, 0.15, 0.45], 0.0, 0.0), [0]],
    ["fit-motion", makeStill([-0.45, -0.15, 0.15, 0.45], 0.010, 0.0), [2]],
    ["held-out-motion", makeStill([-0.45, -0.15, 0.15, 0.45], 0.0, 0.010), [2]],
    ["weak", makeStill([-0.02, -0.007, 0.007, 0.02], 0.0, 0.0), [2]],
  ]) {
    fs.writeFileSync(path.join(stillDir, `${name}-input.json`), JSON.stringify(obj, null, 2));
    runFtx(["measure", "still-window", `${name}-input.json`, "--out", `${name}-native.json`], codes, stillDir);
  }
  const fwStill = fs.readFileSync(path.join(stillDir, "stationary-input.json"), "utf8")
    .replace('"timestamp_us": 0', '"timestamp_us": 18446744073709401615')
    .replace('"timestamp_us": 50000', '"timestamp_us": 18446744073709451615')
    .replace('"timestamp_us": 100000', '"timestamp_us": 18446744073709501615')
    .replace('"timestamp_us": 150000', '"timestamp_us": 18446744073709551615');
  fs.writeFileSync(path.join(stillDir, "full-width-input.json"), fwStill);
  runFtx(["measure", "still-window", "full-width-input.json", "--out", "full-width-native.json"], [0], stillDir);

  fs.copyFileSync(path.join(triDir, "plain-input.json"), path.join(stillDir, "triangulation-input.json"));
  fs.copyFileSync(path.join(triDir, "plain-native.json"), path.join(stillDir, "triangulation-native.json"));
  fs.copyFileSync(path.join(bundleDir, "accepted-input.json"), path.join(stillDir, "bundle-input.json"));
  fs.copyFileSync(path.join(bundleDir, "accepted-native.json"), path.join(stillDir, "bundle-native.json"));
}

// 7. Tape-fitted body-check fixture for FTX_BODY_CHECK_DIR (when admitted Anny model is present)
const admittedModel = process.env.FTX_BODY_MODEL || path.join(process.env.HOME || "", ".cache/franken_textiles/anny-0.6.1-default.ftxbody");
if (fs.existsSync(admittedModel)) {
  const bodyTapeDir = path.join(work, "body-check-tape");
  fs.mkdirSync(bodyTapeDir, { recursive: true });
  const measPath = path.join(bodyTapeDir, "measurements.json");
  runFtx(["measure", "reference", "--stature-mm", "920", "--age-months", "30", "--out", measPath], [0], bodyTapeDir);
  const measDoc = JSON.parse(fs.readFileSync(measPath, "utf8"));
  if (Array.isArray(measDoc.records) && !measDoc.records.some(r => r.name === "head")) {
    measDoc.records.push({
      name: "head",
      value_mm: 490.0,
      sigma_mm: 5.0,
      provenance: "PUBLIC_PRIOR",
      method: "synthetic body-check reference",
      timestamp: null,
      note: "synthetic head circumference target",
    });
    fs.writeFileSync(measPath, JSON.stringify(measDoc, null, 2));
  }
  const stdout = runFtx(["body", "--measurements", measPath, "--model", admittedModel, "--pose", "drape", "--out", path.join(bodyTapeDir, "body-out"), "--json"], [0], bodyTapeDir);
  fs.writeFileSync(path.join(bodyTapeDir, "native.stdout"), stdout);
}
EOF
fi

echo "[check_all_web] 4/5 Running all 9 Node VM suites (FTX_REAL_WASM=1) + 3 direct *_wasm.cjs suites..."
# check_trace_frontend.cjs requires 7 sibling VM suites (8 total) + check_material_screen_builder.cjs = all 9 VM suites.
if [ "$WITH_REPLAYS" -eq 1 ]; then
  ADMITTED_MODEL="${FTX_BODY_MODEL:-${HOME:-}/.cache/franken_textiles/anny-0.6.1-default.ftxbody}"
  BODY_CHECK_ENV_DIR=""
  BODY_CHECK_ENV_MODEL=""
  if [ -f "$ADMITTED_MODEL" ] && [ -d "$WORK/body-check-tape" ]; then
    BODY_CHECK_ENV_DIR="$WORK/body-check-tape"
    BODY_CHECK_ENV_MODEL="$ADMITTED_MODEL"
  fi
  FTX_REAL_WASM=1 \
  FTX_GUIDED_SCENARIO_NATIVE="$FTX_MATERIALS_BIN" \
  FTX_GUIDED_DESIGN_NATIVE="$FTX_MATERIALS_BIN" \
  FTX_GUIDED_MATRIX_NATIVE="$FTX_MATERIALS_BIN" \
  FTX_GUIDED_MOTION_NATIVE="$FTX_MATERIALS_BIN" \
  FTX_GUIDED_SEAM_NATIVE="$FTX_MATERIALS_BIN" \
  FTX_GUIDED_CONTACT_NATIVE="$FTX_MATERIALS_BIN" \
  FTX_RESIDUAL_STOCK_NATIVE="$FTX_MATERIALS_BIN" \
  FTX_GUIDED_DESIGN_BODY="$WORK/design-screening" \
  FTX_GUIDED_SCENARIO_BODY="$WORK/consumer-adaptive" \
  FTX_GUIDED_MATRIX_BODY="$WORK/consumer-adaptive" \
  FTX_GUIDED_MOTION_BODY="$WORK/consumer-adaptive" \
  FTX_GUIDED_SEAM_BODY="$WORK/consumer-adaptive" \
  FTX_GUIDED_CONTACT_BODY="$WORK/consumer-adaptive" \
  FTX_PHOTO_WEB_FIXTURES="$WORK/photo-web-fixtures" \
  FTX_CAMERA_WEB_FIXTURES="$WORK/camera-web-fixtures" \
  FTX_BODY_FIT_PORTABLE_DIR="$WORK/body-fit-portable" \
  FTX_TRIANGULATION_FIXTURES="$WORK/triangulation-fixtures" \
  FTX_BUNDLE_FIXTURES="$WORK/bundle-fixtures" \
  FTX_STILL_FIXTURES="$WORK/still-fixtures" \
  FTX_CONSUMER_ADAPTIVE_REPLAY="$WORK/consumer-adaptive" \
  FTX_BODY_CHECK_DIR="$BODY_CHECK_ENV_DIR" \
  FTX_BODY_MODEL="$BODY_CHECK_ENV_MODEL" \
  FTX_DESIGN_SCREENING_REPLAY="$WORK/design-screening" \
  FTX_MOTION_REPLAY="$WORK/motion-replay" \
  FTX_JOINT_DESIGN_CAPTURE_DIR="$WORK/joint-design" \
  FTX_JOINT_DESIGN_REPLAY="$WORK/joint-design" \
  FTX_DESIGN_ADAPTIVE_REPLAY="$WORK/design-adaptive" \
  FTX_MOVING_BRIDGES_REPLAY="$WORK/moving-bridges" \
  FTX_THREAD_COST_REPLAY="$WORK/thread-cost" \
  FTX_CUT_AND_SEW_REPLAY="$WORK/cut-and-sew" \
  FTX_FINITE_KNIFE_REPLAY="$WORK/finite-knife" \
  FTX_KNIFE_PROGRAM_REPLAY="$WORK/knife-program" \
  FTX_ROUTED_FIXTURE_REPLAY="$WORK/routed-fixtures" \
  FTX_ELIGIBLE_ROUTING_REPLAY="$WORK/eligible-routing" \
  FTX_COUPLED_WATER_ALTERNATIVES_REPLAY="$WORK/coupled-water-alternatives" \
  FTX_DEPTH_PROFILE_WATER_REPLAY="$WORK/depth-profile-water" \
  FTX_FINITE_VAPOR_RESERVOIR_REPLAY="$WORK/finite-vapor-reservoir" \
    node --experimental-vm-modules --test tools/check_trace_frontend.cjs tools/check_material_screen_builder.cjs
else
  FTX_REAL_WASM=1 FTX_GUIDED_SCENARIO_NATIVE="$FTX_MATERIALS_BIN" node --experimental-vm-modules --test tools/check_trace_frontend.cjs tools/check_material_screen_builder.cjs
fi
node --experimental-vm-modules tools/check_materials_wasm.cjs --native "$FTX_MATERIALS_BIN"
node --experimental-vm-modules tools/check_strip_wasm.cjs --native "$FTX_MATERIALS_BIN"
node tools/check_sewing_wasm.cjs "$WORK/node-pkg/ftx_wasm.js" "$FTX_MATERIALS_BIN"

echo "[check_all_web] 5/5 Running all 7 headless Chromium Playwright E2E suites..."
if [ -z "${CHROMIUM_EXECUTABLE:-}" ] && [ -z "${FTX_CHROMIUM_EXECUTABLE:-}" ]; then
  DETECTED_CHROMIUM="$(node -e 'const fs=require("node:fs"),{chromium}=require(process.env.FTX_PLAYWRIGHT_MODULE||"playwright"),p=chromium.executablePath();if(p&&fs.existsSync(p))process.stdout.write(p);' 2>/dev/null || true)"
  if [ -n "$DETECTED_CHROMIUM" ]; then
    export CHROMIUM_EXECUTABLE="$DETECTED_CHROMIUM"
  fi
fi
if [ "$WITH_REPLAYS" -eq 1 ]; then
  node tools/check_web.cjs --ftx "$FTX_BIN" --measurement-evidence "$WORK/evidence-replay"
  node tools/check_web.cjs --ftx "$FTX_BIN" --trace-edits
  node tools/check_web.cjs --ftx "$FTX_BIN" --alterations
  node tools/check_web.cjs --ftx "$FTX_BIN" --body-check "$WORK/body-check"
  node tools/check_web.cjs --calibration-sheets "$WORK/calib-sheets"
  node tools/check_web.cjs --generated-calibration-sheets "$WORK/generated-sheets"
  node tools/check_materials_web.cjs --native "$FTX_MATERIALS_BIN"
  node tools/check_materials_web.cjs --native "$FTX_MATERIALS_BIN" --outfit-proof
  node tools/check_materials_web.cjs --native "$FTX_MATERIALS_BIN" --compression-proof
  node tools/check_materials_web.cjs --native "$FTX_MATERIALS_BIN" --purchasing-proof
  node tools/check_materials_web.cjs --native "$FTX_MATERIALS_BIN" --guided-study-proof
  node tools/check_photo_web.cjs "$WORK/photo-plain"
  node tools/check_photo_web.cjs --section
  node tools/check_photo_web.cjs --charuco "$WORK/photo-charuco"
  node tools/check_photo_web.cjs --triangulate "$WORK/triangulation-fixtures"
  node tools/check_photo_web.cjs --bundle "$WORK/bundle-fixtures"
  node tools/check_photo_web.cjs --still-window "$WORK/still-fixtures"
  node tools/check_photo_web.cjs --fit-body "$WORK/body-fit-portable"
else
  node tools/check_web.cjs --ftx "$FTX_BIN"
  node tools/check_materials_web.cjs --native "$FTX_MATERIALS_BIN"
  node tools/check_photo_web.cjs --section
fi
node tools/check_body_starts.cjs "$WORK/body-starts" "$WORK/body-starts-out" "$FTX_BIN"
node tools/check_body_targets.cjs "$WORK/body-targets" "$WORK/body-targets-out"
node tools/check_capture_review.cjs "$WORK/capture-editor" "$WORK/capture-editor-out"
node tools/check_capture_setup.cjs "$WORK/capture-setup" "$WORK/capture-setup-out" "$FTX_BIN"

echo "[check_all_web] PASS: all 19 frontend/WASM/Playwright suites passed (with_replays=$WITH_REPLAYS, work dir: $WORK)"
