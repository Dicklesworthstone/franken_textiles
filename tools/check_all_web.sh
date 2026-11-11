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
FTX_BODY_START_DIR="$WORK/body-starts" cargo test --locked -p ftx-cli --test body_fit_workflow start_edits_editor_fixture_replays_the_complete_capture_through_the_fitter
FTX_BODY_TARGET_DIR="$WORK/body-targets" cargo test --locked -p ftx-cli --test body_fit_workflow targets_editor_fixture_replays_a_corrected_capture_through_the_ordinary_fitter
FTX_CAPTURE_EDITOR_DIR="$WORK/capture-editor" cargo test --locked -p ftx-cli --test body_fit_workflow capture_editor_fixture_rejects_bad_check_and_keeps_a_positive_native_reference
FTX_CAPTURE_SETUP_DIR="$WORK/capture-setup" cargo test --locked -p ftx-cli --test body_fit_workflow capture_setup_constructs_the_first_request_and_reaches_the_ordinary_fitter

if [ "$WITH_REPLAYS" -eq 1 ]; then
  echo "[check_all_web] 3b/5 Populating opt-in native->browser replay directories (--with-replays)..."
  FTX_PHOTO_WEB_FIXTURES="$WORK/photo-web-fixtures" \
    cargo test --locked -p ftx-wasm --test photo retained_camera_uncertainty_reaches_browser_photo_measurements_without_resampling
  FTX_CAMERA_WEB_FIXTURES="$WORK/camera-web-fixtures" \
    cargo test --locked -p ftx-wasm --test photo local_images_calibrate_through_shared_acquisition_and_replay_into_photo_measurement
  FTX_EVIDENCE_REPLAY_DIR="$WORK/evidence-replay" \
    cargo test --locked -p ftx-wasm --test page supplemental_photo_records_join_tape_through_ordinary_fusion_and_the_full_round
  FTX_BODY_FIT_PORTABLE_DIR="$WORK/body-fit-portable" \
    cargo test --locked -p ftx-cli --test body_fit_workflow prepared_body_fit_replays_native_acceptance_and_failures_in_the_portable_engine
  FTX_DESIGN_SCREENING_REPLAY="$WORK/design-screening" \
    cargo test --locked -p ftx-cli --test accepted_body_workflow fit_screening_reaches_a_late_feasible_design_with_one_downstream_slot
  FTX_BODY_CHECK_CAPTURE_DIR="$WORK/body-check" \
    cargo test --locked -p ftx-cli --test accepted_body_workflow standalone_body_check_reuses_the_exact_shape_without_a_garment_or_tape_fallback
  FTX_MOTION_REPLAY="$WORK/motion-replay" \
    cargo test --locked -p ftx-cli --test accepted_body_workflow scenario_binaries_replay_accepted_shape_and_pose_with_real_drape_and_heat
  FTX_JOINT_DESIGN_CAPTURE_DIR="$WORK/joint-design" \
    cargo test --locked -p ftx-cli --test accepted_body_workflow independent_joint_polling_reaches_both_commands_and_exports_browser_replay
  FTX_DESIGN_ADAPTIVE_REPLAY="$WORK/design-adaptive" \
    cargo test --locked -p ftx-cli --test accepted_body_workflow adaptive_fit_finds_an_intermediate_design_in_native_and_portable_scenarios
  FTX_CONSUMER_ADAPTIVE_CAPTURE_DIR="$WORK/consumer-adaptive" \
    cargo test --locked -p ftx-cli --test accepted_body_workflow consumer_adaptive_uses_real_scenario_mass_and_preserves_cached_incumbent_on_budget_refusal
  FTX_CONSUMER_ADAPTIVE_CAPTURE_DIR="$WORK/consumer-adaptive" \
    cargo test --locked -p ftx-cli --test accepted_body_workflow independent_case_gates_preserve_signed_heat_tradeoffs_and_exact_native_outputs
  FTX_MOVING_BRIDGES_REPLAY="$WORK/moving-bridges" \
    cargo test --locked -p ftx-cli --test accepted_body_workflow qualified_moving_bridges_reach_real_heat_objectives_native_portable_and_atomic_refusals
  FTX_THREAD_COST_REPLAY="$WORK/thread-cost" \
  FTX_CUT_AND_SEW_REPLAY="$WORK/cut-and-sew" \
    cargo test --locked -p ftx-cli --test material_study_consumers -- --test-threads=1
  FTX_FINITE_KNIFE_REPLAY="$WORK/finite-knife" \
    cargo test --locked -p ftx-cli --test finite_stock_workflow qualified_finite_and_mixed_knife_programs_replay_through_both_binaries
  FTX_KNIFE_PROGRAM_REPLAY="$WORK/knife-program" \
    cargo test --locked -p ftx-cli --test cutting_workflow qualified_knife_paths_replay_through_both_commands_portable_api_and_material_studies
  FTX_ROUTED_FIXTURE_REPLAY="$WORK/routed-fixtures" \
    cargo test --locked -p ftx-export --test sewing_routing -- --test-threads=1
  FTX_ROUTED_FIXTURE_REPLAY="$WORK/routed-fixtures" \
    cargo test --locked -p ftx-cli --test sewing_production_workflow -- --test-threads=1
