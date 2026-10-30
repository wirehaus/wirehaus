# Registro de cambios

Sigue el formato de [Keep a Changelog](https://keepachangelog.com/es-ES/1.1.0/) y [Versionado Semántico](https://semver.org/lang/es/).

## [Sin publicar]

### Cambiado

- El visor reintenta la conexión sin rendirse mientras haya un billete de reanudación (antes paraba a los 30 segundos), vigila los apretones de manos que se quedan colgados con poca cobertura y los repite solo; el agente da 15 segundos (antes 5) para completar el apretón de manos.
- Cerrar la sesión por inactividad ya no revoca el billete: al volver, el equipo recupera la sesión sin pedir el segundo factor.
- El menú «Vincular otro equipo» pasa a llamarse «Conectar dispositivo» y ya no pide el segundo factor al aprobar. Los límites del código corto se relajan (16 pendientes, 8 por dirección) porque ya no hace falta para el QR.
- El instalador ya no enciende terminal, archivos, portapapeles y audio: quedan apagados, como dicen los documentos, y el propio instalador explica cómo encenderlos. Quien quiera la instalación con todo encendido pasa `--allow-all` (`-AllowAll` en Windows, y también al instalador de arranque de `wirehaus.dev`). Los equipos ya instalados conservan lo que tengan activado.

### Seguridad

- El gestor de archivos rechaza siempre, aunque estén dentro de una carpeta compartida, el directorio de estado del agente, el de instalación y las rutas de arranque y credenciales (`~/.ssh`, `~/.gnupg`, los ficheros de inicio de la shell, `autostart`, `systemd/user`, `LaunchAgents` y la carpeta Inicio de Windows). No aparecen en los listados y tampoco se puede entrar por enlaces simbólicos.
- Mover con reemplazo ya no sobrescribe ficheros ocultos ni ejecutables.
- Antes de instalar una actualización se vuelve a comprobar el contenido extraído.
- Las conexiones sin autenticar se limitan por dirección real del cliente (4 a la vez) y el buzón de vinculación admite 2 altas pendientes por dirección.
- El instalador de Windows solo quita la marca de descarga web a los ficheros cuando el paquete llega verificado por el instalador de arranque.

### Corregido

- El QR de «Conectar con mi móvil» podía no salir (sonda de 4 segundos sin reintento, o límites por dirección en redes compartidas como la de un instituto). Ahora se dibuja en el navegador sin depender del agente y la oferta no desaparece si una comprobación falla.
- El billete de reanudación se quemaba al usarlo: si la respuesta se perdía por mala cobertura, el equipo caía al segundo factor. Ahora sigue valiendo diez minutos tras usarse.
- La actualización automática reintenta enseguida (30 s, 1, 2, 4... hasta 30 min) si la comprobación falla por falta de red al arrancar, no se queda dormida cuando el equipo se suspende y no vuelve a bajar un paquete que el agente ya rechazó.

### Añadido

- Sesión recordada: el billete de reanudación dura 30 días, se renueva en cada uso y se guarda en el navegador (opción «Mantener la sesión iniciada en este equipo», activa por defecto). El agente conserva solo su huella en un fichero aparte (`resume`), de modo que sobrevive a reinicios y actualizaciones.
- Vincular un equipo desde el móvil sin servidor intermedio: el equipo nuevo genera su clave en el navegador, enseña el QR al instante (con enlace para copiar y número de comprobación) y el móvil aprueba con un toque, sin segundo factor; el equipo nuevo entra y su sesión queda iniciada. Código corto opcional. Mensajes `ApproveKey` y `ApproveId` del canal de vinculación; `/link/ping` anuncia el nivel 2 y el visor sigue funcionando con agentes anteriores.
- Emparejamiento de confianza: `wirehaus-agent pairing --phone` genera un código de un solo uso, de 30 minutos, que entra sin segundo factor (lo usan el QR del instalador y la aprobación desde el móvil). `pairing --card` y `--days` generan una tarjeta de acceso de hasta 90 días que sí exige el segundo factor.
- `wirehaus-agent onboard`: asistente paso a paso, bilingüe, con móvil o con tarjeta de acceso, que los instaladores de Linux, macOS y Windows lanzan al terminar (`--phone` y `--card` fijan la opción). `init --quiet` crea la identidad sin imprimir el segundo factor.
- Selector de pantallas en el visor: cada pantalla del ordenador o todas a la vez (Linux con X11 y macOS; en Windows solo la principal). El puntero sigue a la pantalla elegida y la lista se actualiza al conectar o desconectar monitores. En macOS, el tamaño de las pantallas Retina usa los píxeles reales.
- Paquete de macOS para Intel (`darwin-x86_64`) además del de Apple Silicon; el instalador, el actualizador y la web los distinguen.
- Aviso para instalar la aplicación en el móvil y recarga automática del visor en segundo plano cuando hay una versión nueva.
- El túnel se apaga tras 30 minutos sin uso (`serve --tunnel-sleep-minutes`, 0 lo desactiva) y se vuelve a crear cuando alguien abre el visor (el servicio solo lo acepta cuando va justo de túneles; mientras haya hueco el túnel se queda como siempre): el agente avisa de que está inactivo, consulta cada pocos segundos mientras duerme y, al despertar, guarda el token nuevo y reinicia el conector (`systemctl --user`, `launchctl` o la tarea de Windows). El visor pide el despertar antes de conectar, enseña «preparando acceso» y reintenta solo; si conectó hace menos de 20 minutos va directo.
- Actualización automática del agente: comprueba cada hora (y 30 segundos después de arrancar) la lista de versiones firmada de `wirehaus.dev`, verifica la firma SSH con la clave fijada en el binario, nunca baja de versión, comprueba el hash del paquete, espera a que no haya sesión, cambia el binario y el visor de forma atómica y se reinicia por el gestor del sistema. Si la versión nueva no llega a arrancar tres veces, vuelve a la anterior y la descarta. Comandos `update status`, `check`, `apply`, `enable`, `disable` y `rollback`. Solo actúa en instalaciones con el diseño del instalador (`serve --no-auto-update` la desactiva).
- El conector del túnel de Cloudflare vuelve a usar su actualizador propio (los instaladores ya no pasan `--no-autoupdate`).
- Varias sesiones a la vez (hasta 5) y varios billetes de reanudación por dispositivo, de un solo uso: se puede abrir el visor en dos pestañas.
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
- Vincular otro equipo desde el móvil, como en WhatsApp Web: el equipo nuevo pulsa «Conectar con mi móvil» y muestra un QR, un código corto y un número de comprobación; el móvil ya conectado escanea el QR con la cámara (o se escribe el código), se comprueba el número y se confirma con un código del segundo factor; el agente abre un emparejamiento y se lo entrega cifrado (Noise N) a la clave efímera del equipo nuevo, que pasa solo al paso del segundo factor. Canal nuevo `Link` del protocolo, tres rutas `/link/ping`, `/link/start` y `/link/poll` con origen comprobado y CORS estricto, como máximo cuatro vinculaciones pendientes de tres minutos, y doce consultas o aprobaciones por sesión. El visor solo ofrece la opción si el agente responde a `/link/ping`.
- QR generado y leído dentro del WebAssembly (`qrcode` y `rqrr`, sin librerías de JavaScript): el visor dibuja el QR en un canvas y lee la cámara fotograma a fotograma.
- Aplicación instalable (PWA): manifiesto, iconos, service worker mínimo que no guarda nada (el visor se descarga y se verifica siempre), entrada «Instalar la aplicación» en el menú, y la aplicación abre directa el último ordenador. Permiso de cámara solo para el propio origen.
- El instalador termina con tres pasos claros (escanear el QR del segundo factor, escanear el de emparejamiento con el móvil, escribir los seis dígitos) y recuerda cómo vincular más equipos.
- Instalador más fácil y a prueba de redes lentas: el conector del túnel se descarga primero de `wirehaus.dev` (copia exacta de la oficial, con sumas fijadas en el instalador) y solo si falla de GitHub; hay barra de progreso, tiempos límite y reintentos; en el comando de una línea el conector se prepara **antes** de gastar el código de un solo uso; los mensajes van en el idioma del sistema y numerados (1/4 a 4/4); en macOS abre solo los Ajustes de Grabación de pantalla y Accesibilidad si faltan; en Windows la consola se pone en UTF-8 para que el QR se vea bien y el script lleva marca de orden de bytes.
- Abrir con la cámara nativa del móvil el enlace del QR de vinculación abre directamente la aprobación cuando el visor ya está conectado, y la pantalla de inicio del visor lista «Tus equipos» (los últimos seis).
- El segundo factor se muestra como un QR escaneable en la terminal (`init`) y vuelve a mostrarse en el propio ordenador con `wirehaus-agent totp`; el código de emparejamiento se muestra también como enlace y QR (`#pair=`) que rellena el formulario del visor sin teclearlo y se borra de la barra de direcciones. El instalador recuerda la dirección del visor en `viewer-url`.

### Corregido

- Tras la revisión independiente de la 0.6: el emparejamiento forzado por enlace ya no se queda activo (un móvil emparejado por QR reconectaba al formulario con un código gastado); el instalador detecta el emparejamiento por el registro de auditoría (no depende del idioma ni de que el dispositivo sea nuevo) y no espera si no hay terminal (`WIREHAUS_NO_WAIT=1` también lo salta); `init --allow-all` enseña antes el segundo factor y trata un fallo al activar funciones como aviso; `boot.js` cancela su recarga cuando la página llega a cargar, oculta su aviso y tiene un contador de respaldo si `sessionStorage` no funciona; el arranque de PowerShell comprueba la respuesta del servicio; reinstalar sobre un estado previo recuerda el comando `allow-all`.
- `wirehaus-agent update ...` lanzado por el enlace de `~/.local/bin` decía «no managed installation was found next to the agent» (miraba junto al enlace y no junto al binario real); ahora resuelve el enlace.
- Instalación sin configurar nada después: el instalador activa archivos (carpeta personal), terminal, portapapeles y audio (`init --allow-all`; `wirehaus-agent allow-all --code SEIS_DÍGITOS` hace lo mismo en un agente ya instalado, y los comandos sueltos para quitar funciones siguen igual), muestra los pasos numerados y **espera a que el móvil se empareje** para avisar «Listo» en la propia terminal. La ayuda de `--code` y el error de código incorrecto dicen que son los seis dígitos de la app de autenticación, no el código de emparejamiento, y el nombre de la etiqueta del segundo factor usa el nombre real del equipo.
- Conexión estable en el móvil: el agente manda un ping de WebSocket cada 20 s para que Cloudflare y los routers no cierren las sesiones quietas; al volver a la pestaña tras unos segundos o recuperar la red, el visor reconecta al momento con el billete (sin pedir el segundo factor) y, mientras tanto, deja la pantalla o los archivos a la vista con un aviso pequeño «Reconectando…» en vez de tapar todo con la pantalla de carga.
- Carga del visor a prueba de despliegues: el aviso de versión nueva solo sale cuando todos los ficheros servidos coinciden con el manifiesto, y si aun así el navegador mezcla versiones (el arranque falla por integridad) la página se recarga sola hasta cuatro veces y, si no, muestra un botón de reintento en lugar de quedarse en negro.
- `wirehaus-agent` sin `--state-dir` usaba `~/.config/wirehaus` mientras el instalador guarda la identidad en `~/Library/Application Support/Wirehaus` (macOS) o `~/.local/share/wirehaus-state` (Linux): `wirehaus-agent pairing` fallaba y un `init` creaba una identidad nueva y aparte, sin QR de emparejamiento. Ahora la carpeta por defecto es la de la instalación si existe (si no, la antigua si ya tenía estado, y si no, la de la instalación).
- Escanear el QR de emparejamiento desde un móvil que ya recordaba ese nombre (de una instalación anterior, con otra clave del agente) intentaba reconectar y salía «El agente no ha aceptado la conexión»; ahora un código en el enlace se impone, empareja solo y pasa al segundo factor, el dispositivo se nombra por el aparato («iPhone · Safari») y un dispositivo que el agente ya conoce puede volver a emparejarse con código y segundo factor (se queda una sola entrada con el nombre nuevo; uno suspendido sigue necesitando `devices reactivate`).
- El instalador comprueba que el túnel conecta de verdad: si no, en macOS y Windows excluye solo el conector de Wirehaus de Mullvad VPN (que bloquea el puerto 7844 de Cloudflare) y, si sigue sin conectar, explica las causas habituales (VPN, cortafuegos de empresa o colegio) en vez de dar un «listo» falso.
- La recarga tras «Hay una versión nueva» dejaba la página rota hasta un hard reload: Cloudflare servía los módulos con `max-age=14400` (Browser Cache TTL de la zona) y el navegador mezclaba un `index.html` nuevo con módulos viejos que fallaban la comprobación de integridad. La zona ahora respeta las cabeceras (`no-cache`) y el despliegue del visor falla si `main.js` vuelve a salir con caché larga.
- Tras la revisión independiente de la vinculación: consultar el estado de una vinculación ya no la borra (el sello sigue disponible hasta que caduca, y aprobar renueva los tres minutos); aprobar comprueba que la petición sigue viva antes de gastar el código del segundo factor, no pisa un emparejamiento que ya estuviera abierto y mantiene el candado durante todo el proceso; una respuesta tardía del agente ya no se entrega a la petición siguiente del móvil.
- Abrir una segunda pestaña o recargar ya no obliga a volver a escribir el segundo factor: presentar el billete de una sesión que el agente aún cree viva la reanuda y cierra la antigua (antes el billete se rechazaba hasta que la sesión vieja caía por tiempo), y una sesión que acaba por un fallo del propio equipo (captura, codificación, entrada) ya no revoca el billete. El agente permite hasta 5 sesiones, espera 45 s antes de dar por atascado a un visor y anota la causa al cerrar cada sesión.

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
