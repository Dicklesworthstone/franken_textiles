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
# The posing room's threaded engine (ftx-wasm `threads`, public/labs/studio-engine-threads.mjs): std rebuilt
# with atomics into shared memory (1 GiB at most), in its own target directory. It only runs where the page
# is cross-origin isolated (_headers); everywhere else the page uses pkg/.
threads="${FTX_WASM_THREADS_ARTIFACT:-${CARGO_TARGET_DIR:-target}/threads/wasm32-unknown-unknown/wasm-release/ftx_wasm.wasm}"
if [ -z "${FTX_WASM_THREADS_ARTIFACT:-}" ]; then
  RUSTFLAGS="-C target-feature=+atomics,+bulk-memory -C link-arg=--shared-memory -C link-arg=--import-memory \
-C link-arg=--max-memory=1073741824 -C link-arg=--export=__wasm_init_tls -C link-arg=--export=__tls_size \
-C link-arg=--export=__tls_align -C link-arg=--export=__tls_base" \
    cargo build -Z build-std=panic_abort,std --profile wasm-release --locked --target wasm32-unknown-unknown \
    -p ftx-wasm --features threads --target-dir "${CARGO_TARGET_DIR:-target}/threads"
fi
wasm-bindgen --target web --no-typescript --remove-name-section --remove-producers-section \
  --out-dir "$dest/pkg-threads" "$threads"
gzip -6n -c "$dest/pkg-threads/ftx_wasm_bg.wasm" > "$dest/pkg-threads/ftx_wasm_bg.wasm.gz"
(cd showcase && bun scripts/workbench-share.tsx)
