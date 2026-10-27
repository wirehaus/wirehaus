# Registro de cambios

Sigue el formato de [Keep a Changelog](https://keepachangelog.com/es-ES/1.1.0/) y [Versionado Semántico](https://semver.org/lang/es/).

## [Sin publicar]

### Añadido

- Actualización automática del agente: comprueba cada seis horas la lista de versiones firmada de `wirehaus.dev`, verifica la firma SSH con la clave fijada en el binario, nunca baja de versión, comprueba el hash del paquete, espera a que no haya sesión, cambia el binario y el visor de forma atómica y se reinicia por el gestor del sistema. Si la versión nueva no llega a arrancar tres veces, vuelve a la anterior y la descarta. Comandos `update status`, `check`, `apply`, `enable`, `disable` y `rollback`. Solo actúa en instalaciones con el diseño del instalador (`serve --no-auto-update` la desactiva).
- El conector del túnel de Cloudflare vuelve a usar su actualizador propio (los instaladores ya no pasan `--no-autoupdate`).
- Varias sesiones a la vez (hasta 3) y varios billetes de reanudación por dispositivo, de un solo uso: se puede abrir el visor en dos pestañas.
- El agente anuncia sus capacidades en el listado de carpetas compartidas y el visor desactiva renombrar, mover y editar con agentes anteriores.
- Editor con colores por tipo de archivo (HTML con scripts y estilos incrustados, CSS, JavaScript y TypeScript, JSON, Python, shell, Rust, Go, C y derivados, Java y Kotlin, PHP, SQL, YAML, TOML y Markdown), números de línea, línea actual resaltada, sangría automática, cierre de paréntesis y comillas, comentar con Ctrl+/ y tabuladores de bloque.
- El visor recuerda la pestaña y la carpeta de Archivos al recargar, el idioma se elige en el menú, y el decodificador de vídeo liberado por el navegador en segundo plano provoca una reconexión automática en vez de un error.
- El actualizador prueba el binario nuevo (`info` debe dar la versión anunciada) antes de instalarlo, escribe la marca de periodo de prueba antes de cambiar nada, cuenta los arranques desde el principio de `serve` para poder revertir aunque el nuevo ni arranque, y `update rollback` descarta la versión que estaba en marcha.
- El resaltador tiene un presupuesto de tiempo y no puede bloquear la pestaña con ficheros hostiles; el panel local evita sobrescribir archivos por diferencias de mayúsculas, y las transferencias recuerdan la raíz con que empezaron.
- Portapapeles en dos paneles (este navegador y el ordenador remoto), cada uno con su historial, y botones para pasar el texto de un lado a otro; el historial vive solo en `sessionStorage`.
- Modo de doble panel en Archivos, al estilo del SFTP de Termius: las carpetas de tu equipo a la izquierda (API de acceso a carpetas del navegador, solo Chromium) y el ordenador remoto a la derecha, con subir, traer y arrastrar entre ambos; las descargas se escriben directamente en el archivo local sin pasar por memoria.
- Gestor de archivos del visor al estilo de un escritorio: carpetas compartidas y acceso rápido, migas, vistas de lista y de iconos, orden y filtro, selección múltiple, menú contextual, teclado completo, arrastrar para mover y para subir (también carpetas), creación y renombrado en línea, borrado recursivo con confirmación, vista previa de imágenes y editor de texto integrado que conserva fin de línea, BOM y permisos.
- Operación `Rename` en el protocolo de archivos para mover y renombrar dentro de la carpeta compartida; reemplazar solo vale entre archivos normales y conserva los permisos del destino. Un agente anterior no la entiende.
- Captura de todo el teclado del escritorio remoto con pantalla completa (Keyboard Lock en Chromium); un toque suelto de Ctrl derecho la suelta.
- Al recargar la página la sesión se reanuda sin pedir el segundo factor mientras dure el billete de reanudación (5 minutos tras la desconexión, guardado solo en `sessionStorage`).
- Aviso en el visor cuando hay una versión nueva publicada, con botón para recargar.
- La grabación de pantalla admite MP4 cuando el navegador no puede grabar WebM.

## [0.2.1] - 2026-10-27

### Añadido

- Visor rediseñado: pantallas de emparejamiento, segundo factor y estado con tema claro y oscuro, pasos, envío automático de los seis dígitos y mensajes de error con detalle; menú de cuenta, iconos y barra de pestañas.
- Versión móvil: en teléfonos el visor ofrece solo Archivos (descarga y subida) y Terminal, sin escritorio remoto, y no decodifica ni confirma fotogramas para gastar casi nada de datos. En tabletas y pantallas táctiles el escritorio admite toque para hacer clic, pulsación larga para arrastrar, dos dedos para clic derecho, desplazamiento y zoom, y un teclado táctil con barra de teclas (Esc, Tab, Ctrl, Alt, Shift, flechas) y distribución española o inglesa. La barra de teclas sirve también al terminal.
- Los estilos que inyecta xterm se aplican con una hoja construida por código, de modo que el terminal se ve bien bajo la política de contenido estricta del origen.

### Corregido

- Los instaladores descargan y verifican `cloudflared` antes de quitar el túnel que ya funcionaba, activan `linger` en Linux para que el túnel arranque con el equipo, aceptan líneas en blanco en `SHA256SUMS` y descargan solo por HTTPS.

## [0.2.0] - 2026-10-27

### Añadido

- Instalación en un solo comando: `install.sh` e `install.ps1` aceptan `--tunnel-token-file` / `-TunnelTokenFile`, descargan el conector de túneles (`cloudflared`, versión y suma SHA-256 fijadas en el propio script), lo dejan como servicio de usuario (unidad de systemd, LaunchAgent o tarea programada sin ventana) y lo arrancan. Quitan todo con `--uninstall`. La web publica instaladores de arranque (`install.sh`, `install.ps1`) que canjean un código de un solo uso por el nombre y el token, descargan el paquete y comprueban su suma antes de ejecutar nada.

- Compilación del visor para un origen propio (`npm run build:origin`): la página declara que acepta la dirección del agente en el fragmento, cada hoja de estilo y módulo lleva su huella de integridad (atributo `integrity` y mapa de importación con política de contenido que fija su hash), `_headers` con política `default-src 'none'` y conexiones solo a `'self'` y `wss://*.wirehaus.dev`, `security.txt` y un manifiesto propio de la versión. La orden `wirehaus-agent serve` sigue sirviendo el visor sin esa capacidad.

- Audio del ordenador: canal propio con permiso explícito (`audio allow --code`, `audio deny`), apagado por defecto. El agente captura el audio del sistema (Linux por `parec` sobre PulseAudio o PipeWire; Windows y macOS 14.6 o superior por el bucle de la salida predeterminada con `cpal`: WASAPI y un tap de CoreAudio), lo convierte a 48 kHz estéreo y lo codifica con Opus en tramas de 20 ms usando `opus-pure` (Rust puro, sin dependencia de C, versión fijada). El visor lo decodifica con WebCodecs y lo reproduce con un botón Escuchar, descartando lo que llega tarde para mantener la latencia baja. Si se retira el permiso durante la sesión, el audio se corta.

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

### Cambiado

- La terminal remota abre Windows PowerShell por defecto en Windows (antes `cmd.exe`) y se puede elegir otra shell con la variable de entorno `WIREHAUS_SHELL` en cualquier sistema.

### Corregido

- En Windows, enviar teclas o clics con la pantalla bloqueada (o ante un aviso de control de cuentas) ya no cierra la sesion: el sistema los rechaza por aislamiento de interfaz (UIPI), el agente los descarta y la sesion sigue viva, de modo que al desbloquear se continua sin reconectar. Antes el error terminaba la conexion.

- El visor ya no manda el Ctrl falso que Windows y algunos navegadores inventan al pulsar AltGr (`ControlLeft` y `AltRight` en el mismo instante): escribir `@` o `\u20ac` con un teclado español en un navegador de Windows contra un destino Linux o macOS habría pulsado Ctrl+AltGr. Un Ctrl real se retrasa 5 ms para poder distinguirlo.

- `files add`, `files list` y `files remove` no muestran el prefijo interno `\\?\` de las rutas de Windows.

- En Windows la terminal remota detecta que la shell ha terminado vigilando el proceso hijo: ConPTY no cierra la tubería de salida hasta que se cierra la consola, así que antes teclear `exit` dejaba la pestaña de la terminal abierta para siempre. Los tests de la terminal corren ya también en Windows, contestando a la consulta de posición del cursor (`ESC[6n`) que ConPTY exige.

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
- Las teclas Pausa y F13 a F24 funcionan en Windows (comprobadas con `GetAsyncKeyState`).
- Portapapeles de texto con el sistema (`wirehaus-clipboard`).
- `wirehaus-agent service install`, `status` y `uninstall` en Windows: tarea programada del usuario con inicio de sesión interactivo, relanzada cada minuto, y lanzador sin ventana `wirehaus-agentd` con registro propio.
- `scripts/package-windows.sh` y `scripts/install.ps1`: paquete `.zip` con sumas SHA-256 e instalador por usuario, sin administrador, idempotente, con `-Uninstall` y `-Purge`.
- La captura elige la salida de DXGI que coincide con el monitor principal y falla con un mensaje claro si no hay ninguna, en vez de usar la primera salida del primer adaptador.
- `scripts/build-windows.sh` para compilar desde Linux con mingw, sin dependencias de DLL de mingw en el ejecutable.

### Corregido

- En Windows el directorio de estado por defecto era relativo (`./wirehaus`) porque no existen `HOME` ni `XDG_CONFIG_HOME`; ahora es `%LOCALAPPDATA%\Wirehaus\state`.
- `service uninstall` en Windows se mataba a sí mismo al parar los procesos por nombre; ahora excluye su propio PID.
- Un nombre de pantalla X11 cuyo puerto TCP desborda (por ejemplo `:59999`) hacía entrar en pánico a x11rb; ahora se rechaza con un error.

### Seguridad

- Las carpetas compartidas en Windows rechazan los nombres de dispositivo (`CON`, `NUL`, `COM1`, `LPT1`..., con cualquier extensión) y los caracteres que Windows no admite en un nombre: antes, subir un fichero llamado `NUL` respondía que estaba hecho y descartaba los datos en el dispositivo. Los tests del módulo de archivos corren ya también en Windows (antes eran solo para Unix).

- En Windows el directorio de estado y sus ficheros solo admiten al usuario actual y al sistema (ACL sin herencia), el equivalente de los permisos 700 y 600 de Unix.

- Los handshakes de emparejamiento fallidos ya no queman el código: con 160 bits no frenaban nada y permitían a cualquiera que conociera la dirección anular un emparejamiento en curso. Los fallos del segundo factor siguen quemándolo.
- Todas las respuestas del agente llevan `Cross-Origin-Resource-Policy` y una `Permissions-Policy` que niega al visor cámara, micrófono, geolocalización, pagos y USB.
