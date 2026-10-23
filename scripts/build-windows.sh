#!/usr/bin/env bash
set -euo pipefail

target=x86_64-pc-windows-gnu
export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc
export CC_x86_64_pc_windows_gnu=x86_64-w64-mingw32-gcc
export CXX_x86_64_pc_windows_gnu=x86_64-w64-mingw32-g++
export AR_x86_64_pc_windows_gnu=x86_64-w64-mingw32-ar
export CXXSTDLIB_x86_64_pc_windows_gnu=static=stdc++
gcc_dir=$(dirname "$(x86_64-w64-mingw32-g++ -print-file-name=libstdc++.a)")
export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_RUSTFLAGS="-L native=$gcc_dir -C target-feature=+crt-static -C link-arg=-static -C link-arg=-static-libstdc++ -C link-arg=-static-libgcc"

cd "$(dirname "$0")/.."
cargo build --release --locked --target "$target" -p wirehaus-agent "$@"
echo "target/$target/release/wirehaus-agent.exe"
