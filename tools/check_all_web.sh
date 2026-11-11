#!/bin/sh
# Reproducible runner for web/pkg and all 19 tools/check_*.cjs frontend, direct-WASM, and
# headless Chromium Playwright E2E suites (ftx-onn.29 / ftx-onn.31).
#
# Respects CARGO_TARGET_DIR, TMPDIR, FTX_PLAYWRIGHT_MODULE, CHROMIUM_EXECUTABLE, and NODE_PATH.
set -eu
cd "$(dirname "$0")/.." || exit 1
ROOT="$(pwd)"

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

echo "[check_all_web] 4/5 Running all 9 Node VM suites (FTX_REAL_WASM=1) + 3 direct *_wasm.cjs suites..."
# check_trace_frontend.cjs requires 7 sibling VM suites (8 total) + check_material_screen_builder.cjs = all 9 VM suites.
FTX_REAL_WASM=1 FTX_GUIDED_SCENARIO_NATIVE="$FTX_MATERIALS_BIN" node --experimental-vm-modules --test tools/check_trace_frontend.cjs tools/check_material_screen_builder.cjs
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
node tools/check_web.cjs --ftx "$FTX_BIN"
node tools/check_materials_web.cjs --native "$FTX_MATERIALS_BIN"
node tools/check_photo_web.cjs --section
node tools/check_body_starts.cjs "$WORK/body-starts" "$WORK/body-starts-out" "$FTX_BIN"
node tools/check_body_targets.cjs "$WORK/body-targets" "$WORK/body-targets-out"
node tools/check_capture_review.cjs "$WORK/capture-editor" "$WORK/capture-editor-out"
node tools/check_capture_setup.cjs "$WORK/capture-setup" "$WORK/capture-setup-out" "$FTX_BIN"

echo "[check_all_web] PASS: all 19 frontend/WASM/Playwright suites passed (work dir: $WORK)"
