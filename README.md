# Wirehaus

Acceso remoto a tu ordenador desde cualquier navegador: escritorio, terminal, archivos y portapapeles, con cifrado de extremo a extremo.

Dejas el ordenador de casa encendido con el agente de Wirehaus y te conectas desde donde estés, sin instalar nada en el equipo desde el que entras y sin abrir puertos en tu router.

## Estado

Desarrollo temprano. Todavía no hay nada que instalar ni versiones publicadas.

Funciona hoy, probado en Linux con X11 y en macOS:

- Escritorio con ratón, teclado y rueda. Las zonas que cambian se envían como imágenes JPEG y, cuando hay mucha actividad, como vídeo H.264.
- Terminal en pestañas, con xterm.js.
- Transferencia de archivos limitada a las carpetas que compartes.
- Portapapeles de texto, con permiso aparte para cada sentido.
- Capturas de pantalla y grabación, hechas en el navegador.

Todo el tráfico va cifrado de extremo a extremo con Noise y exige un dispositivo emparejado y un segundo factor (TOTP) que comprueba el propio agente. La terminal, los archivos y el portapapeles están apagados hasta que los activas en el ordenador con un código del segundo factor.

Lo que falta: audio, soporte de Windows y de Wayland, el instalador en un comando y el servicio de registro. El agente solo escucha en `127.0.0.1` y comprueba `Host` y `Origin` en cada petición. Se puede publicar detrás de un túnel con `--public-host` y `--viewer-origin`, pero el diseño no ha pasado todavía una revisión externa: no lo expongas a Internet en un equipo que te importe.

## Qué busca ser

- Escritorio remoto fluido, pensado para equipos modestos.
- Terminal y transferencia de archivos en el mismo navegador, en pestañas.
- Cifrado de extremo a extremo entre el navegador y tu ordenador, con segundo factor obligatorio.
- Audio.
- Instalación en un solo comando.

## Plataformas

Linux (X11) y macOS. Windows 10 y 11 y Linux con Wayland están previstos y todavía no funcionan.

En macOS el agente necesita dos permisos del sistema, Grabación de pantalla y Accesibilidad. `wirehaus-agent permissions` los comprueba, los solicita y explica qué falta. `wirehaus-agent service install` lo deja funcionando como servicio de usuario. macOS asocia los permisos a la firma del programa: `scripts/macos-sign.sh <binario>` lo firma con una identidad estable para no tener que volver a concederlos tras cada compilación.

## Estructura

| Ruta | Contenido |
|---|---|
| `crates/protocol` | Mensajes y codificación del protocolo |
| `crates/crypto` | Handshake Noise, emparejamiento, segundo factor y capa de registros cifrados |
| `crates/crypto-wasm` | Enlaces de WebAssembly del handshake para el visor |
| `crates/capture` | Captura de pantalla y detección de zonas cambiadas |
| `crates/encode` | Codificación de las zonas cambiadas en JPEG y H.264 |
| `crates/input` | Inyección de ratón y teclado |
| `crates/clipboard` | Acceso al portapapeles del sistema |
| `crates/agent` | Agente que se ejecuta en el ordenador de casa |
| `viewer` | Visor que corre en el navegador |

## Compilar

Hace falta Rust 1.90 o posterior y `nasm`, que usa el codificador H.264 al compilarse.

```
cargo build --workspace
cargo test --workspace
```

Los tests de integración en Linux arrancan un servidor X virtual y necesitan `xvfb`.

## Probar en local

Hace falta una sesión de escritorio con X11 (o macOS), Rust con el objetivo `wasm32-unknown-unknown`, `wasm-bindgen-cli` 0.2.129 y Node.js.

```
cargo install wasm-bindgen-cli --version 0.2.129 --locked
rustup target add wasm32-unknown-unknown
cd viewer && npm ci && npm run build && cd ..
cargo run -p wirehaus-agent -- init
cargo run -p wirehaus-agent -- serve --viewer-dir viewer/dist
```

