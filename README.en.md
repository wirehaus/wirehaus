# Wirehaus

[Español](README.md) | English

[![CI](https://github.com/wirehaus/wirehaus/actions/workflows/ci.yml/badge.svg)](https://github.com/wirehaus/wirehaus/actions/workflows/ci.yml)
[![Version](https://img.shields.io/badge/version-0.2.0-blue)](CHANGELOG.md)
[![License](https://img.shields.io/badge/license-PolyForm%20Shield%201.0.0-blue)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.90%2B-orange)](rust-toolchain.toml)
[![Platforms](https://img.shields.io/badge/platforms-Linux%20%C2%B7%20macOS%20%C2%B7%20Windows-lightgrey)](#platforms)

**Remote access to your computer from any browser: desktop, terminal, files, clipboard and audio, end-to-end encrypted.**

You leave the home computer on with the Wirehaus agent and connect from anywhere, without installing anything on the machine you connect from and without opening ports on your router.

- **End-to-end encryption.** The key is negotiated between your browser and your agent; the tunnel and whoever runs it see only opaque bytes.
- **Two factors you cannot skip.** A one-time pairing code and a TOTP verified by your own agent, not by someone else's server.
- **Nothing open.** The agent listens only on `127.0.0.1` and goes out through a tunnel.
- **One command to install it**, and one to remove it.
- **Sensitive features off by default**: terminal, files, clipboard and audio are turned on by hand, on the computer itself.

## Install

Create your access at [wirehaus.dev](https://wirehaus.dev) and click **Connect** in your dashboard: it gives you a command with a single-use code.

```
curl -fsSL https://wirehaus.dev/install.sh | sh -s -- XXXX-XXXX-XXXX-XXXX-XXXX
```

On Windows, in PowerShell: `& ([scriptblock]::Create((irm https://wirehaus.dev/install.ps1))) XXXX-XXXX-XXXX-XXXX-XXXX`.

The installer downloads the package for your system, checks its SHA-256 sum, installs it as a service for your user, downloads `cloudflared` (version and checksum pinned in the script itself) and leaves it connected to your name. It asks for no administrator rights and opens no ports. Remove it with `--uninstall` (and `--purge` to delete the keys too).

The 0.2.0 packages are for Linux x86_64 and Windows x64; the macOS (Apple Silicon) one arrives with the next release and, until then, you can [build it](#build).

If you would rather not run a downloaded script, download the package from [wirehaus.dev/download](https://wirehaus.dev/docs/#download), check its signature and run `install.sh` (or `install.ps1`) from the extracted folder. You can also [build it](#build) yourself.

## How it works

```
 Browser (viewer)                                       Your computer (agent)
 ┌────────────────┐    WebSocket over HTTPS   ┌──────────────────────────────┐
 │ TypeScript     │ ────────────────────────► │ Rust, listens on 127.0.0.1   │
 │ + WebAssembly  │   Noise encryption,       │ capture, input, terminal,    │
 │ (encryption)   │   end to end              │ files, clipboard             │
 └────────────────┘                           └──────────────────────────────┘
```

The agent only listens on `127.0.0.1`. To reach it from outside it is published behind an outbound tunnel (for example `cloudflared`), so nothing needs to be opened on the router. The tunnel only carries encrypted bytes: encryption is negotiated between the browser and the agent, and the agent itself checks the second factor.

## What it does

| Feature | Status |
|---|---|
| Desktop with mouse, keyboard and wheel | Works. JPEG by region and, under heavy activity, H.264 video |
| Terminal in tabs (xterm.js) | Works. Off until you enable it on the computer |
| File transfer | Works. Only in the folders you share |
| Text clipboard | Works. Separate permission for each direction |
| Screenshots and recording | Works. Done in the browser |
| Computer audio | Works on Linux (PulseAudio or PipeWire), Windows and macOS 14.6 or later. Off until you enable it |
| Viewer on a phone | Files and terminal (no desktop, almost no data). On tablets, touch desktop with an on-screen keyboard |
| One-command install and registration service (`wirehaus.dev`) | Works |

## Security

- End-to-end encryption with Noise (`snow`, ChaCha20-Poly1305), with the agent key pinned at pairing. The tunnel and whoever runs it see only opaque bytes.
- A device paired with a one-time 160-bit code, plus a TOTP second factor verified by the agent. Neither is enough on its own.
- Escalating lockout per device, suspension after ten failures and an access log with no secrets.
- The key and the second factor secret can optionally live in the system secret store, outside the state file.
- Terminal, files, clipboard and audio are off by default. Enabling them needs a second factor code on the computer itself.
- Automatic close on inactivity (30 minutes by default).
- No `unsafe` in our own code, inputs validated before allocating memory and tests with hostile data on every channel.
- The viewer is served from memory, verified against a manifest of SHA-256 hashes from a reproducible build.

Nobody outside the project has audited the design or the code yet: use it knowing that, and do not protect anything with it that you cannot afford to lose. Report vulnerabilities as described in [SECURITY.md](SECURITY.md).

## Platforms

| System | Status |
|---|---|
| Linux (X11) | Works |
| macOS | Works. Needs the Screen Recording and Accessibility permissions |
| Windows 10 and 11 | Works. Must run inside the user's desktop session |
| Linux with Wayland | Planned |

### macOS

On macOS, `wirehaus-agent permissions` checks and requests the permissions and `wirehaus-agent service install` sets it up as a user service. macOS ties permissions to the program's signature: `scripts/macos-sign.sh <binary>` signs it with a stable identity so you do not have to grant them again after every build.

### Windows

On Windows the agent captures with DXGI Desktop Duplication, injects keys by physical position (scan codes, as on Linux and macOS) and uses the system clipboard. It has to start in the user's interactive session: a Windows service or an SSH session runs in session 0, which has no desktop. `wirehaus-agent service install --viewer-dir <viewer>` creates a per-user scheduled task that launches it at sign-in without a window (`wirehaus-agentd.exe` starts it hidden and keeps the log in `%LOCALAPPDATA%\Wirehaus\agent.log`) and relaunches it every minute if it dies; `service status` and `service uninstall` query and remove it. It cannot control windows that run with more privileges than the agent, nor the lock screen or the account control prompt. The agent state is protected with an ACL that only allows the user and the system. `scripts/build-windows.sh` builds from Linux with mingw and `scripts/package-windows.sh` produces a `.zip` with the executables, the viewer, the SHA-256 sums and `install.ps1`. To install, unzip and run `powershell -ExecutionPolicy Bypass -File install.ps1`: it installs into `%LOCALAPPDATA%\Programs\Wirehaus` without asking for administrator rights, checks the sums, creates the keys the first time (showing the second-factor secret once), installs the service and prints the pairing code. `install.ps1 -Uninstall` removes it and `-Purge` also deletes the keys. State lives in `%LOCALAPPDATA%\Wirehaus\state`.

## Build

You need Rust 1.90 or later and `nasm`, which the H.264 encoder uses while it builds.

```
cargo build --workspace
cargo test --workspace
```

The Linux integration tests start a virtual X server and need `xvfb`.

## Try it locally

You need a desktop session with X11 (or macOS), Rust with the `wasm32-unknown-unknown` target, `wasm-bindgen-cli` 0.2.129 and Node.js.

```
cargo install wasm-bindgen-cli --version 0.2.129 --locked
rustup target add wasm32-unknown-unknown
cd viewer && npm ci && npm run build && cd ..
cargo run -p wirehaus-agent -- init
cargo run -p wirehaus-agent -- serve --viewer-dir viewer/dist
```

`init` creates the agent identity and shows the second factor as a QR (and as a typed secret) for any authenticator app (2FAS, Aegis, Google Authenticator, Apple Passwords...); `wirehaus-agent totp` shows it again on that same computer. Open `http://127.0.0.1:7878` in the browser. The first time, generate a code with `cargo run -p wirehaus-agent -- pairing` and type it in the page together with the six-digit code from your app; the browser is then paired and later visits only ask for the second factor.

The agent keeps its state in `~/.config/wirehaus` (or `--state-dir`), readable only by its owner. `devices` lists the paired devices; `devices revoke`, `devices reactivate`, `lock` and `unlock` need a second factor code with `--code`. `--display` picks another X11 display and `--listen` another port on the local machine. Messages come out in Spanish or English according to `LANG`.

The agent secrets (its private key and the second factor secret) live by default in the state file, with private permissions. You can move them to the system secret store with `wirehaus-agent secrets use system` and back with `secrets use file --code 123456`; `secrets status` tells you where they are. On Linux you need `secret-tool` (package `libsecret-tools`) and an unlocked secret service, normally the one of your desktop session. It is optional on purpose: if the store is locked after a reboot the agent will not start until you unlock it, and if you lose it you must run `init` again and pair again.

To enable the other channels, always on the computer and with a second factor code:

```
wirehaus-agent terminal allow --code 123456
wirehaus-agent files add ~/Documents --code 123456
wirehaus-agent clipboard allow --direction both --code 123456
wirehaus-agent audio allow --code 123456
```

The terminal opens the system shell (`$SHELL` on Linux and macOS, Windows PowerShell on Windows); `WIREHAUS_SHELL` picks another one, for example `pwsh`. Removing them needs no code: `terminal deny`, `files remove <folder>`, `clipboard deny` and `audio deny`. Audio is played with the Listen button in the viewer: on Linux it needs `parec` (package `pulseaudio-utils`, also with PipeWire), on Windows and macOS the default output is captured (macOS needs the system audio recording permission), and it is encoded with Opus (`opus-pure`, pure Rust). A session with no activity is closed after 30 minutes (`--idle-minutes`), even while you listen.

## Layout

| Path | Contents |
|---|---|
| `crates/protocol` | Protocol messages and encoding |
| `crates/crypto` | Noise handshake, pairing, second factor and encrypted record layer |
| `crates/crypto-wasm` | WebAssembly bindings of the handshake for the viewer |
| `crates/capture` | Screen capture and changed-region detection |
| `crates/encode` | JPEG and H.264 encoding of the changed regions |
| `crates/input` | Mouse and keyboard injection |
| `crates/clipboard` | System clipboard access |
| `crates/audio` | System audio capture and Opus encoding |
| `crates/vault` | Access to the system secret store (Keychain, Credential Manager, secret service) |
| `crates/agent` | The agent that runs on the home computer |
| `viewer` | The viewer that runs in the browser |

## Contributing

Read [CONTRIBUTING.md](CONTRIBUTING.md) before proposing changes and see the [changelog](CHANGELOG.md).

## Contact

Website: [wirehaus.dev](https://wirehaus.dev). For any question, write to [contact@wirehaus.dev](mailto:contact@wirehaus.dev). Report vulnerabilities to [security@wirehaus.dev](mailto:security@wirehaus.dev), as described in [SECURITY.md](SECURITY.md).

## License

Wirehaus is distributed under the [PolyForm Shield 1.0.0](LICENSE) license. You may use it for any purpose, including at work, except to offer a product that competes with it. It is source-available code, not free software in the OSI sense.
