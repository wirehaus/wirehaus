#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
cargo build --manifest-path ../Cargo.toml -p wirehaus-crypto-wasm --target wasm32-unknown-unknown --release --locked
wasm-bindgen ../target/wasm32-unknown-unknown/release/wirehaus_crypto_wasm.wasm \
  --target web --out-dir src --out-name wirehaus_crypto_wasm --typescript
