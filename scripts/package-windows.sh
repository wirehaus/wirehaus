#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
version=$(cargo metadata --no-deps --format-version 1 | python3 -c 'import sys,json; print(next(p["version"] for p in json.load(sys.stdin)["packages"] if p["name"]=="wirehaus-agent"))')
target=target/x86_64-pc-windows-gnu/release
name="wirehaus-$version-windows-x64"
out="target/dist/$name"

scripts/build-windows.sh >/dev/null
rm -rf "$out" "$out.zip"
mkdir -p "$out"
cp "$target/wirehaus-agent.exe" "$target/wirehaus-agentd.exe" scripts/install.ps1 LICENSE "$out/"
cp -r viewer/dist "$out/viewer"
(cd "$out" && find . -type f ! -name SHA256SUMS ! -name install.ps1 | sed 's|^\./||' | sort | xargs sha256sum > SHA256SUMS)
python3 - "$out" "$out.zip" <<'PY'
import os, sys, zipfile
source, archive = sys.argv[1], sys.argv[2]
with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED) as bundle:
    for root, _, names in os.walk(source):
        for name in sorted(names):
            path = os.path.join(root, name)
            bundle.write(path, os.path.join(os.path.basename(source), os.path.relpath(path, source)))
PY
(cd target/dist && sha256sum "$name.zip")
