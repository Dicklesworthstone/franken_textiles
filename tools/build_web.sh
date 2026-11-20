#!/bin/sh
# Builds web/pkg, the page's WebAssembly module and its wasm-bindgen glue, from crates/ftx-wasm.
# Needs the wasm32-unknown-unknown target (rust-toolchain.toml) and the wasm-bindgen CLI at the version
# Cargo.toml pins (cargo install wasm-bindgen-cli --version 0.2.128).
set -eu
cd "$(dirname "$0")/.."
if cargo --list 2>/dev/null | grep -qw lbuild; then
  cargo lbuild --locked --profile wasm-release --target wasm32-unknown-unknown -p ftx-wasm
else
  cargo build --locked --profile wasm-release --target wasm32-unknown-unknown -p ftx-wasm
fi
wasm-bindgen --target web --remove-name-section --remove-producers-section --out-dir web/pkg "${CARGO_TARGET_DIR:-target}/wasm32-unknown-unknown/wasm-release/ftx_wasm.wasm"
echo "built web/pkg; serve the page with: python3 -m http.server -d web 8000  (then open http://localhost:8000/)"
