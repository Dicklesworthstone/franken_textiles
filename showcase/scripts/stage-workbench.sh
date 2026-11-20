#!/bin/sh
# Package the existing local workbenches with the native project's pinned WASM bindings.
set -eu
cd "$(dirname "$0")/../.."
artifact="${FTX_WASM_ARTIFACT:-${CARGO_TARGET_DIR:-target}/wasm32-unknown-unknown/wasm-release/ftx_wasm.wasm}"
if [ -z "${FTX_WASM_ARTIFACT:-}" ]; then
  cargo build --profile wasm-release --locked --target wasm32-unknown-unknown -p ftx-wasm
fi
dest=showcase/public/workbench
mkdir -p "$dest/pkg" "$dest/calibration" "$dest/atelier-assets"
cp web/*.js web/*.html web/*.css "$dest/"
cp web/atelier-assets/* "$dest/atelier-assets/"
cp web/calibration/* "$dest/calibration/"
wasm-bindgen --target web --no-typescript --remove-name-section --remove-producers-section \
  --out-dir "$dest/pkg" "$artifact"
gzip -6n -c "$dest/pkg/ftx_wasm_bg.wasm" > "$dest/pkg/ftx_wasm_bg.wasm.gz"
(cd showcase && bun scripts/workbench-share.tsx)
