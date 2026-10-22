# Registro de cambios

Sigue el formato de [Keep a Changelog](https://keepachangelog.com/es-ES/1.1.0/) y [Versionado Semántico](https://semver.org/lang/es/).

## [Sin publicar]

### Añadido

- Esqueleto del espacio de trabajo con el crate `wirehaus-protocol` y el binario `wirehaus-agent`.
- Codificación y decodificación de tramas con límite de tamaño.
- Canales del protocolo: escritorio, terminal, archivos, portapapeles, audio y captura.
- Integración continua, auditoría de dependencias y análisis de seguridad.

- Captura de pantalla en X11 con detección de zonas cambiadas en teselas de 64 píxeles.
- Eventos de entrada y mensajes de escritorio (cambio de tamaño y teselas codificadas) en el protocolo.
- Inyección de ratón y teclado en X11 mediante XTEST.
- Codificación de teselas en JPEG.
- Servidor local del agente (`wirehaus-agent serve`): solo en la interfaz de bucle local, con comprobación de `Host` y `Origin`, un único visor a la vez y entrega de los ficheros del visor.
- Visor web en TypeScript con lienzo, ratón, rueda y teclado.

### Cambiado

- Las tramas usan una cabecera fija de nueve bytes (canal, flujo y longitud) en lugar de una librería de serialización, lo que deja el protocolo sin dependencias de serialización.