`init` crea la identidad del agente y muestra una vez el secreto del segundo factor para tu aplicación de autenticación. Abre `http://127.0.0.1:7878` en el navegador. La primera vez genera un código con `cargo run -p wirehaus-agent -- pairing` y escríbelo en la página junto al código de seis dígitos de tu aplicación; el navegador queda emparejado y las siguientes veces solo pide el segundo factor.

El agente guarda su estado en `~/.config/wirehaus` (o en `--state-dir`), solo legible por su dueño. `devices` lista los dispositivos emparejados; `devices revoke`, `devices reactivate`, `lock` y `unlock` exigen un código del segundo factor con `--code`. Con `--display` eliges otra pantalla de X11 y con `--listen` otro puerto de la máquina local.

Para activar los demás canales, siempre en el ordenador y con un código del segundo factor:

```
wirehaus-agent terminal allow --code 123456
wirehaus-agent files add ~/Documentos --code 123456
wirehaus-agent clipboard allow --direction both --code 123456
```

Quitarlos no exige código: `terminal deny`, `files remove <carpeta>` y `clipboard deny`. Una sesión sin actividad se cierra a los 30 minutos (`--idle-minutes`).

## Licencia

Wirehaus se distribuye bajo la licencia [PolyForm Shield 1.0.0](LICENSE). Puedes usarlo para cualquier fin, también en el trabajo, salvo para ofrecer un producto que compita con él. Es código con fuente disponible, no software libre en el sentido de la OSI.

## Seguridad

Si encuentras una vulnerabilidad, lee [SECURITY.md](SECURITY.md) antes de publicar nada.

---

# Wirehaus (English)

Remote access to your computer from any browser: desktop, terminal, files and clipboard, end-to-end encrypted.

You leave the home computer on with the Wirehaus agent and connect from anywhere, without installing anything on the machine you connect from and without opening ports on your router.

## Status

Early development. There is nothing to install and there are no releases yet.

Working today, tested on Linux with X11 and on macOS:

- Desktop with mouse, keyboard and wheel. Changed regions are sent as JPEG images and, under heavy activity, as H.264 video.
- Terminal in tabs, with xterm.js.
- File transfer limited to the folders you share.
- Text clipboard, with a separate permission for each direction.
- Screenshots and recording, done in the browser.

All traffic is end-to-end encrypted with Noise and requires a paired device and a second factor (TOTP) that the agent itself verifies. The terminal, files and clipboard stay off until you enable them on the computer with a second factor code.

Still missing: audio, Windows and Wayland support, the one-command installer and the registration service. The agent only listens on `127.0.0.1` and checks `Host` and `Origin` on every request. It can be published behind a tunnel with `--public-host` and `--viewer-origin`, but the design has not had an external review yet: do not expose it to the Internet on a machine you care about.

## Platforms

Linux (X11) and macOS. Windows 10 and 11 and Linux with Wayland are planned and do not work yet.

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

`init` creates the agent identity and shows the second factor secret once, for your authenticator app. Open `http://127.0.0.1:7878` in the browser. The first time, generate a code with `cargo run -p wirehaus-agent -- pairing` and type it in the page together with the six-digit code from your app; the browser is then paired and later visits only ask for the second factor.

To enable the other channels, always on the computer and with a second factor code:

```
wirehaus-agent terminal allow --code 123456
wirehaus-agent files add ~/Documents --code 123456
wirehaus-agent clipboard allow --direction both --code 123456
```

Removing them needs no code: `terminal deny`, `files remove <folder>` and `clipboard deny`. A session with no activity is closed after 30 minutes (`--idle-minutes`).

## License

Wirehaus is distributed under the [PolyForm Shield 1.0.0](LICENSE) license. You may use it for any purpose, including at work, except to offer a product that competes with it. It is source-available code, not free software in the OSI sense.

## Security

If you find a vulnerability, read [SECURITY.md](SECURITY.md) before publishing anything.
