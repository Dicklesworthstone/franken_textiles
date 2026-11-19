#!/bin/sh
# Package the existing local workbenches with the native project's pinned WASM bindings.
set -eu
cd "$(dirname "$0")/../.."
artifact="${FTX_WASM_ARTIFACT:-${CARGO_TARGET_DIR:-target}/wasm32-unknown-unknown/release/ftx_wasm.wasm}"
if [ -z "${FTX_WASM_ARTIFACT:-}" ]; then
  cargo build --release --locked --target wasm32-unknown-unknown -p ftx-wasm
fi
dest=showcase/public/workbench
mkdir -p "$dest/pkg" "$dest/calibration"
cp web/*.js web/*.html "$dest/"
cp web/calibration/* "$dest/calibration/"
wasm-bindgen --target web --no-typescript --remove-name-section --remove-producers-section \
  --out-dir "$dest/pkg" "$artifact"
gzip -6n -c "$dest/pkg/ftx_wasm_bg.wasm" > "$dest/pkg/ftx_wasm_bg.wasm.gz"
(cd showcase && bun scripts/workbench-share.tsx)