fi

echo "[check_all_web] 4/5 Running all 9 Node VM suites (FTX_REAL_WASM=1) + 3 direct *_wasm.cjs suites..."
# check_trace_frontend.cjs requires 7 sibling VM suites (8 total) + check_material_screen_builder.cjs = all 9 VM suites.
if [ "$WITH_REPLAYS" -eq 1 ]; then
  FTX_REAL_WASM=1 \
  FTX_GUIDED_SCENARIO_NATIVE="$FTX_MATERIALS_BIN" \
  FTX_PHOTO_WEB_FIXTURES="$WORK/photo-web-fixtures" \
  FTX_CAMERA_WEB_FIXTURES="$WORK/camera-web-fixtures" \
  FTX_BODY_FIT_PORTABLE_DIR="$WORK/body-fit-portable" \
  FTX_CONSUMER_ADAPTIVE_REPLAY="$WORK/consumer-adaptive" \
  FTX_BODY_CHECK_DIR="$WORK/body-check" \
  FTX_DESIGN_SCREENING_REPLAY="$WORK/design-screening" \
  FTX_MOTION_REPLAY="$WORK/motion-replay" \
  FTX_JOINT_DESIGN_CAPTURE_DIR="$WORK/joint-design" \
  FTX_DESIGN_ADAPTIVE_REPLAY="$WORK/design-adaptive" \
  FTX_MOVING_BRIDGES_REPLAY="$WORK/moving-bridges" \
  FTX_THREAD_COST_REPLAY="$WORK/thread-cost" \
  FTX_CUT_AND_SEW_REPLAY="$WORK/cut-and-sew" \
  FTX_FINITE_KNIFE_REPLAY="$WORK/finite-knife" \
  FTX_KNIFE_PROGRAM_REPLAY="$WORK/knife-program" \
  FTX_ROUTED_FIXTURE_REPLAY="$WORK/routed-fixtures" \
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
else
  node tools/check_web.cjs --ftx "$FTX_BIN"
fi
node tools/check_materials_web.cjs --native "$FTX_MATERIALS_BIN"
node tools/check_photo_web.cjs --section
node tools/check_body_starts.cjs "$WORK/body-starts" "$WORK/body-starts-out" "$FTX_BIN"
node tools/check_body_targets.cjs "$WORK/body-targets" "$WORK/body-targets-out"
node tools/check_capture_review.cjs "$WORK/capture-editor" "$WORK/capture-editor-out"
node tools/check_capture_setup.cjs "$WORK/capture-setup" "$WORK/capture-setup-out" "$FTX_BIN"

echo "[check_all_web] PASS: all 19 frontend/WASM/Playwright suites passed (with_replays=$WITH_REPLAYS, work dir: $WORK)"
