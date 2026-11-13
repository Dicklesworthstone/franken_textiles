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
VM_ONLY=0
for arg in "$@"; do
  case "$arg" in
    --with-replays) WITH_REPLAYS=1 ;;
    --vm-only) VM_ONLY=1 ;;
    *) echo "Usage: sh tools/check_all_web.sh [--with-replays] [--vm-only]" >&2; exit 2 ;;
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

if cargo --list 2>/dev/null | grep -qw lbuild; then
  CARGO_BUILD=lbuild
  CARGO_TEST=ltest
else
  CARGO_BUILD=build
  CARGO_TEST=test
fi

echo "[check_all_web] 2/5 Building native release ftx and ftx-materials..."
cargo "$CARGO_BUILD" --locked --release -p ftx-cli
FTX_BIN="$TARGET_DIR/release/ftx"
FTX_MATERIALS_BIN="$TARGET_DIR/release/ftx-materials"

echo "[check_all_web] 3/5 Preparing synthetic offline browser editor fixtures..."
FTX_BODY_START_DIR="$WORK/body-starts" cargo "$CARGO_TEST" --locked --release -p ftx-cli --test body_fit_workflow start_edits_editor_fixture_replays_the_complete_capture_through_the_fitter
FTX_BODY_TARGET_DIR="$WORK/body-targets" cargo "$CARGO_TEST" --locked --release -p ftx-cli --test body_fit_workflow targets_editor_fixture_replays_a_corrected_capture_through_the_ordinary_fitter
FTX_CAPTURE_EDITOR_DIR="$WORK/capture-editor" cargo "$CARGO_TEST" --locked --release -p ftx-cli --test body_fit_workflow capture_editor_fixture_rejects_bad_check_and_keeps_a_positive_native_reference
FTX_CAPTURE_SETUP_DIR="$WORK/capture-setup" cargo "$CARGO_TEST" --locked --release -p ftx-cli --test body_fit_workflow capture_setup_constructs_the_first_request_and_reaches_the_ordinary_fitter

