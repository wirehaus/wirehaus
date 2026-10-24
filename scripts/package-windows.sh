#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
target=${WIREHAUS_TARGET:-x86_64-pc-windows-gnu}
python=$(command -v python3 || command -v python)
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
release="target/$target/release"
name="wirehaus-$version-windows-x64"
out="target/dist/$name"

if [ "$target" = x86_64-pc-windows-gnu ]; then
  scripts/build-windows.sh >/dev/null
else
  cargo build --release --locked --target "$target" -p wirehaus-agent
fi
rm -rf "$out" "$out.zip"
mkdir -p "$out"
cp "$release/wirehaus-agent.exe" "$release/wirehaus-agentd.exe" scripts/install.ps1 LICENSE "$out/"
cp -r viewer/dist "$out/viewer"
(cd "$out" && find . -type f ! -name SHA256SUMS ! -name install.ps1 | sed 's|^\./||' | sort | xargs sha256sum > SHA256SUMS)
"$python" - "$out" "$out.zip" <<'PY'
import os, sys, zipfile
source, archive = sys.argv[1], sys.argv[2]
with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED) as bundle:
    for root, _, names in os.walk(source):
        for name in sorted(names):
            path = os.path.join(root, name)
            bundle.write(path, os.path.join(os.path.basename(source), os.path.relpath(path, source)))
PY
(cd target/dist && sha256sum "$name.zip" | tee "$name.zip.sha256")
