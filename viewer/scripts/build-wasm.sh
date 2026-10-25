#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
export RUSTFLAGS="${RUSTFLAGS:-} --remap-path-prefix=$(cd .. && pwd)=/wirehaus --remap-path-prefix=${CARGO_HOME:-$HOME/.cargo}=/cargo --remap-path-prefix=$(rustc --print sysroot)=/rustc"
cargo build --manifest-path ../Cargo.toml -p wirehaus-crypto-wasm --target wasm32-unknown-unknown --release --locked
wasm-bindgen "${CARGO_TARGET_DIR:-../target}/wasm32-unknown-unknown/release/wirehaus_crypto_wasm.wasm" \
  --target web --out-dir src --out-name wirehaus_crypto_wasm --typescript