if [ "$WITH_REPLAYS" -eq 1 ]; then
  echo "[check_all_web] 3b/5 Populating opt-in native->browser replay directories (--with-replays)..."
  FTX_PHOTO_WEB_FIXTURES="$WORK/photo-web-fixtures" \
    cargo "$CARGO_TEST" --locked --release -p ftx-wasm --test photo retained_camera_uncertainty_reaches_browser_photo_measurements_without_resampling
  FTX_CAMERA_WEB_FIXTURES="$WORK/camera-web-fixtures" \
    cargo "$CARGO_TEST" --locked --release -p ftx-wasm --test photo local_images_calibrate_through_shared_acquisition_and_replay_into_photo_measurement
  FTX_EVIDENCE_REPLAY_DIR="$WORK/evidence-replay" \
    cargo "$CARGO_TEST" --locked --release -p ftx-wasm --test page supplemental_photo_records_join_tape_through_ordinary_fusion_and_the_full_round
  FTX_BODY_FIT_PORTABLE_DIR="$WORK/body-fit-portable" \
    cargo "$CARGO_TEST" --locked --release -p ftx-cli --test body_fit_workflow prepared_body_fit_replays_native_acceptance_and_failures_in_the_portable_engine
  FTX_DESIGN_SCREENING_REPLAY="$WORK/design-screening" \
    cargo "$CARGO_TEST" --locked --release -p ftx-cli --test accepted_body_workflow fit_screening_reaches_a_late_feasible_design_with_one_downstream_slot
  FTX_BODY_CHECK_CAPTURE_DIR="$WORK/body-check" \
    cargo "$CARGO_TEST" --locked --release -p ftx-cli --test accepted_body_workflow standalone_body_check_reuses_the_exact_shape_without_a_garment_or_tape_fallback
  FTX_MOTION_REPLAY="$WORK/motion-replay" \
    cargo "$CARGO_TEST" --locked --release -p ftx-cli --test accepted_body_workflow scenario_binaries_replay_accepted_shape_and_pose_with_real_drape_and_heat
  FTX_JOINT_DESIGN_CAPTURE_DIR="$WORK/joint-design" \
    cargo "$CARGO_TEST" --locked --release -p ftx-cli --test accepted_body_workflow independent_joint_polling_reaches_both_commands_and_exports_browser_replay
  FTX_DESIGN_ADAPTIVE_REPLAY="$WORK/design-adaptive" \
    cargo "$CARGO_TEST" --locked --release -p ftx-cli --test accepted_body_workflow adaptive_fit_finds_an_intermediate_design_in_native_and_portable_scenarios
  FTX_CONSUMER_ADAPTIVE_CAPTURE_DIR="$WORK/consumer-adaptive" \
    cargo "$CARGO_TEST" --locked --release -p ftx-cli --test accepted_body_workflow consumer_adaptive_uses_real_scenario_mass_and_preserves_cached_incumbent_on_budget_refusal
  FTX_CONSUMER_ADAPTIVE_CAPTURE_DIR="$WORK/consumer-adaptive" \
    cargo "$CARGO_TEST" --locked --release -p ftx-cli --test accepted_body_workflow independent_case_gates_preserve_signed_heat_tradeoffs_and_exact_native_outputs
  FTX_MOVING_BRIDGES_REPLAY="$WORK/moving-bridges" \
    cargo "$CARGO_TEST" --locked --release -p ftx-cli --test accepted_body_workflow qualified_moving_bridges_reach_real_heat_objectives_native_portable_and_atomic_refusals
  FTX_THREAD_COST_REPLAY="$WORK/thread-cost" \
  FTX_CUT_AND_SEW_REPLAY="$WORK/cut-and-sew" \
    cargo "$CARGO_TEST" --locked --release -p ftx-cli --test material_study_consumers -- --test-threads=1
  FTX_FINITE_KNIFE_REPLAY="$WORK/finite-knife" \
    cargo "$CARGO_TEST" --locked --release -p ftx-cli --test finite_stock_workflow qualified_finite_and_mixed_knife_programs_replay_through_both_binaries
  FTX_KNIFE_PROGRAM_REPLAY="$WORK/knife-program" \
    cargo "$CARGO_TEST" --locked --release -p ftx-cli --test cutting_workflow qualified_knife_paths_replay_through_both_commands_portable_api_and_material_studies
  FTX_ROUTED_FIXTURE_REPLAY="$WORK/routed-fixtures" \
  FTX_ROUTING_REPLAY="$WORK/routing-replay" \
    cargo "$CARGO_TEST" --locked --release -p ftx-export --test sewing_routing -- --test-threads=1
  FTX_ROUTED_FIXTURE_REPLAY="$WORK/routed-fixtures" \
  FTX_ELIGIBLE_ROUTING_REPLAY="$WORK/eligible-routing" \
    cargo "$CARGO_TEST" --locked --release -p ftx-cli --test sewing_production_workflow -- --test-threads=1
  FTX_COUPLED_WATER_ALTERNATIVES_REPLAY="$WORK/coupled-water-alternatives" \
  FTX_DEPTH_PROFILE_WATER_REPLAY="$WORK/depth-profile-water" \
  FTX_FINITE_VAPOR_RESERVOIR_REPLAY="$WORK/finite-vapor-reservoir" \
  FTX_COMPRESSION_MOTION_REPLAY="$WORK/compression-motion" \
  FTX_CYCLIC_COMPRESSION_MOTION_REPLAY="$WORK/cyclic-compression-motion" \
    cargo "$CARGO_TEST" --locked --release -p ftx-cli --test accepted_body_workflow surface_water

  node - "$FTX_BIN" "$FTX_MATERIALS_BIN" "$WORK" "$ROOT" <<'EOF'
const fs = require("node:fs"), path = require("node:path"), zlib = require("node:zlib");
const { spawnSync } = require("node:child_process");
const [ftx, ftxMaterials, work, root] = process.argv.slice(2);

function runFtx(args, allowedCodes = [0], cwd = root) {
  const r = spawnSync(ftx, args, { cwd, encoding: "utf8", maxBuffer: 64 * 1024 * 1024 });
  if (!allowedCodes.includes(r.status)) {
    throw new Error(`ftx ${args.join(" ")} exited ${r.status}: ${r.stdout}\n${r.stderr}`);
  }
  return r.stdout;
}

