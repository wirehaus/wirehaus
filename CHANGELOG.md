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
- La captura de pantalla pide a X las zonas dañadas (extensión DAMAGE) y solo lee lo que cambia; sin DAMAGE compara teselas.
- Control de flujo: el visor confirma cada lote de pantalla y el agente limita lo que lleva sin confirmar y adapta la calidad JPEG al enlace.
- Cifrado de extremo a extremo con Noise: reconexión de dispositivos emparejados (`IK`) y emparejamiento de dispositivos nuevos con un código de un solo uso (`XXpsk3`), sobre una capa de registros cifrados con orden, repetición y tamaño controlados.
- Segundo factor TOTP verificado por el agente, con bloqueo creciente por dispositivo, suspensión al décimo fallo y pasos de tiempo de un solo uso.
- Estado del agente en disco con permisos privados, escritura atómica y bloqueo entre procesos, y registro de accesos.
- Órdenes `init`, `pairing`, `devices`, `lock` y `unlock` del agente; revocar, reactivar, bloquear y desbloquear exigen un código del segundo factor.
- El servidor exige autenticarse antes de abrir ningún canal, solo admite un visor autenticado a la vez y corta una sesión en cuanto se revoca el dispositivo o se bloquea el agente.
- El visor web hace el handshake Noise compilado a WebAssembly, pide el código de emparejamiento la primera vez, guarda su identidad y la clave fijada del agente en IndexedDB y pide el segundo factor en cada conexión.
- Vídeo H.264 con OpenH264: cuando hay mucha actividad sostenida (desplazar una página, ver vídeo) el agente pasa a un flujo de vídeo y vuelve a las teselas JPEG, con un refresco completo, cuando la pantalla se calma. Solo se envía a visores que declaran poder decodificarlo; el visor aún no lo hace.
- Visor web en TypeScript con lienzo, ratón, rueda y teclado.
- El visor admite una dirección de agente distinta de la suya, indicada en el fragmento `#agent=`, solo cuando la compilación lo declara; la dirección se valida de forma estricta, se muestra en la cabecera y la clave fijada del agente se guarda por dirección.
- Soporte de macOS: captura de pantalla por sondeo con `CGDisplay`, ratón, teclado y rueda con `CGEvent`, portapapeles con `pbcopy` y `pbpaste`, y la orden `wirehaus-agent permissions` para comprobar y solicitar los permisos de Grabación de pantalla y Accesibilidad. Los modificadores siguen la posición física: Ctrl es Control, la tecla Windows es Option y Alt es Comando.
- El agente suelta las teclas y los botones que el visor dejó pulsados cuando una sesión termina por cualquier causa.
- Sondeo adaptativo de la pantalla en macOS: ritmo completo con actividad y hasta una lectura cada 250 ms en reposo; la entrada del visor lo reactiva.
- Orden `wirehaus-agent service install`, `uninstall` y `status` en macOS: servicio de usuario que arranca al iniciar sesión, se reinicia si cae y solicita sus propios permisos.
- `scripts/macos-sign.sh`: firma el binario con una identidad estable para que macOS conserve los permisos entre versiones.

### Cambiado

- Las tramas usan una cabecera fija de nueve bytes (canal, flujo y longitud) en lugar de una librería de serialización, lo que deja el protocolo sin dependencias de serialización.
