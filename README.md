# Wirehaus

Acceso remoto a tu ordenador desde cualquier navegador: escritorio, terminal, archivos, portapapeles y audio, con cifrado de extremo a extremo.

Dejas el ordenador de casa encendido con el agente de Wirehaus y te conectas desde donde estés, sin instalar nada en el equipo desde el que entras y sin abrir puertos en tu router.

## Estado

Desarrollo temprano. Todavía no hay nada que instalar. Este repositorio contiene el protocolo y el esqueleto del agente.

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
| `crates/agent` | Agente que se ejecuta en el ordenador de casa |

## Compilar

Hace falta Rust estable.

```
cargo build --workspace
cargo test --workspace
```

## Licencia

Wirehaus se distribuye bajo la licencia [PolyForm Shield 1.0.0](LICENSE). Puedes usarlo para cualquier fin, también en el trabajo, salvo para ofrecer un producto que compita con él. Es código con fuente disponible, no software libre en el sentido de la OSI.

## Seguridad

Si encuentras una vulnerabilidad, lee [SECURITY.md](SECURITY.md) antes de publicar nada.
