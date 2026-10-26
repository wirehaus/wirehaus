#!/bin/sh
set -eu

source_dir=$(cd "$(dirname "$0")" && pwd)
install_dir=${WIREHAUS_INSTALL_DIR:-"$HOME/.local/share/wirehaus"}
agent="$install_dir/wirehaus-agent"
mode=install
purge=no

for argument in "$@"; do
  case "$argument" in
    --uninstall) mode=uninstall ;;
    --purge) purge=yes ;;
    *) echo "unknown option: $argument" >&2; exit 2 ;;
  esac
done

if [ -n "${WIREHAUS_STATE_DIR:-}" ]; then
  data_dir=$WIREHAUS_STATE_DIR
elif [ "$(uname -s)" = Darwin ]; then
  data_dir="$HOME/Library/Application Support/Wirehaus"
else
  data_dir=${XDG_DATA_HOME:-"$HOME/.local/share"}/wirehaus-state
fi

checksum() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | cut -d ' ' -f 1
  else
    shasum -a 256 "$1" | cut -d ' ' -f 1
  fi
}

verify_package() {
  sums="$source_dir/SHA256SUMS"
  [ -f "$sums" ] || { echo "SHA256SUMS is missing from the package." >&2; exit 1; }
  while read -r hash name; do
    [ -f "$source_dir/$name" ] || { echo "$name is listed in SHA256SUMS but missing." >&2; exit 1; }
    [ "$(checksum "$source_dir/$name")" = "$hash" ] || { echo "$name does not match SHA256SUMS." >&2; exit 1; }
  done < "$sums"
}

run_agent() {
  "$agent" --state-dir "$data_dir" "$@"
}

if [ "$mode" = uninstall ]; then
  if [ -x "$agent" ]; then
    run_agent service uninstall || true
  fi
  rm -rf "$install_dir"
  if [ "$purge" = yes ]; then
    rm -rf "$data_dir"
  fi
  echo "Wirehaus has been removed."
  exit 0
fi

verify_package

if [ -x "$agent" ]; then
  run_agent service uninstall || true
fi
mkdir -p "$install_dir"
rm -rf "$install_dir/viewer"
cp "$source_dir/wirehaus-agent" "$agent"
chmod 755 "$agent"
cp -R "$source_dir/viewer" "$install_dir/viewer"

if ! run_agent devices >/dev/null 2>&1; then
  mkdir -p "$data_dir"
  chmod 700 "$data_dir"
  run_agent init
  echo
  echo "Save the secret above in your authenticator app now: it is only shown once."
  echo
fi

run_agent service install --viewer-dir "$install_dir/viewer"
run_agent pairing
echo
echo "Open http://127.0.0.1:7878 on this computer, or reach it through your tunnel, and enter the pairing code."
