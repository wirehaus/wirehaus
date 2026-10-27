#!/bin/sh
set -eu

source_dir=$(cd "$(dirname "$0")" && pwd)
install_dir=${WIREHAUS_INSTALL_DIR:-"$HOME/.local/share/wirehaus"}
agent="$install_dir/wirehaus-agent"
mode=install
purge=no
name=
zone=wirehaus.dev
viewer_origin=https://app.wirehaus.dev
public_host=
tunnel_token_file=
cf_version=2026.10.4
cf_linux_amd64_sha256=ae5e8add7884b0531d66bbca4570d1de491ff6da51e4ebe7eb160cdf241d9e42
cf_darwin_arm64_sha256=68b6bc4080ac1330234db2a9002d07f98b000537a1bd531a687b660d4160fa7f

while [ $# -gt 0 ]; do
  case "$1" in
    --uninstall) mode=uninstall ;;
    --purge) purge=yes ;;
    --name) [ $# -ge 2 ] || { echo "--name needs a value" >&2; exit 2; }; name=$2; shift ;;
    --zone) [ $# -ge 2 ] || { echo "--zone needs a value" >&2; exit 2; }; zone=$2; shift ;;
    --viewer-origin) [ $# -ge 2 ] || { echo "--viewer-origin needs a value" >&2; exit 2; }; viewer_origin=$2; shift ;;
    --tunnel-token-file) [ $# -ge 2 ] || { echo "--tunnel-token-file needs a value" >&2; exit 2; }; tunnel_token_file=$2; shift ;;
    --public-host) [ $# -ge 2 ] || { echo "--public-host needs a value" >&2; exit 2; }; public_host=$2; shift ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
  esac
  shift
done

if [ -n "$name" ] && [ -z "$public_host" ]; then
  public_host=$name.$zone
fi

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
  while read -r hash file; do
    [ -f "$source_dir/$file" ] || { echo "$file is listed in SHA256SUMS but missing." >&2; exit 1; }
    [ "$(checksum "$source_dir/$file")" = "$hash" ] || { echo "$file does not match SHA256SUMS." >&2; exit 1; }
  done < "$sums"
}

cloudflared="$install_dir/cloudflared"
tunnel_token="$data_dir/tunnel-token"
tunnel_unit="$HOME/.config/systemd/user/wirehaus-tunnel.service"
tunnel_plist="$HOME/Library/LaunchAgents/wirehaus.tunnel.plist"
tunnel_label=wirehaus.tunnel

fetch() {
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL --proto '=https' --proto-redir '=https' -o "$2" "$1"
  else
    wget -q -O "$2" "$1"
  fi
}

download_cloudflared() {
  base=https://github.com/cloudflare/cloudflared/releases/download/$cf_version
  case "$(uname -s)-$(uname -m)" in
    Linux-x86_64) asset=cloudflared-linux-amd64; want=$cf_linux_amd64_sha256 ;;
    Darwin-arm64) asset=cloudflared-darwin-arm64.tgz; want=$cf_darwin_arm64_sha256 ;;
    *) echo "No tunnel connector is available for $(uname -s) $(uname -m)." >&2; exit 1 ;;
  esac
  cf_work=$(mktemp -d)
  trap 'rm -rf "$cf_work"' EXIT
  fetch "$base/$asset" "$cf_work/$asset" || { echo "Could not download the tunnel connector." >&2; exit 1; }
  [ "$(checksum "$cf_work/$asset")" = "$want" ] || { echo "$asset does not match the expected checksum." >&2; exit 1; }
  case "$asset" in
    *.tgz) tar -xzf "$cf_work/$asset" -C "$cf_work" cloudflared; cf_binary=$cf_work/cloudflared ;;
    *) cf_binary=$cf_work/$asset ;;
  esac
  chmod 755 "$cf_binary"
}

