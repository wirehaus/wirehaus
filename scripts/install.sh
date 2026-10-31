#!/bin/sh
set -eu

source_dir=$(cd "$(dirname "$0")" && pwd)
install_dir=${WIREHAUS_INSTALL_DIR:-"$HOME/.local/share/wirehaus"}
agent="$install_dir/wirehaus-agent"
mode=install
purge=no
allow_all=no
onboard_mode=
name=
zone=wirehaus.dev
viewer_origin=https://app.wirehaus.dev
public_host=
tunnel_token_file=
site_origin=https://wirehaus.dev
connector_file=
fetch_connector=
cf_version=2026.10.4
cf_linux_amd64_sha256=ae5e8add7884b0531d66bbca4570d1de491ff6da51e4ebe7eb160cdf241d9e42
cf_linux_amd64_gz_sha256=be6c8a8b56fe9e516cee7c6e6dd97da09529bae182677b9d67a9b3642f219588
cf_darwin_arm64_sha256=68b6bc4080ac1330234db2a9002d07f98b000537a1bd531a687b660d4160fa7f
cf_darwin_amd64_sha256=f11fbef4207d715892331917dafd437f032cf4b7137ed64dea3008338c1c613d

case "${LC_ALL:-${LC_MESSAGES:-${LANG:-}}}" in
  es*) lang=es ;;
  *) lang=en ;;
esac

say() {
  if [ "$lang" = es ]; then printf '%s\n' "$1"; else printf '%s\n' "$2"; fi
}

if [ -t 1 ] && [ -z "${NO_COLOR:-}" ]; then
  c_accent=$(printf '\033[1;36m'); c_off=$(printf '\033[0m')
else
  c_accent=; c_off=
fi

step() {
  printf '\n%s%s%s  ' "$c_accent" "$1" "$c_off"
  if [ "$lang" = es ]; then printf '%s\n' "$2"; else printf '%s\n' "$3"; fi
}

fail() {
  say "$1" "$2" >&2
  exit 1
}

while [ $# -gt 0 ]; do
  case "$1" in
    --uninstall) mode=uninstall ;;
    --purge) purge=yes ;;
    --allow-all) allow_all=yes ;;
    --phone) onboard_mode=--phone ;;
    --card) onboard_mode=--card ;;
    --name) [ $# -ge 2 ] || { echo "--name needs a value" >&2; exit 2; }; name=$2; shift ;;
    --zone) [ $# -ge 2 ] || { echo "--zone needs a value" >&2; exit 2; }; zone=$2; shift ;;
    --viewer-origin) [ $# -ge 2 ] || { echo "--viewer-origin needs a value" >&2; exit 2; }; viewer_origin=$2; shift ;;
    --tunnel-token-file) [ $# -ge 2 ] || { echo "--tunnel-token-file needs a value" >&2; exit 2; }; tunnel_token_file=$2; shift ;;
    --public-host) [ $# -ge 2 ] || { echo "--public-host needs a value" >&2; exit 2; }; public_host=$2; shift ;;
    --connector-file) [ $# -ge 2 ] || { echo "--connector-file needs a value" >&2; exit 2; }; connector_file=$2; shift ;;
    --fetch-connector) [ $# -ge 2 ] || { echo "--fetch-connector needs a value" >&2; exit 2; }; fetch_connector=$2; shift ;;
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
    if [ -t 2 ]; then meter=-#; else meter=-sS; fi
    curl -fL "$meter" --proto '=https' --proto-redir '=https' --connect-timeout 20 --retry 3 --retry-delay 2 --speed-limit 20000 --speed-time 40 -o "$2" "$1"
  else
    wget -q -T 40 -t 3 -O "$2" "$1"
  fi
}

