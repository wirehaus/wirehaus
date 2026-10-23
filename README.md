# Wirehaus

Español | [English](README.en.md)

[![CI](https://github.com/wirehaus/wirehaus/actions/workflows/ci.yml/badge.svg)](https://github.com/wirehaus/wirehaus/actions/workflows/ci.yml)
[![Licencia](https://img.shields.io/badge/licencia-PolyForm%20Shield%201.0.0-blue)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.90%2B-orange)](rust-toolchain.toml)
[![Estado](https://img.shields.io/badge/estado-desarrollo%20temprano-yellow)](#estado)

Acceso remoto a tu ordenador desde cualquier navegador: escritorio, terminal, archivos y portapapeles, con cifrado de extremo a extremo.

Dejas el ordenador de casa encendido con el agente de Wirehaus y te conectas desde donde estés, sin instalar nada en el equipo desde el que entras y sin abrir puertos en tu router.

## Cómo funciona

```
 Navegador (visor)                                      Tu ordenador (agente)
 ┌────────────────┐   WebSocket sobre HTTPS   ┌──────────────────────────────┐
 │ TypeScript     │ ────────────────────────► │ Rust, escucha en 127.0.0.1   │
 │ + WebAssembly  │   cifrado Noise de        │ captura, entrada, terminal,  │
 │ (cifrado)      │   extremo a extremo       │ archivos, portapapeles       │
 └────────────────┘                           └──────────────────────────────┘
```

El agente solo escucha en `127.0.0.1`. Para llegar a él desde fuera se publica detrás de un túnel de salida (por ejemplo `cloudflared`), así que no hace falta abrir nada en el router. El túnel solo transporta bytes cifrados: el cifrado se negocia entre el navegador y el agente, y el segundo factor lo comprueba el propio agente.

## Qué hace hoy

| Función | Estado |
|---|---|
| Escritorio con ratón, teclado y rueda | Funciona. JPEG por zonas y, con mucha actividad, vídeo H.264 |
| Terminal en pestañas (xterm.js) | Funciona. Apagada hasta que la activas en el ordenador |
| Transferencia de archivos | Funciona. Solo en las carpetas que compartes |
| Portapapeles de texto | Funciona. Permiso aparte para cada sentido |
| Capturas y grabación | Funciona. Se hacen en el navegador |
| Audio | Pendiente |
| Instalación en un solo comando y servicio de registro | Pendiente |

## Seguridad

- Cifrado de extremo a extremo con Noise (`snow`, ChaCha20-Poly1305), con la clave del agente fijada en el emparejamiento. El túnel y quien lo opere ven solo bytes opacos.
- Dispositivo emparejado con un código de un solo uso de 160 bits y segundo factor TOTP que verifica el agente. Ninguna de las dos cosas basta por sí sola.
- Bloqueo creciente por dispositivo, suspensión tras diez fallos y registro de accesos sin secretos.
- Terminal, archivos y portapapeles desactivados por defecto. Activarlos exige un código del segundo factor en el propio ordenador.
- Cierre automático por inactividad (30 minutos por defecto).
- Sin `unsafe` en el código propio, entradas validadas antes de reservar memoria y pruebas con datos hostiles en cada canal.
- El visor se sirve desde memoria, verificado contra un manifiesto de hashes SHA-256 de compilación reproducible.

El diseño todavía no ha pasado una revisión externa: no lo expongas a Internet en un equipo que te importe. Las vulnerabilidades se informan según [SECURITY.md](SECURITY.md).

## Estado

Desarrollo temprano. Todavía no hay nada que instalar ni versiones publicadas.

Lo que falta: audio, soporte de Windows y de Wayland, el instalador en un comando y el servicio de registro. Se puede publicar detrás de un túnel con `--public-host` y `--viewer-origin`.

## Plataformas

| Sistema | Estado |
|---|---|
| Linux (X11) | Funciona |
| macOS | Funciona. Necesita los permisos Grabación de pantalla y Accesibilidad |
| Windows 10 y 11 | Previsto |
| Linux con Wayland | Previsto |

En macOS, `wirehaus-agent permissions` comprueba y solicita los permisos y `wirehaus-agent service install` lo deja como servicio de usuario. macOS asocia los permisos a la firma del programa: `scripts/macos-sign.sh <binario>` lo firma con una identidad estable para no tener que volver a concederlos tras cada compilación.

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

El agente guarda su estado en `~/.config/wirehaus` (o en `--state-dir`), solo legible por su dueño. `devices` lista los dispositivos emparejados; `devices revoke`, `devices reactivate`, `lock` y `unlock` exigen un código del segundo factor con `--code`. Con `--display` eliges otra pantalla de X11 y con `--listen` otro puerto de la máquina local. Los mensajes salen en español o en inglés según `LANG`.

Para activar los demás canales, siempre en el ordenador y con un código del segundo factor:

```
wirehaus-agent terminal allow --code 123456
wirehaus-agent files add ~/Documentos --code 123456
wirehaus-agent clipboard allow --direction both --code 123456
```

Quitarlos no exige código: `terminal deny`, `files remove <carpeta>` y `clipboard deny`. Una sesión sin actividad se cierra a los 30 minutos (`--idle-minutes`).

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

## Contribuir

Lee [CONTRIBUTING.md](CONTRIBUTING.md) antes de proponer cambios y consulta el [registro de cambios](CHANGELOG.md).

## Licencia

Wirehaus se distribuye bajo la licencia [PolyForm Shield 1.0.0](LICENSE). Puedes usarlo para cualquier fin, también en el trabajo, salvo para ofrecer un producto que compita con él. Es código con fuente disponible, no software libre en el sentido de la OSI.