remove_tunnel() {
  if [ "$(uname -s)" = Darwin ]; then
    launchctl bootout "gui/$(id -u)/$tunnel_label" >/dev/null 2>&1 || true
    rm -f "$tunnel_plist"
  else
    systemctl --user disable --now wirehaus-tunnel.service >/dev/null 2>&1 || true
    rm -f "$tunnel_unit"
    systemctl --user daemon-reload >/dev/null 2>&1 || true
  fi
  rm -f "$cloudflared" "$tunnel_token"
}

install_tunnel() {
  download_cloudflared
  remove_tunnel
  cp "$cf_binary" "$cloudflared"
  chmod 755 "$cloudflared"
  mkdir -p "$data_dir"
  chmod 700 "$data_dir"
  ( umask 077; tr -d '\r\n' < "$tunnel_token_file" > "$tunnel_token" )
  if [ "$(uname -s)" = Darwin ]; then
    mkdir -p "$HOME/Library/LaunchAgents" "$HOME/Library/Logs/Wirehaus"
    cat > "$tunnel_plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>$tunnel_label</string>
    <key>ProgramArguments</key>
    <array>
        <string>$cloudflared</string>
        <string>tunnel</string>
        <string>run</string>
        <string>--token-file</string>
        <string>$tunnel_token</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardErrorPath</key>
    <string>$HOME/Library/Logs/Wirehaus/tunnel.log</string>
    <key>StandardOutPath</key>
    <string>$HOME/Library/Logs/Wirehaus/tunnel.log</string>
</dict>
</plist>
EOF
    launchctl bootstrap "gui/$(id -u)" "$tunnel_plist"
  else
    mkdir -p "$HOME/.config/systemd/user"
    cat > "$tunnel_unit" <<EOF
[Unit]
Description=Wirehaus tunnel
After=network-online.target
Wants=network-online.target

[Service]
ExecStart="$cloudflared" tunnel run --token-file "$tunnel_token"
Restart=always
RestartSec=5
NoNewPrivileges=yes

[Install]
WantedBy=default.target
EOF
    loginctl enable-linger "$(id -un)" >/dev/null 2>&1 || true
    systemctl --user daemon-reload || { echo "systemctl --user is not available in this session: sign in to the desktop (not over a bare SSH session) and run the installer again." >&2; exit 1; }
    systemctl --user enable --now wirehaus-tunnel.service
  fi
}

run_agent() {
  "$agent" --state-dir "$data_dir" "$@"
}

if [ "$mode" = uninstall ]; then
  if [ -x "$agent" ]; then
    run_agent service uninstall || true
  fi
  remove_tunnel
  rm -f "$HOME/.local/bin/wirehaus-agent"
  case "$install_dir" in ""|/) echo "refusing to remove $install_dir" >&2; exit 1 ;; esac
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
mkdir -p "$HOME/.local/bin"
ln -sf "$agent" "$HOME/.local/bin/wirehaus-agent"

if ! run_agent devices >/dev/null 2>&1; then
  mkdir -p "$data_dir"
  chmod 700 "$data_dir"
  run_agent init
  echo
  echo "Save the secret above in your authenticator app now: it is only shown once."
  echo
fi

if [ -n "$public_host" ]; then
  run_agent service install --viewer-dir "$install_dir/viewer" --public-host "$public_host" --viewer-origin "$viewer_origin"
else
  run_agent service install --viewer-dir "$install_dir/viewer"
fi
if [ -n "$tunnel_token_file" ]; then
  install_tunnel
fi
run_agent pairing
echo
if [ -n "$public_host" ] && [ -n "$tunnel_token_file" ]; then
  echo "The tunnel is connected. Open $viewer_origin/${public_host%%.*} and enter the pairing code."
elif [ -n "$public_host" ]; then
  echo "Next, connect the tunnel (see your dashboard), then open $viewer_origin/${public_host%%.*} and enter the pairing code."
else
  echo "Open http://127.0.0.1:7878 on this computer, or reach it through your tunnel, and enter the pairing code."
fi