download_cloudflared() {
  if [ -n "$connector_file" ]; then
    [ -f "$connector_file" ] || fail "No encuentro el conector descargado." "The downloaded connector is missing."
    cf_binary=$connector_file
    return 0
  fi
  github=https://github.com/cloudflare/cloudflared/releases/download/$cf_version
  case "$(uname -s)-$(uname -m)" in
    Linux-x86_64)
      official=cloudflared-linux-amd64; official_sum=$cf_linux_amd64_sha256
      mirror=cloudflared-$cf_version-linux-amd64.gz; mirror_sum=$cf_linux_amd64_gz_sha256; kind=gz ;;
    Darwin-arm64)
      official=cloudflared-darwin-arm64.tgz; official_sum=$cf_darwin_arm64_sha256
      mirror=cloudflared-$cf_version-darwin-arm64.tgz; mirror_sum=$cf_darwin_arm64_sha256; kind=tgz ;;
    Darwin-x86_64)
      official=cloudflared-darwin-amd64.tgz; official_sum=$cf_darwin_amd64_sha256
      mirror=cloudflared-$cf_version-darwin-amd64.tgz; mirror_sum=$cf_darwin_amd64_sha256; kind=tgz ;;
    *) fail "No hay conector de túnel para $(uname -s) $(uname -m)." "No tunnel connector is available for $(uname -s) $(uname -m)." ;;
  esac
  cf_work=$(mktemp -d)
  trap 'rm -rf "$cf_work"' EXIT
  say "Descargando el conector del túnel (unos 20 MB)..." "Downloading the tunnel connector (about 20 MB)..."
  cf_binary=
  if fetch "$site_origin/download/$mirror" "$cf_work/$mirror" && [ "$(checksum "$cf_work/$mirror")" = "$mirror_sum" ]; then
    case "$kind" in
      gz) gzip -dc "$cf_work/$mirror" > "$cf_work/cloudflared" && [ "$(checksum "$cf_work/cloudflared")" = "$official_sum" ] && cf_binary=$cf_work/cloudflared ;;
      *) LC_ALL=C tar -xzf "$cf_work/$mirror" -C "$cf_work" cloudflared && cf_binary=$cf_work/cloudflared ;;
    esac
  fi
  if [ -z "$cf_binary" ]; then
    say "La primera fuente no ha respondido bien; pruebo con la segunda..." "The first source did not answer well; trying the second one..."
    fetch "$github/$official" "$cf_work/$official" || fail "No se pudo descargar el conector del túnel. Comprueba tu conexión y repite el comando." "Could not download the tunnel connector. Check your connection and run the command again."
    [ "$(checksum "$cf_work/$official")" = "$official_sum" ] || fail "El conector descargado no coincide con su suma de comprobación." "The downloaded connector does not match its checksum."
    case "$official" in
      *.tgz) LC_ALL=C tar -xzf "$cf_work/$official" -C "$cf_work" cloudflared; cf_binary=$cf_work/cloudflared ;;
      *) cf_binary=$cf_work/$official ;;
    esac
  fi
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
  rm -f "$cloudflared"
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

if [ -n "$fetch_connector" ]; then
  download_cloudflared
  cp "$cf_binary" "$fetch_connector"
  exit 0
fi

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

tunnel_up() {
  code=$(curl -s -o /dev/null -w '%{http_code}' -m 6 -H "Origin: $viewer_origin" "https://$public_host/link/ping" 2>/dev/null || true)
  [ "$code" = 200 ]
}

wait_tunnel() {
  waited=0
  while [ "$waited" -lt "$1" ]; do
    tunnel_up && return 0
    sleep 2
    waited=$((waited + 2))
  done
  return 1
}

mullvad_cli() {
  for candidate in "$(command -v mullvad 2>/dev/null || true)" "/Applications/Mullvad VPN.app/Contents/Resources/mullvad"; do
    if [ -n "$candidate" ] && [ -x "$candidate" ]; then
      printf '%s' "$candidate"
      return 0
    fi
  done
  return 1
}

check_tunnel() {
  command -v curl >/dev/null 2>&1 || return 0
  say "Comprobando que el túnel conecta..." "Checking that the tunnel connects..."
  if wait_tunnel 40; then
    say "Túnel conectado." "Tunnel connected."
    return 0
  fi
  if [ "$(uname -s)" = Darwin ] && mullvad_bin=$(mullvad_cli) && "$mullvad_bin" status 2>/dev/null | grep -q "^Connected"; then
    say "Mullvad VPN bloquea la conexión del túnel. Excluyo solo el conector de Wirehaus de la VPN (el resto de tu tráfico sigue por Mullvad)..." "Mullvad VPN is blocking the tunnel connection. Excluding only the Wirehaus connector from the VPN (the rest of your traffic still goes through Mullvad)..."
    "$mullvad_bin" split-tunnel app add "$cloudflared" >/dev/null 2>&1 || true
    "$mullvad_bin" split-tunnel set on >/dev/null 2>&1 || true
    launchctl kickstart -k "gui/$(id -u)/$tunnel_label" >/dev/null 2>&1 || true
    if wait_tunnel 40; then
      say "Túnel conectado." "Tunnel connected."
      return 0
    fi
  fi
  say "Aviso: el túnel no consigue conectar con Cloudflare (puerto 7844), así que tu ordenador aún no es accesible desde fuera." "Warning: the tunnel cannot reach Cloudflare (port 7844), so your computer is not reachable from outside yet." >&2
  say "Suele ser una VPN (por ejemplo Mullvad: excluye el conector de la VPN) o el cortafuegos de una red de empresa o colegio. Prueba con otra red o desactiva la VPN un momento; el servicio sigue reintentando solo." "It is usually a VPN (for example Mullvad: exclude the connector from the VPN) or the firewall of a company or school network. Try another network or turn the VPN off for a moment; the service keeps retrying by itself." >&2
  if [ "$(uname -s)" = Darwin ]; then
    say "Registro del túnel: $HOME/Library/Logs/Wirehaus/tunnel.log" "Tunnel log: $HOME/Library/Logs/Wirehaus/tunnel.log" >&2
  else
    say "Registro del túnel: journalctl --user -u wirehaus-tunnel" "Tunnel log: journalctl --user -u wirehaus-tunnel" >&2
  fi
}

