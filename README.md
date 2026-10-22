# Wirehaus

Acceso remoto a tu ordenador desde cualquier navegador: escritorio, terminal, archivos, portapapeles y audio, con cifrado de extremo a extremo.

Dejas el ordenador de casa encendido con el agente de Wirehaus y te conectas desde donde estés, sin instalar nada en el equipo desde el que entras y sin abrir puertos en tu router.

## Estado

Desarrollo temprano. Todavía no hay nada que instalar.

El agente ya muestra y controla un escritorio Linux con X11 desde un navegador de la misma máquina. Todavía no hay cifrado de extremo a extremo, segundo factor ni emparejamiento, así que el servidor solo escucha en `127.0.0.1`, rechaza cualquier otra dirección y comprueba `Host` y `Origin` en cada petición. No lo expongas por un túnel ni por la red.

## Qué busca ser

- Escritorio remoto fluido, pensado para equipos modestos.
- Terminal y transferencia de archivos en el mismo navegador, en pestañas.
- Portapapeles compartido, audio, capturas de pantalla y grabación.
- Cifrado de extremo a extremo entre el navegador y tu ordenador, con segundo factor obligatorio.
- Instalación en un solo comando.

## Plataformas

Windows 10 y 11, macOS y Linux (X11).

## Estructura

| Ruta | Contenido |
|---|---|
| `crates/protocol` | Mensajes y codificación del protocolo |
| `crates/capture` | Captura de pantalla y detección de zonas cambiadas |
| `crates/encode` | Codificación de las zonas cambiadas |
| `crates/input` | Inyección de ratón y teclado |
| `crates/agent` | Agente que se ejecuta en el ordenador de casa |
| `web` | Visor que corre en el navegador |

## Compilar

Hace falta Rust estable.

```
cargo build --workspace
cargo test --workspace
```

## Probar en local

Hace falta una sesión de escritorio con X11, Rust estable y Node.js.

```
cd web && npm ci && npm run build && cd ..
cargo run -p wirehaus-agent -- serve --viewer-dir web/dist
```

Abre `http://127.0.0.1:7878` en el navegador. Con `--display` eliges otra pantalla de X11 y con `--listen` otro puerto de la máquina local.

## Licencia

Wirehaus se distribuye bajo la licencia [PolyForm Shield 1.0.0](LICENSE). Puedes usarlo para cualquier fin, también en el trabajo, salvo para ofrecer un producto que compita con él. Es código con fuente disponible, no software libre en el sentido de la OSI.

## Seguridad

Si encuentras una vulnerabilidad, lee [SECURITY.md](SECURITY.md) antes de publicar nada.
