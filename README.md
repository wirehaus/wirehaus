# Wirehaus

Español | [English](README.en.md)

[![CI](https://github.com/wirehaus/wirehaus/actions/workflows/ci.yml/badge.svg)](https://github.com/wirehaus/wirehaus/actions/workflows/ci.yml)
[![Versión](https://img.shields.io/badge/versi%C3%B3n-0.2.0-blue)](CHANGELOG.md)
[![Licencia](https://img.shields.io/badge/licencia-PolyForm%20Shield%201.0.0-blue)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.90%2B-orange)](rust-toolchain.toml)
[![Plataformas](https://img.shields.io/badge/plataformas-Linux%20%C2%B7%20macOS%20%C2%B7%20Windows-lightgrey)](#plataformas)

**Acceso remoto a tu ordenador desde cualquier navegador: escritorio, terminal, archivos, portapapeles y audio, con cifrado de extremo a extremo.**

Dejas el ordenador de casa encendido con el agente de Wirehaus y te conectas desde donde estés, sin instalar nada en el equipo desde el que entras y sin abrir puertos en tu router.

- **Cifrado de extremo a extremo.** La clave se negocia entre tu navegador y tu agente; el túnel y quien lo opere ven solo bytes opacos.
- **Dos factores que no se pueden saltar.** Un código de emparejamiento de un solo uso y un TOTP que verifica tu propio agente, no un servidor ajeno.
- **Nada abierto.** El agente escucha solo en `127.0.0.1` y sale hacia fuera por un túnel.
- **Un comando para instalarlo**, y otro para quitarlo.
- **Todo activado al instalar, y apagable**: el instalador activa archivos de tu carpeta personal, terminal, portapapeles y audio; se quita lo que no quieras con un comando local.

## Instalación

Crea tu acceso en [wirehaus.dev](https://wirehaus.dev) y pulsa **Conectar** en tu panel: te da un comando con un código de un solo uso.

```
curl -fsSL https://wirehaus.dev/install.sh | sh -s -- XXXX-XXXX-XXXX-XXXX-XXXX
```

En Windows, en PowerShell: `& ([scriptblock]::Create((irm https://wirehaus.dev/install.ps1))) XXXX-XXXX-XXXX-XXXX-XXXX`.

El instalador descarga el paquete de tu sistema, comprueba su suma SHA-256, lo instala como servicio de tu usuario, descarga `cloudflared` (versión y suma fijadas en el propio script) y lo deja conectado a tu nombre. No pide administrador ni abre puertos. Quítalo con `--uninstall` (y `--purge` para borrar también las claves).

Los paquetes de la versión 0.2.0 son para Linux x86_64 y Windows x64; el de macOS (Apple Silicon) llega con la siguiente publicación y, mientras tanto, se [compila](#compilar).

Si prefieres no ejecutar un script descargado, bájate el paquete de [wirehaus.dev/download](https://wirehaus.dev/docs/#descarga), comprueba su firma y ejecuta `install.sh` (o `install.ps1`) desde la carpeta descomprimida. También puedes [compilarlo](#compilar) tú.

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

## Qué hace

| Función | Estado |
|---|---|
| Escritorio con ratón, teclado y rueda | Funciona. JPEG por zonas y, con mucha actividad, vídeo H.264 |
| Terminal en pestañas (xterm.js) | Funciona. Apagada hasta que la activas en el ordenador |
| Transferencia de archivos | Funciona. Solo en las carpetas que compartes |
| Portapapeles de texto | Funciona. Permiso aparte para cada sentido |
| Capturas y grabación | Funciona. Se hacen en el navegador |
| Audio del ordenador | Funciona en Linux (PulseAudio o PipeWire), Windows y macOS 14.6 o superior. Apagado hasta que lo activas |
| Visor en el móvil | Archivos y terminal (sin escritorio, casi sin datos). En tabletas, escritorio táctil con teclado en pantalla |
| Instalación en un solo comando y servicio de registro (`wirehaus.dev`) | Funciona |

## Seguridad

- Cifrado de extremo a extremo con Noise (`snow`, ChaCha20-Poly1305), con la clave del agente fijada en el emparejamiento. El túnel y quien lo opere ven solo bytes opacos.
- Dispositivo emparejado con un código de un solo uso de 160 bits y segundo factor TOTP que verifica el agente. Ninguna de las dos cosas basta por sí sola.
- Bloqueo creciente por dispositivo, suspensión tras diez fallos y registro de accesos sin secretos.
- Clave y secreto del segundo factor opcionalmente en el almacén de claves del sistema, fuera del fichero de estado.
- El instalador activa terminal, archivos (tu carpeta personal), portapapeles y audio; quien conecte necesita igualmente el emparejamiento y el segundo factor. Un `init` a secas los deja apagados; `wirehaus-agent allow-all --code 123456` los enciende de golpe, y activar uno suelto exige un código del segundo factor en el propio ordenador.
- Cierre automático por inactividad (30 minutos por defecto).
- Sin `unsafe` en el código propio, entradas validadas antes de reservar memoria y pruebas con datos hostiles en cada canal.
- El visor se sirve desde memoria, verificado contra un manifiesto de hashes SHA-256 de compilación reproducible.

El diseño y el código todavía no los ha auditado nadie ajeno al proyecto: úsalo sabiéndolo y no protejas con él nada que no puedas permitirte perder. Las vulnerabilidades se informan según [SECURITY.md](SECURITY.md).

## Plataformas

| Sistema | Estado |
|---|---|
| Linux (X11) | Funciona |
| macOS | Funciona. Necesita los permisos Grabación de pantalla y Accesibilidad |
| Windows 10 y 11 | Funciona. Debe ejecutarse dentro de la sesión de escritorio del usuario |
| Linux con Wayland | Previsto |

### macOS

En macOS, `wirehaus-agent permissions` comprueba y solicita los permisos y `wirehaus-agent service install` lo deja como servicio de usuario. macOS asocia los permisos a la firma del programa: `scripts/macos-sign.sh <binario>` lo firma con una identidad estable para no tener que volver a concederlos tras cada compilación.

### Windows

En Windows el agente captura con DXGI Desktop Duplication, inyecta teclas por posición física (código de escaneo, como en Linux y macOS) y usa el portapapeles del sistema. Tiene que arrancar en la sesión interactiva del usuario: un servicio de Windows o una sesión SSH corren en la sesión 0, que no tiene escritorio. `wirehaus-agent service install --viewer-dir <visor>` crea una tarea programada del usuario que lo lanza al iniciar sesión, sin ventana (`wirehaus-agentd.exe` lo arranca oculto y guarda el registro en `%LOCALAPPDATA%\Wirehaus\agent.log`) y lo vuelve a lanzar cada minuto si se cae; `service status` y `service uninstall` lo consultan y lo quitan. No puede controlar ventanas que se ejecutan con más privilegios que el agente, ni la pantalla de bloqueo ni el aviso de control de cuentas. El estado del agente se protege con una ACL que solo permite al usuario y al sistema. `scripts/build-windows.sh` compila desde Linux con mingw y `scripts/package-windows.sh` genera un `.zip` con los ejecutables, el visor, las sumas SHA-256 y `install.ps1`. Para instalarlo, descomprimir y ejecutar `powershell -ExecutionPolicy Bypass -File install.ps1`: instala en `%LOCALAPPDATA%\Programs\Wirehaus` sin pedir administrador, comprueba las sumas, crea las claves la primera vez (muestra el secreto del segundo factor una sola vez), instala el servicio y muestra el código de emparejamiento. `install.ps1 -Uninstall` lo quita y `-Purge` borra también las claves. El estado vive en `%LOCALAPPDATA%\Wirehaus\state`.

## Compilar

Hace falta Rust 1.90 o posterior y `nasm`, que usa el codificador H.264 al compilarse.

```
cargo build --workspace
cargo test --workspace
```

Los tests de integración en Linux arrancan un servidor X virtual y necesitan `xvfb`.

## Probar en local

Hace falta una sesión de escritorio con X11, macOS o Windows, Rust con el objetivo `wasm32-unknown-unknown`, `wasm-bindgen-cli` 0.2.129 y Node.js.

```
cargo install wasm-bindgen-cli --version 0.2.129 --locked
rustup target add wasm32-unknown-unknown
cd viewer && npm ci && npm run build && cd ..
cargo run -p wirehaus-agent -- init
cargo run -p wirehaus-agent -- serve --viewer-dir viewer/dist
```

`init` crea la identidad del agente y muestra el segundo factor como un QR (y como secreto escrito) para cualquier aplicación de autenticación (2FAS, Aegis, Google Authenticator, Contraseñas de Apple...); `wirehaus-agent totp` lo vuelve a mostrar en ese mismo ordenador. Para dar acceso a otro equipo sin teclear el código largo, pulsa «Conectar con mi móvil» en la página de emparejamiento y aprueba desde el móvil conectado (menú, «Vincular otro equipo»). Abre `http://127.0.0.1:7878` en el navegador. La primera vez genera un código con `cargo run -p wirehaus-agent -- pairing` y escríbelo en la página junto al código de seis dígitos de tu aplicación; el navegador queda emparejado y las siguientes veces solo pide el segundo factor.

El agente guarda su estado en `~/.config/wirehaus` (o en `--state-dir`), solo legible por su dueño. `devices` lista los dispositivos emparejados; `devices revoke`, `devices reactivate`, `lock` y `unlock` exigen un código del segundo factor con `--code`. Con `--display` eliges otra pantalla de X11 y con `--listen` otro puerto de la máquina local. Los mensajes salen en español o en inglés según `LANG`.

Los secretos del agente (su clave privada y el secreto del segundo factor) viven por defecto en el fichero de estado, con permisos privados. Puedes moverlos al almacén de claves del sistema con `wirehaus-agent secrets use system` y volver con `secrets use file --code 123456`; `secrets status` dice dónde están. En Linux hace falta `secret-tool` (paquete `libsecret-tools`) y un servicio de secretos desbloqueado, normalmente el de tu sesión de escritorio. Es opcional a propósito: si el almacén está bloqueado tras un reinicio, el agente no arranca hasta que lo desbloqueas, y si lo pierdes hay que volver a ejecutar `init` y emparejar de nuevo.

Si instalaste con el instalador, ya están activados. Para activarlos de golpe en un agente que no los tenga: `wirehaus-agent allow-all --code 123456` (los seis dígitos de tu app de autenticación, no el código de emparejamiento). Uno a uno, siempre en el ordenador y con ese código:

```
wirehaus-agent terminal allow --code 123456
wirehaus-agent files add ~/Documentos --code 123456
wirehaus-agent clipboard allow --direction both --code 123456
wirehaus-agent audio allow --code 123456
```

La terminal abre la shell del sistema (`$SHELL` en Linux y macOS, Windows PowerShell en Windows); `WIREHAUS_SHELL` permite elegir otra, por ejemplo `pwsh`. Quitarlos no exige código: `terminal deny`, `files remove <carpeta>`, `clipboard deny` y `audio deny`. El audio se escucha con el botón Escuchar del visor: en Linux hace falta `parec` (paquete `pulseaudio-utils`, también con PipeWire), en Windows y macOS se captura la salida predeterminada (en macOS hace falta el permiso de grabación de audio del sistema), y se codifica con Opus (`opus-pure`, Rust puro). Una sesión sin actividad se cierra a los 30 minutos (`--idle-minutes`), aunque estés escuchando.

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
| `crates/audio` | Captura del audio del sistema y codificación Opus |
| `crates/vault` | Acceso al almacén de claves del sistema (Llavero, Administrador de credenciales, servicio de secretos) |
| `crates/agent` | Agente que se ejecuta en el ordenador de casa |
| `viewer` | Visor que corre en el navegador |

## Contribuir

Lee [CONTRIBUTING.md](CONTRIBUTING.md) antes de proponer cambios y consulta el [registro de cambios](CHANGELOG.md).

## Contacto

Web: [wirehaus.dev](https://wirehaus.dev). Para cualquier consulta, escribe a [contact@wirehaus.dev](mailto:contact@wirehaus.dev). Las vulnerabilidades van a [security@wirehaus.dev](mailto:security@wirehaus.dev), según [SECURITY.md](SECURITY.md).

## Licencia

Wirehaus se distribuye bajo la licencia [PolyForm Shield 1.0.0](LICENSE). Puedes usarlo para cualquier fin, también en el trabajo, salvo para ofrecer un producto que compita con él. Es código con fuente disponible, no software libre en el sentido de la OSI.