mac_permissions() {
  log="$HOME/Library/Logs/Wirehaus/agent.log"
  waited=0
  while [ "$waited" -lt 12 ] && ! grep -qiE "Accessibility|Accesibilidad" "$log" 2>/dev/null; do
    sleep 1
    waited=$((waited + 1))
  done
  if tail -n 6 "$log" 2>/dev/null | grep -qiE "pending|pendiente"; then
    say "macOS necesita dos permisos para wirehaus-agent. Voy a abrir los Ajustes: activa «wirehaus-agent» en Grabación de pantalla y en Accesibilidad (si no aparece, pulsa + y elige $agent)." "macOS needs two permissions for wirehaus-agent. I am opening System Settings: turn on \"wirehaus-agent\" under Screen Recording and under Accessibility (if it is not listed, click + and pick $agent)."
    open "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture" >/dev/null 2>&1 || true
    sleep 2
    open "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility" >/dev/null 2>&1 || true
  fi
}

verify_package

printf '%s' "$c_accent"
say "Wirehaus - acceso remoto a tu ordenador" "Wirehaus - remote access to your computer"
printf '%s' "$c_off"
step "[1/4]" "Instalando el agente..." "Installing the agent..."
if [ -x "$agent" ]; then
  run_agent service uninstall >/dev/null 2>&1 || true
fi
mkdir -p "$install_dir"
rm -rf "$install_dir/viewer"
rm -f "$agent"
cp "$source_dir/wirehaus-agent" "$agent"
chmod 755 "$agent"
cp -R "$source_dir/viewer" "$install_dir/viewer"
mkdir -p "$HOME/.local/bin"
ln -sf "$agent" "$HOME/.local/bin/wirehaus-agent"

if ! run_agent devices >/dev/null 2>&1; then
  mkdir -p "$data_dir"
  chmod 700 "$data_dir"
  step "[2/4]" "Creando la identidad de este ordenador..." "Creating the identity of this computer..."
  if [ "$allow_all" = yes ]; then
    run_agent init --quiet --allow-all
  else
    run_agent init --quiet
    say "Terminal, archivos, portapapeles y audio quedan apagados. Para encenderlos (cuando quieras, con los seis dígitos de la app): wirehaus-agent allow-all --code SEIS_DÍGITOS" "Terminal, files, clipboard and audio are left off. To turn them on (whenever you like, with the six digits from the app): wirehaus-agent allow-all --code SIX_DIGITS"
  fi
else
  step "[2/4]" "Este equipo ya estaba configurado (segundo factor incluido). Para activar archivos, terminal, portapapeles y audio de golpe: wirehaus-agent allow-all --code SEIS_DÍGITOS" "This computer was already set up (second factor included). To turn on files, terminal, clipboard and audio at once: wirehaus-agent allow-all --code SIX_DIGITS"
fi

step "[3/4]" "Arrancando el servicio y el túnel..." "Starting the service and the tunnel..."
if [ -n "$public_host" ]; then
  run_agent service install --viewer-dir "$install_dir/viewer" --public-host "$public_host" --viewer-origin "$viewer_origin" >/dev/null
else
  run_agent service install --viewer-dir "$install_dir/viewer" >/dev/null
fi
if [ -n "$tunnel_token_file" ]; then
  install_tunnel
fi
if [ "$(uname -s)" = Darwin ]; then
  mac_permissions
fi
if [ -n "$tunnel_token_file" ] && [ -n "$public_host" ]; then
  check_tunnel
fi

step "[4/4]" "Entrar a tu ordenador desde fuera" "Getting into your computer from outside"
if [ -n "$public_host" ]; then
  [ -n "$tunnel_token_file" ] || say "Antes conecta el túnel (mira tu panel)." "First connect the tunnel (see your dashboard)."
  wait_flag=
  if [ ! -t 1 ] || [ -n "${WIREHAUS_NO_WAIT:-}" ]; then wait_flag=--no-wait; fi
  run_agent onboard $onboard_mode $wait_flag
else
  echo "Open http://127.0.0.1:7878 on this computer, or reach it through your tunnel, and enter the pairing code."
  run_agent totp
  run_agent pairing
fi
