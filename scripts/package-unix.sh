#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
sum() {
  if command -v sha256sum >/dev/null 2>&1; then sha256sum "$@"; else shasum -a 256 "$@"; fi
}
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
system=$(uname -s | tr '[:upper:]' '[:lower:]')
machine=$(uname -m)
name="wirehaus-$version-$system-$machine"
out="target/dist/$name"

cargo build --release --locked -p wirehaus-agent
rm -rf "$out" "$out.tar.gz"
mkdir -p "$out"
cp target/release/wirehaus-agent scripts/install.sh LICENSE "$out/"
cp -r viewer/dist "$out/viewer"
(cd "$out" && find . -type f ! -name SHA256SUMS ! -name install.sh | sed 's|^\./||' | LC_ALL=C sort | xargs sh -c 'if command -v sha256sum >/dev/null 2>&1; then sha256sum "$@"; else shasum -a 256 "$@"; fi' _ > SHA256SUMS)
tar -C target/dist -czf "$out.tar.gz" "$name"
(cd target/dist && sum "$name.tar.gz" | tee "$name.tar.gz.sha256")