function runFtxMaterials(args, allowedCodes = [0], cwd = root) {
  const r = spawnSync(ftxMaterials, args, { cwd, encoding: "utf8", maxBuffer: 64 * 1024 * 1024 });
  if (!allowedCodes.includes(r.status)) {
    throw new Error(`ftx-materials ${args.join(" ")} exited ${r.status}: ${r.stdout}\n${r.stderr}`);
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

const nodePkg = require(path.join(work, "node-pkg/ftx_wasm.js"));

const acceptedRaw = fs.readFileSync(path.join(work, "motion-replay/accepted.json"), "utf8").trim();

// 8. Configured leg-posture & settling fixture for FTX_POSE_CAPTURE_DIR
{
  const poseDir = path.join(work, "pose-capture");
  fs.mkdirSync(poseDir, { recursive: true });
  fs.copyFileSync(path.join(work, "consumer-adaptive/model.ftxbody"), path.join(poseDir, "model.ftxbody"));
  fs.copyFileSync(path.join(work, "consumer-adaptive/pattern.json"), path.join(poseDir, "pattern.json"));
  fs.writeFileSync(path.join(poseDir, "accepted.json"), acceptedRaw + "\n");
  fs.writeFileSync(path.join(poseDir, "m.json"), JSON.stringify({ wearer: { stature_mm: 1111.0 } }, null, 2));
  fs.writeFileSync(path.join(poseDir, "study.json"), JSON.stringify({
    schema: "ftx-material-study/1",
    variants: [{ key: "baseline", changes: { schema: "ftx-material-bindings/1" } }],
  }, null, 2));
  fs.writeFileSync(path.join(poseDir, "scenario.json"), JSON.stringify({
    schema: "ftx-drape-scenario/1",
    options: { spacing: 100.0, sew_steps: 3, drape_steps: 3, step: 0.001 },
    fit_spec: {},
    max_mesh_points: 4096,
    max_vertex_frames: 1000000,
    motion: {
      schema: "ftx-drape-motion/1",
      keyframes: [
        { time_s: 0.002, arm_abduction_deg: 35.0, leg_pose: { posture: "seated", fraction: 0.002 } },
        { time_s: 0.004, arm_abduction_deg: 35.0, leg_pose: { posture: "squat", fraction: 0.003 } },
      ],
      max_step_s: 0.001,
      max_trials: 16,
      max_pairs: 100000,
      max_sample_vertices: 10000,
    },
  }, null, 2));
  fs.writeFileSync(
    path.join(poseDir, "body-input.json"),
    `{"schema":"ftx-scenario-body/1","accepted_body_fit":${acceptedRaw},"measurements":{"wearer":{"stature_mm":1111.0}},"arm_abduction_deg":35.0,"legs":"seated","leg_fraction":0.001}\n`,
  );
  fs.writeFileSync(
    path.join(poseDir, "leg-motion-report.json"),
    runFtxMaterials(["scenario-study", "pattern.json", "study.json", "scenario.json", "body-input.json", "model.ftxbody"], [0], poseDir),
  );
  runFtx(["drape", "pattern.json", "--body-fit", "accepted.json", "--measurements", "m.json", "--model", "model.ftxbody", "--legs", "seated", "--leg-fraction", "0.001", "--max-settle-steps", "2", "--out", "leg-fold", "--json"], [0, 2], poseDir);
}

// 9 & 10. Ordinary tape arm-pose & preview-pose study fixtures (when admitted Anny model is present)
if (fs.existsSync(admittedModel) && fs.existsSync(path.join(work, "body-check-tape/measurements.json"))) {
  const tapeArmDir = path.join(work, "tape-arm");
  fs.mkdirSync(tapeArmDir, { recursive: true });
  const basePat = JSON.parse(fs.readFileSync(path.join(root, "tests/fixtures/materials/synthetic-scenario-pattern.json"), "utf8"));
  const wrapRadius = 105.0;
  const wrapWidth = 2 * Math.PI * wrapRadius;
  basePat.pieces[0].vertices.b = [wrapWidth, 0.0];
  basePat.pieces[0].vertices.c = [wrapWidth, 50.0];
  basePat.pieces[0].vertices.d = [0.0, 50.0];
  basePat.pieces[0].placement.radius_mm = wrapRadius;
  basePat.pieces[0].placement.x0 = wrapWidth / 2.0;
  fs.writeFileSync(path.join(tapeArmDir, "pattern.json"), JSON.stringify(basePat, null, 2));
  fs.copyFileSync(path.join(work, "body-check-tape/measurements.json"), path.join(tapeArmDir, "full-measurements.json"));
  fs.writeFileSync(path.join(tapeArmDir, "fit.json"), JSON.stringify({ schema: "fabric-fit/1", points: [] }, null, 2));
  for (const deg of [35, 50]) {
    runFtx(["drape", "pattern.json", "--measurements", "full-measurements.json", "--fit", "fit.json", "--model", admittedModel, "--arm-deg", String(deg), "--out", `full-native-${deg}`, "--json"], [0, 2], tapeArmDir);
  }
  fs.writeFileSync(
    path.join(tapeArmDir, "full-native-0.stdout"),
    runFtx(["drape", "pattern.json", "--measurements", "full-measurements.json", "--fit", "fit.json", "--model", admittedModel, "--arm-deg", "0", "--out", "full-native-0", "--json"], [3], tapeArmDir),
  );

  const previewDir = path.join(work, "preview-study");
  fs.mkdirSync(previewDir, { recursive: true });
  const previewPat = JSON.parse(fs.readFileSync(path.join(work, "design-screening/pattern.json"), "utf8"));
  for (const piece of previewPat.pieces || []) {
    if (piece.placement) piece.placement.radius_mm = 105.0;
  }
  fs.writeFileSync(path.join(previewDir, "pattern.json"), JSON.stringify(previewPat, null, 2));
  const previewStudy = JSON.parse(fs.readFileSync(path.join(work, "design-screening/study.json"), "utf8"));
  if (previewStudy.design_grid?.variables?.[0]) {
    previewStudy.design_grid.variables[0].values_mm = [-10.0, -8.0, 0.0, 10.0];
  }
  fs.writeFileSync(path.join(previewDir, "study.json"), JSON.stringify(previewStudy, null, 2));
  const primaryDoc = JSON.parse(fs.readFileSync(path.join(work, "body-check-tape/measurements.json"), "utf8"));
  delete primaryDoc.wearer.reference;
  delete primaryDoc.predictions;
  primaryDoc.taken_on = "1970-01-01";
  for (const r of primaryDoc.records || []) {
    r.provenance = "MEASURED_TAPE";
    r.method = "synthetic preview primary tape";
    r.timestamp = "1970-01-01";
    r.sigma_mm = Math.max(r.sigma_mm || 2.0, 2.0);
  }
  const primary = JSON.stringify(primaryDoc, null, 2);
  const supplemental = JSON.stringify({
    schema: "fabric-measurements/1",
    taken_on: "1970-01-01",
    wearer: { stature_mm: 920.0, age_months: 30.0 },
    records: [{
      name: "across_back",
      value_mm: 225.0,
      sigma_mm: 4.0,
      provenance: "PHOTO_ESTIMATED",
      method: "synthetic preview supplemental",
      timestamp: "1970-01-01",
      note: "synthetic preview fusion test",
    }],
  });
  const pose = { arm_abduction_deg: 50, legs: "seated", leg_fraction: 0.001 };
  const merged = nodePkg.measurement_file(
    `{"mode":"file","measurements":${primary},"supplemental_measurements":${JSON.stringify([supplemental])},"supplemental_same_wearer":true}`,
    "1970-01-01",
  );
  const bodyInput = `{"schema":"ftx-scenario-body/1","measurements":${merged},${JSON.stringify(pose).slice(1)}`;
  const singleScenario = {
    schema: "ftx-drape-scenario/1",
    options: { spacing: 100.0, sew_steps: 20, drape_steps: 0, step: 0.001, max_settle_steps: 2 },
    fit_spec: {},
    max_mesh_points: 4096,
    max_vertex_frames: 100000,
  };
  const matrixScenario = {
    schema: "ftx-drape-scenario-matrix/1",
    max_evaluations: 8,
    cases: [{ key: "preview-case", scenario: singleScenario }],
  };
  fs.writeFileSync(path.join(previewDir, "primary.json"), primary);
  fs.writeFileSync(path.join(previewDir, "supplemental.json"), supplemental);
  fs.writeFileSync(path.join(previewDir, "pose.json"), JSON.stringify(pose));
  fs.writeFileSync(path.join(previewDir, "body-input.json"), bodyInput);
  fs.writeFileSync(path.join(previewDir, "single.json"), JSON.stringify(singleScenario, null, 2));
  fs.writeFileSync(path.join(previewDir, "matrix.json"), JSON.stringify(matrixScenario, null, 2));
  fs.writeFileSync(
    path.join(previewDir, "manufacturing-native.json"),
    runFtxMaterials(["study-on-body", "pattern.json", "study.json", "body-input.json", admittedModel], [0], previewDir),
  );
  fs.writeFileSync(
    path.join(previewDir, "single-native.json"),
    runFtxMaterials(["scenario-study", "pattern.json", "study.json", "single.json", "body-input.json", admittedModel], [0], previewDir),
  );
  fs.writeFileSync(
    path.join(previewDir, "matrix-native.json"),
    runFtxMaterials(["scenario-study", "pattern.json", "study.json", "matrix.json", "body-input.json", admittedModel], [0], previewDir),
  );
}

// 11. Guided layered vapor & finite body-water fixtures (FTX_GUIDED_VAPOR_BODY & FTX_GUIDED_BODY_WATER_BODY)
{
  const vaporDir = path.join(work, "guided-vapor-body");
  fs.mkdirSync(vaporDir, { recursive: true });
  fs.copyFileSync(path.join(work, "consumer-adaptive/model.ftxbody"), path.join(vaporDir, "model.ftxbody"));
  fs.writeFileSync(
    path.join(vaporDir, "body-input.json"),
    `{"schema":"ftx-scenario-body/1","accepted_body_fit":${acceptedRaw},"arm_abduction_deg":35.0}\n`,
  );

  // Inspect synthetic body OBJ triangles to find a vertical face (uncovered_area == 0) distinct from cap face 2
  runFtx(["body", "--body-fit", path.join(work, "pose-capture/accepted.json"), "--model", "model.ftxbody", "--out", "body-probe", "--json"], [0], vaporDir);
  const objLines = fs.readFileSync(path.join(vaporDir, "body-probe/body.obj"), "utf8").split(/\r?\n/);
  const verts = [], faces = [];
  for (const line of objLines) {
    const parts = line.trim().split(/\s+/);
    if (parts[0] === "v") verts.push([Number(parts[1]), Number(parts[2]), Number(parts[3])]);
    else if (parts[0] === "f") faces.push(parts.slice(1, 4).map(tok => Number(tok.split("/")[0]) - 1));
  }
  let verticalTri = 0;
  for (let i = 0; i < faces.length; i++) {
    const [a, b, c] = faces[i].map(idx => verts[idx]);
    const ux = b[0] - a[0], uy = b[1] - a[1], uz = b[2] - a[2];
    const vx = c[0] - a[0], vy = c[1] - a[1], vz = c[2] - a[2];
    const nx = uy * vz - uz * vy, ny = uz * vx - ux * vz, nz = ux * vy - uy * vx;
    const norm = Math.hypot(nx, ny, nz);
    if (norm > 0 && Math.abs(nz) < 0.1 * norm) { verticalTri = i; break; }
  }
  const triZMm = faces[verticalTri].map(idx => verts[idx][2] * 1000.0);
  const lowZ = Math.min(...triZMm) - 50.0, highZ = Math.max(...triZMm) + 50.0;
  const bodyDoc = JSON.parse(fs.readFileSync(path.join(vaporDir, "body-probe/body.json"), "utf8"));
  const waistZ = (bodyDoc.landmarks_mm?.waist?.[2]) ?? 550.0;

  const pat = JSON.parse(fs.readFileSync(path.join(root, "tests/fixtures/materials/synthetic-scenario-pattern.json"), "utf8"));
  const width = 2 * Math.PI * 180.0;
  pat.pieces[0].vertices.b = [width, 0.0];
  pat.pieces[0].vertices.c = [width, highZ - lowZ];
  pat.pieces[0].vertices.d = [0.0, highZ - lowZ];
  pat.pieces[0].placement.radius_mm = 180.0;
  pat.pieces[0].placement.x0 = width / 2.0;
  pat.pieces[0].placement.y0 = waistZ - lowZ;
  const props = pat.fabric.material.record.properties;
  props.thickness.bounds = [0.00035, 0.00045];
  props.thermal_conductivity = {
    value: 0.05, bounds: [0.04, 0.06], unit: "W/(m*K)", source: "test", evidence: "estimate",
    method: "Synthetic bounded conductivity", conditions: { conditioning: "synthetic dry coupon" },
  };
  props.vapor_resistance = {
    value: 7.0, bounds: [6.0, 8.0], unit: "m2*Pa/W", source: "test", evidence: "estimate",
    method: "Synthetic bounded Ret", conditions: { conditioning: "synthetic dry coupon" },
  };
  delete pat.fabric.material.expected_sha256;
  pat.fabric.material = JSON.parse(nodePkg.material_inspect(JSON.stringify(pat.fabric.material))).selection;
  fs.writeFileSync(path.join(vaporDir, "pattern.json"), JSON.stringify(pat, null, 2));

  const coverageObj = {
    body_triangle_ids: [verticalTri],
    instance_layers: { band: "shell" },
    settings: { max_distance_m: 0.2, min_normal_cosine: 0.1, separation_tolerance_m: 0.00001, interfaces: [], cells_per_layer: 1 },
    conditions: { boundaries: [{ inside_c: 35.0, outside_c: 20.0, inside_resistance_m2_k_w: 0.1, outside_resistance_m2_k_w: 0.2 }] },
  };
  const vaporSurfaceObj = {
    schema: "ftx-vapor-surface-conditions/1",
    boundaries: [{ inside_vapor_pressure_pa: 2000.0, outside_vapor_pressure_pa: 1000.0, inside_film_resistance_m2_pa_w: 1.0, outside_film_resistance_m2_pa_w: 2.0, latent_heat_j_kg: 2500000.0 }],
  };
  const vaporCoverageObj = {
    body_triangle_ids: [verticalTri],
    instance_layers: { band: "shell" },
    settings: { max_distance_m: 0.2, min_normal_cosine: 0.1, separation_tolerance_m: 0.00001, interfaces: [] },
    conditions: { boundaries: [{ inside_vapor_pressure_pa: 2000.0, outside_vapor_pressure_pa: 1000.0, inside_film_resistance_m2_pa_w: 1.0, outside_film_resistance_m2_pa_w: 2.0, latent_heat_j_kg: 2500000.0 }] },
  };
  const motionObj = {
    schema: "ftx-drape-motion/1",
    keyframes: [{ time_s: 0.002, arm_abduction_deg: 35.1 }, { time_s: 0.004, arm_abduction_deg: 35.2 }],
    max_step_s: 0.001,
    max_trials: 16,
    max_pairs: 100000,
    max_sample_vertices: 10000,
    vapor_coverage_history: { schema: "ftx-motion-vapor-coverage/1", integration: "composite_trapezoid", max_geometry_operations: 1000000 },
  };
  const scenarioObj = {
    schema: "ftx-drape-scenario/1",
    options: { spacing: 100.0, sew_steps: 120, drape_steps: 0, step: 0.04, max_settle_steps: 0 },
    fit_spec: {},
    max_mesh_points: 4096,
    max_vertex_frames: 100000,
    surface: {
      schema: "ftx-steady-surface-conditions/1",
      boundaries: [{ inside_c: 35.0, outside_c: 20.0, inside_resistance_m2_k_w: 0.1, outside_resistance_m2_k_w: 0.2 }],
    },
    vapor_coverage: vaporCoverageObj,
  };
  fs.writeFileSync(path.join(vaporDir, "coverage.json"), JSON.stringify(coverageObj));
  fs.writeFileSync(path.join(vaporDir, "vapor-surface.json"), JSON.stringify(vaporSurfaceObj));
  fs.writeFileSync(path.join(vaporDir, "vapor-coverage.json"), JSON.stringify(vaporCoverageObj));
  fs.writeFileSync(path.join(vaporDir, "motion.json"), JSON.stringify(motionObj));
  fs.writeFileSync(path.join(vaporDir, "scenario.json"), JSON.stringify(scenarioObj, null, 2));

  // Probe once to read exact body_mesh_sha256 for wet_film
  fs.writeFileSync(path.join(vaporDir, "probe-study.json"), JSON.stringify({
    schema: "ftx-material-study/1",
    variants: [{ key: "baseline", changes: { schema: "ftx-material-bindings/1" } }],
  }));
  const probeOut = JSON.parse(runFtxMaterials(["scenario-study", "pattern.json", "probe-study.json", "scenario.json", "body-input.json", "model.ftxbody"], [0], vaporDir));
  const bodyMeshSha = probeOut.simulations[0].evaluation.result.vapor_coverage.result.body_mesh_sha256;
  const bodyWaterMotion = structuredClone(motionObj);
  bodyWaterMotion.vapor_coverage_history.wet_film = {
    schema: "ftx-body-wet-film/1",
    body_mesh_sha256: bodyMeshSha,
    source: "test:explicit-body-water",
    basis: "Synthetic initial film and constant body supply; no inferred wearer physiology",
    initial_liquid_mass_kg: [1e-12],
    supply_rate_kg_s: [1e-10],
  };
  fs.writeFileSync(path.join(vaporDir, "body-water-motion.json"), JSON.stringify(bodyWaterMotion));
}

// 12. Guided textile-water fixture (FTX_GUIDED_TEXTILE_WATER_BODY)
{
  const twDir = path.join(work, "guided-textile-water");
  fs.mkdirSync(twDir, { recursive: true });
  fs.copyFileSync(path.join(work, "consumer-adaptive/model.ftxbody"), path.join(twDir, "model.ftxbody"));
  fs.writeFileSync(
    path.join(twDir, "body-input.json"),
    `{"schema":"ftx-scenario-body/1","accepted_body_fit":${acceptedRaw},"arm_abduction_deg":35.0}\n`,
  );

  const pat = JSON.parse(fs.readFileSync(path.join(root, "tests/fixtures/materials/synthetic-scenario-pattern.json"), "utf8"));
  const width = 2 * Math.PI * 180.0;
  pat.pieces[0].vertices.b[0] = width;
  pat.pieces[0].vertices.c[0] = width;
  pat.pieces[0].placement.radius_mm = 180.0;
  pat.pieces[0].placement.x0 = width / 2.0;
  const mat = pat.fabric.material;
  mat.record.properties.specific_heat = { value: 1000.0, unit: "J/(kg*K)", source: "test", evidence: "estimate", method: "Synthetic textile water" };
  mat.record.properties.vapor_resistance = { value: 8.0, unit: "m2*Pa/W", source: "test", evidence: "estimate", method: "Synthetic textile water" };
  delete mat.expected_sha256;
  const inspectedBase = JSON.parse(nodePkg.material_inspect(JSON.stringify(mat)));
  pat.fabric.material = inspectedBase.selection;
  const lockSha = inspectedBase.selection.expected_sha256;
  fs.writeFileSync(path.join(twDir, "pattern.json"), JSON.stringify(pat, null, 2));

  const incomp = structuredClone(mat);
  incomp.id = incomp.record.id = "synthetic-incompatible-textile-water";
  incomp.record.properties.vapor_resistance.value = 16.0;
  const incompSel = JSON.parse(nodePkg.material_inspect(JSON.stringify(incomp))).selection;
  fs.writeFileSync(path.join(twDir, "study.json"), JSON.stringify({
    schema: "ftx-material-study/1",
    variants: [
      { key: "baseline", changes: { schema: "ftx-material-bindings/1" } },
      { key: "incompatible-material", changes: { schema: "ftx-material-bindings/1", default: incompSel } },
    ],
  }, null, 2));

  const makeWaterMotion = (keyframes, outsidePa, supplyRates, drainRates, vaporTemps) => ({
    schema: "ftx-drape-motion/1",
    keyframes,
    max_step_s: 0.001,
    max_trials: 8,
    max_pairs: 100000,
    max_sample_vertices: 100000,
    transient_surface: {
      schema: "ftx-motion-surface-heat/1",
      initial_temperature_c: 20.0,
      cells_per_layer: 1,
      max_step_s: 0.00025,
      max_cell_steps: 1000000,
      boundaries: keyframes.map(() => ({ inside_c: 20.0, outside_c: 20.0, inside_resistance_m2_k_w: 1.0, outside_resistance_m2_k_w: 1.0 })),
      surface_water: {
        schema: "ftx-textile-surface-water/4",
        model: "inner_cell_equilibrium_free_film_frozen_properties/1",
        bindings: { band: { material_sha256: lockSha, initial_liquid_kg_m2: 0.001 } },
        maximum_liquid_loading_kg_m2: 0.01,
        liquid_specific_heat_j_kg_k: 4200.0,
        latent_heat_j_kg: 2500000.0,
        enthalpy_reference_c: 0.0,
        wet_vapor_pressure_pa: [[-20.0, 0.0], [80.0, 10000.0]],
        outside_vapor_pressure_pa: outsidePa,
        inside_film_resistance_m2_pa_w: 1.0,
        outside_film_resistance_m2_pa_w: 1.0,
        supply: {
          schema: "ftx-textile-liquid-supply/1",
          bindings: { band: { rate_kg_m2_s: supplyRates, temperature_c: keyframes.map(() => 20.0) } },
          source: { reference: "test:guided-textile-supply", accessed_on: "2026-11-03", note: "Synthetic supply" },
          evidence: "estimate",
          allow_estimates: true,
          method: "Synthetic prescribed supply",
        },
        drainage: {
          schema: "ftx-textile-liquid-drainage/1",
          bindings: { band: { rate_kg_m2_s: drainRates } },
          source: { reference: "test:guided-textile-drainage", accessed_on: "2026-11-03", note: "Synthetic drainage" },
          evidence: "estimate",
          allow_estimates: true,
          method: "Synthetic prescribed drainage",
          qualification: "Stock-limited removal at retained liquid temperature",
        },
        condensation: {
          schema: "ftx-textile-condensation/1",
          vapor_temperature_c: vaporTemps,
          source: { reference: "test:guided-textile-condensation", accessed_on: "2026-11-03", note: "Synthetic condensation" },
          evidence: "estimate",
          allow_estimates: true,
          method: "Synthetic reverse vapor closure",
          qualification: "Reverse Ret network over [-20,80] C and [0,0.01] kg/m2",
        },
        source: { reference: "test:guided-textile-water", accessed_on: "2026-11-03", note: "Synthetic fluid evidence only" },
        evidence: "estimate",
        allow_estimates: true,
        method: "Synthetic decision replay",
        qualification: "Synthetic fixed dry thermal and vapor properties across [-20,80] C and [0,0.01] kg/m2",
      },
    },
  });

  const kfs3 = [
    { time_s: 0.002, arm_abduction_deg: 35.1 },
    { time_s: 0.003, arm_abduction_deg: 35.2 },
    { time_s: 0.004, arm_abduction_deg: 35.3 },
  ];
  const fullMotion = makeWaterMotion(kfs3, [6000.0, 1000.0, 1000.0], [0.02, 0.02, 0.02], [0.01, 0.01, 0.01], [25.0, 20.0, 20.0]);
  const prefixMotion = makeWaterMotion([kfs3[0]], [6000.0], [0.02], [0.01], [25.0]);
  const lateMotion = makeWaterMotion(kfs3, [6000.0, 1000.0, 1000.0], [0.02, 0.02, 500.0], [0.01, 0.01, 0.01], [25.0, 20.0, 20.0]);

  const baseScenario = {
    schema: "ftx-drape-scenario/1",
    options: { spacing: 100.0, sew_steps: 120, drape_steps: 0, step: 0.04, max_settle_steps: 0 },
    fit_spec: {},
    max_mesh_points: 4096,
    max_vertex_frames: 100000,
    surface: {
      schema: "ftx-steady-surface-conditions/1",
      boundaries: [{ inside_c: 35.0, outside_c: 20.0, inside_resistance_m2_k_w: 0.1, outside_resistance_m2_k_w: 0.2 }],
    },
    vapor_surface: {
      schema: "ftx-vapor-surface-conditions/1",
      boundaries: [{ inside_vapor_pressure_pa: 2000.0, outside_vapor_pressure_pa: 1000.0, inside_film_resistance_m2_pa_w: 1.0, outside_film_resistance_m2_pa_w: 2.0, latent_heat_j_kg: 2500000.0 }],
    },
  };
  fs.writeFileSync(path.join(twDir, "motion.json"), JSON.stringify(fullMotion));
  fs.writeFileSync(path.join(twDir, "scenario.json"), JSON.stringify({ ...baseScenario, motion: fullMotion }, null, 2));
  fs.writeFileSync(path.join(twDir, "prefix-scenario.json"), JSON.stringify({ ...baseScenario, motion: prefixMotion }, null, 2));
  fs.writeFileSync(path.join(twDir, "late-scenario.json"), JSON.stringify({ ...baseScenario, motion: lateMotion }, null, 2));
}
EOF
fi

echo "[check_all_web] 4/5 Running all 9 Node VM suites (FTX_REAL_WASM=1) + 3 direct *_wasm.cjs suites..."
# check_trace_frontend.cjs requires 7 sibling VM suites (8 total) + check_material_screen_builder.cjs = all 9 VM suites.
if [ "$WITH_REPLAYS" -eq 1 ]; then
  ADMITTED_MODEL="${FTX_BODY_MODEL:-${HOME:-}/.cache/franken_textiles/anny-0.6.1-default.ftxbody}"
  BODY_CHECK_ENV_DIR=""
  BODY_CHECK_ENV_MODEL=""
  TAPE_ARM_ENV_DIR=""
  PREVIEW_STUDY_ENV_DIR=""
  if [ -f "$ADMITTED_MODEL" ] && [ -d "$WORK/body-check-tape" ]; then
    BODY_CHECK_ENV_DIR="$WORK/body-check-tape"
    BODY_CHECK_ENV_MODEL="$ADMITTED_MODEL"
    TAPE_ARM_ENV_DIR="$WORK/tape-arm"
    PREVIEW_STUDY_ENV_DIR="$WORK/preview-study"
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
  FTX_GUIDED_VAPOR_BODY="$WORK/guided-vapor-body" \
  FTX_GUIDED_BODY_WATER_BODY="$WORK/guided-vapor-body" \
  FTX_GUIDED_TEXTILE_WATER_BODY="$WORK/guided-textile-water" \
  FTX_POSE_CAPTURE_DIR="$WORK/pose-capture" \
  FTX_TAPE_ARM_DIR="$TAPE_ARM_ENV_DIR" \
  FTX_PREVIEW_STUDY_DIR="$PREVIEW_STUDY_ENV_DIR" \
  FTX_ANNY_MODEL="$BODY_CHECK_ENV_MODEL" \
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
  FTX_COMPRESSION_MOTION_REPLAY="$WORK/compression-motion" \
  FTX_CYCLIC_COMPRESSION_MOTION_REPLAY="$WORK/cyclic-compression-motion" \
    node --experimental-vm-modules --test tools/check_trace_frontend.cjs tools/check_material_screen_builder.cjs
else
  FTX_REAL_WASM=1 FTX_GUIDED_SCENARIO_NATIVE="$FTX_MATERIALS_BIN" node --experimental-vm-modules --test tools/check_trace_frontend.cjs tools/check_material_screen_builder.cjs
fi
node --experimental-vm-modules tools/check_materials_wasm.cjs --native "$FTX_MATERIALS_BIN"
node --experimental-vm-modules tools/check_strip_wasm.cjs --native "$FTX_MATERIALS_BIN"
node tools/check_sewing_wasm.cjs "$WORK/node-pkg/ftx_wasm.js" "$FTX_MATERIALS_BIN"

if [ "$VM_ONLY" -eq 1 ]; then
  echo "[check_all_web] PASS: VM + direct WASM suites passed (--vm-only, with_replays=$WITH_REPLAYS, work dir: $WORK)"
  exit 0
fi

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
node tools/check_photo_web.cjs --body-photo "$WORK/capture-setup"

echo "[check_all_web] PASS: all 19 frontend/WASM/Playwright suites passed (with_replays=$WITH_REPLAYS, work dir: $WORK)"
