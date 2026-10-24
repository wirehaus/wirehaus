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
- Motivo de rechazo `Unavailable`: el visor autenticado sabe que el agente no puede capturar la pantalla o controlar el equipo, y lo muestra con una indicación en vez de un fallo genérico de conexión.
- `scripts/macos-sign.sh`: firma el binario con una identidad estable para que macOS conserve los permisos entre versiones.

### Añadido

- Los secretos del agente (clave privada y secreto del segundo factor) pueden guardarse en el almacén de claves del sistema en lugar del fichero de estado: `wirehaus-agent secrets status`, `secrets use system` y `secrets use file --code`. En Linux usa `secret-tool`; en macOS y Windows, el Llavero y el Administrador de credenciales. Es opcional y el fichero sigue siendo el valor por defecto.

### Cambiado

- Las tramas usan una cabecera fija de nueve bytes (canal, flujo y longitud) en lugar de una librería de serialización, lo que deja el protocolo sin dependencias de serialización.

### Cambiado

- Los mensajes de las órdenes del agente (`init`, `pairing`, `devices`, `files`, `service` y el resto) y sus errores salen en español o en inglés según `LC_ALL`, `LC_MESSAGES` o `LANG`, con el inglés por defecto. Antes mezclaban español sin tildes e inglés.

### Corregido

- En Windows `serve` comprueba al arrancar que puede capturar la pantalla y manejar la entrada, y si no (SSH, servicio de Windows, pantalla bloqueada) se niega con un mensaje que explica que hace falta la sesión de escritorio del usuario.

- En Windows la captura espera a los fotogramas de DXGI en lugar de sondear, y la sesión sobrevive a la pantalla de bloqueo y al aviso de control de cuentas reintentando cada segundo.

- Un error transitorio al aceptar una conexión ya no detiene el servidor del agente: se registra y se reintenta, y solo cincuenta fallos seguidos lo paran.
- Retirar el permiso de terminal cierra las terminales abiertas en un segundo y el visor recibe el aviso; antes solo impedía abrir otras nuevas.
- Un número de flujo de terminal reutilizado ya no hereda la salida ni el cierre del anterior, y el agente deja de esperar sin límite a un intérprete que ha soltado su terminal sin terminar.
- Descargar una tubería con nombre u otro archivo especial de una carpeta compartida se rechaza en vez de bloquear la sesión.
- El puntero sigue el tamaño real de la pantalla cuando cambia la resolución durante la sesión.
- Tras un fallo de decodificación de vídeo, el visor vuelve a reproducir vídeo en la conexión siguiente en lugar de quedarse con la imagen congelada.
- Las órdenes `files add`, `devices revoke` y `devices reactivate` comprueban el destino antes de gastar el código del segundo factor.
- La depuración de los mensajes de control no muestra códigos ni billetes, y el generador de claves borra la clave privada que le entrega la librería.
- El bloqueo del fichero de estado usa el bloqueo del sistema en lugar de un fichero que había que detectar como caducado: si un proceso muere con el bloqueo, se libera al instante y ya no hay una espera de 30 segundos en la que fallaban las autenticaciones.
- El visor se compila también en rutas con espacios y en Windows.

### Windows

- Captura con DXGI Desktop Duplication (`wirehaus-capture`), con reconexión cuando se pierde el acceso a la salida.
- Entrada de ratón y teclado por código de escaneo y posición física (`wirehaus-input`), con rueda en los dos ejes y liberación de teclas y botones al cerrar la sesión.
- Portapapeles de texto con el sistema (`wirehaus-clipboard`).
- `wirehaus-agent service install`, `status` y `uninstall` en Windows: tarea programada del usuario con inicio de sesión interactivo, relanzada cada minuto, y lanzador sin ventana `wirehaus-agentd` con registro propio.
- `scripts/package-windows.sh` y `scripts/install.ps1`: paquete `.zip` con sumas SHA-256 e instalador por usuario, sin administrador, idempotente, con `-Uninstall` y `-Purge`.
- La captura elige la salida de DXGI que coincide con el monitor principal y falla con un mensaje claro si no hay ninguna, en vez de usar la primera salida del primer adaptador.
- `scripts/build-windows.sh` para compilar desde Linux con mingw, sin dependencias de DLL de mingw en el ejecutable.

### Corregido

- En Windows el directorio de estado por defecto era relativo (`./wirehaus`) porque no existen `HOME` ni `XDG_CONFIG_HOME`; ahora es `%LOCALAPPDATA%\Wirehaus\state`.
- `service uninstall` en Windows se mataba a sí mismo al parar los procesos por nombre; ahora excluye su propio PID.

### Seguridad

- En Windows el directorio de estado y sus ficheros solo admiten al usuario actual y al sistema (ACL sin herencia), el equivalente de los permisos 700 y 600 de Unix.

- Los handshakes de emparejamiento fallidos ya no queman el código: con 160 bits no frenaban nada y permitían a cualquiera que conociera la dirección anular un emparejamiento en curso. Los fallos del segundo factor siguen quemándolo.
- Todas las respuestas del agente llevan `Cross-Origin-Resource-Policy` y una `Permissions-Policy` que niega al visor cámara, micrófono, geolocalización, pagos y USB.
