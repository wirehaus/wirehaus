# Wirehaus

Acceso remoto a tu ordenador desde cualquier navegador: escritorio, terminal, archivos, portapapeles y audio, con cifrado de extremo a extremo.

Dejas el ordenador de casa encendido con el agente de Wirehaus y te conectas desde donde estés, sin instalar nada en el equipo desde el que entras y sin abrir puertos en tu router.

## Estado

Desarrollo temprano. Todavía no hay nada que instalar.

El agente ya muestra y controla un escritorio Linux con X11 desde un navegador de la misma máquina. El tráfico va cifrado de extremo a extremo con Noise y exige un dispositivo emparejado y un segundo factor. Aun así el servidor solo escucha en `127.0.0.1`, rechaza cualquier otra dirección y comprueba `Host` y `Origin` en cada petición, y todavía no hay una revisión de seguridad terminada: no lo expongas por un túnel ni por la red.

## Qué busca ser

- Escritorio remoto fluido, pensado para equipos modestos.
- Terminal y transferencia de archivos en el mismo navegador, en pestañas.
- Portapapeles compartido, audio, capturas de pantalla y grabación.
- Cifrado de extremo a extremo entre el navegador y tu ordenador, con segundo factor obligatorio.
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
| `crates/encode` | Codificación de las zonas cambiadas |
| `crates/input` | Inyección de ratón y teclado |
| `crates/agent` | Agente que se ejecuta en el ordenador de casa |
| `viewer` | Visor que corre en el navegador |

## Compilar

Hace falta Rust estable.

```
cargo build --workspace
cargo test --workspace
```

## Probar en local

Hace falta una sesión de escritorio con X11, Rust estable con el objetivo `wasm32-unknown-unknown`, `wasm-bindgen-cli` 0.2.129 y Node.js.

```
cargo install wasm-bindgen-cli --version 0.2.129 --locked
rustup target add wasm32-unknown-unknown
cd viewer && npm ci && npm run build && cd ..
cargo run -p wirehaus-agent -- init
cargo run -p wirehaus-agent -- serve --viewer-dir viewer/dist
```

`init` crea la identidad del agente y muestra una vez el secreto del segundo factor para tu aplicación de autenticación. Abre `http://127.0.0.1:7878` en el navegador. La primera vez genera un código con `cargo run -p wirehaus-agent -- pairing` y escríbelo en la página junto al código de seis dígitos de tu aplicación; el navegador queda emparejado y las siguientes veces solo pide el segundo factor.

El agente guarda su estado en `~/.config/wirehaus` (o en `--state-dir`), solo legible por su dueño. `devices` lista los dispositivos emparejados; `devices revoke`, `devices reactivate`, `lock` y `unlock` exigen un código del segundo factor con `--code`. Con `--display` eliges otra pantalla de X11 y con `--listen` otro puerto de la máquina local.

## Licencia

Wirehaus se distribuye bajo la licencia [PolyForm Shield 1.0.0](LICENSE). Puedes usarlo para cualquier fin, también en el trabajo, salvo para ofrecer un producto que compita con él. Es código con fuente disponible, no software libre en el sentido de la OSI.

## Seguridad

Si encuentras una vulnerabilidad, lee [SECURITY.md](SECURITY.md) antes de publicar nada.
