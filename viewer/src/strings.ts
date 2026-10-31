import type { AudioFailure } from './audioprotocol.js';
import type { ClipboardFailure } from './clipboardprotocol.js';
import type { FileFailure } from './filesprotocol.js';

export type Messages = {
  homeTitle: string;
  homeLead: string;
  homeLabel: string;
  homePlaceholder: string;
  homeOpen: string;
  homeContinue: (name: string) => string;
  homeRecent: string;
  linkOr: string;
  linkStart: string;
  linkPcTitle: string;
  linkPcIntro: string;
  linkPcManual: string;
  linkPcVerify: string;
  linkPcWaiting: string;
  linkPcApproved: string;
  linkPcExpired: string;
  linkPcBusy: string;
  linkPcSlow: string;
  linkPcFailed: string;
  linkPcRetry: string;
  linkPcCancel: string;
  linkPcCopy: string;
  linkPcCopied: string;
  linkPcCountdown: (seconds: number) => string;
  linkOpen: string;
  linkIntro: string;
  linkScan: string;
  linkManualLabel: string;
  linkFind: string;
  linkScanDenied: string;
  linkNotFound: string;
  linkOther: (name: string) => string;
  linkInvalid: string;
  linkVerifyText: string;
  linkCodeLabel: string;
  linkCodeHint: string;
  linkApprove: string;
  linkDone: string;
  linkWrongCode: string;
  linkTooMany: string;
  linkGone: string;
  linkUnavailable: string;
  linkClose: string;
  linkNeedsUpdate: string;
  codeRemember: string;
  monitor: string;
  monitorOne: (number: number, primary: boolean) => string;
  monitorAll: string;
  linkScanning: string;
  installApp: string;
  installTitle: string;
  installLead: string;
  installGo: string;
  installStepsIos: string[];
  installStepsOther: string[];
  installSession: string;
  installLater: string;
  installNever: string;
  installClose: string;
  installPairTitle: string;
  installPairLead: string;
  installPairBrowser: string;
  pairScan: string;
  appFirstNote: string;
  linkSameStart: string;
  linkSameTitle: string;
  linkSameIntro: string;

  homeNoAccess: string;
  homeCreate: string;
  homeInvalid: (name: string) => string;
  homeShort: string;
  notFoundTitle: (name: string) => string;
  notFoundDetail: string;
  coolingDetail: string;
  invalidDetail: string;
  reserveName: (name: string) => string;
  searchAnother: string;
  linkDocs: string;
  linkPrivacy: string;
  linkTerms: string;
  offlineTitle: string;
  offlineDetail: (name: string) => string;
  stepPair: string;
  connectedBasic: string;
  stepVerify: string;
  pairHint: string;
  codeHint: string;
  secureNote: string;
  problemTitle: string;
  endedTitle: string;
  reconnectingTitle: string;
  loadingDetail: string;
  connectingDetail: string;
  wakingDetail: string;
  keyboard: string;
  immersiveOn: string;
  immersiveOff: string;
  updateAvailable: string;
  updateReload: string;
  updateLater: string;
  captureOn: string;
  captureUnsupported: string;
  captureExit: string;
  layoutLabel: string;
  languageName: string;
  menu: string;
  typeUnsupported: string;
  zoomReset: string;
  touchTip: string;
  loading: string;
  connecting: string;
  connected: (width: number, height: number) => string;
  closed: string;
  reconnecting: (attempt: number) => string;
  reconnectingSoft: string;
  endedIdle: string;
  endedRevoked: string;
  endedLocked: string;
  reconnect: string;
  forget: string;
  pairTitle: string;
  pairIntro: string;
  pairCodeLabel: string;
  pairNameLabel: string;
  pairNameDefault: string;
  pairSubmit: string;
  pairInvalid: string;
  codeTitle: string;
  codeLabel: string;
  codeSubmit: string;
  wrongCode: string;
  invalidCode: string;
  locked: (seconds: number) => string;
  suspended: string;
  busy: string;
  unavailable: string;
  refused: string;
  failed: string;
  agentMissing: string;
  agentInvalid: string;
  tabDesktop: string;
  newTerminal: string;
  terminalTitle: (number: number) => string;
  closeTab: string;
  terminalExited: (code: number) => string;
  terminalNotAllowed: string;
  terminalTooMany: string;
  terminalSpawnFailed: string;
  tabFiles: string;
  filesNoRoots: string;
  filesEmpty: string;
  filesNoMatches: string;
  filesTruncated: string;
  filesUp: string;
  filesBack: string;
  filesForward: string;
  filesRefresh: string;
  filesPlaces: string;
  filesQuickAccess: string;
  filesSearch: string;
  filesViewGrid: string;
  filesViewList: string;
  filesSortBy: string;
  filesNewFolder: string;
  filesNewFile: string;
  filesNewFolderName: string;
  filesNewFileName: string;
  filesUpload: string;
  filesDownload: string;
  filesOpen: string;
  filesEdit: string;
  filesRename: string;
  filesRemove: string;
  filesCancel: string;
  filesColumnName: string;
  filesColumnSize: string;
  filesColumnModified: string;
  filesItems: (count: number) => string;
  filesSelected: (count: number, size: string) => string;
  filesDropHint: string;
  filesConfirmRemove: (count: number, name: string) => string;
  filesRemoving: (name: string) => string;
  filesMoved: (count: number) => string;
  filesInvalidName: string;
  filesAgentOld: string;
  filesDual: string;
  filesToLocal: string;
  localTitle: string;
  localChoose: string;
  localReopen: (name: string) => string;
  localUpload: string;
  localForget: string;
  localUnsupported: string;
  localNoFolder: string;
  filesDownloading: (name: string, percent: number) => string;
  filesUploading: (name: string, percent: number) => string;
  filesFinished: (name: string) => string;
  filesTooBig: (name: string) => string;
  filesFailure: Record<FileFailure, string>;
  filesClosed: string;
  editorSave: string;
  editorSaved: (name: string) => string;
  editorClose: string;
  editorLines: (count: number) => string;
  editorSelected: (count: number) => string;
  editorLanguage: string;
  editorModified: string;
  editorUnsaved: string;
  editorPosition: (line: number, column: number) => string;
  editorBinary: string;
  editorLoading: string;
  editorChangedMeanwhile: string;
  screenshot: string;
  recordStart: string;
  recordStop: (seconds: number) => string;
  captureSaved: (name: string) => string;
  recordUnsupported: string;
  captureFailed: string;
  recordLimit: string;
  audioListen: string;
  audioStop: string;
  audioStarting: string;
  audioUnsupported: string;
  audioFailure: Record<AudioFailure, string>;
  tabClipboard: string;
  clipboardIntro: string;
  clipboardPlaceholder: string;
  clipboardSend: string;
  clipboardFetch: string;
  clipboardSent: string;
  clipboardReceived: string;
  clipboardCopied: string;
  clipboardEmpty: string;
  clipboardTooLong: (limit: number) => string;
  clipboardNothingToSend: string;
  clipboardFailure: Record<ClipboardFailure, string>;
  clipboardBrowser: string;
  clipboardRemote: string;
  clipboardRemoteEmpty: string;
  clipboardPaste: string;
  clipboardCopy: string;
  clipboardCopiedLocal: string;
  clipboardPasteDenied: string;
  clipboardHistory: string;
  clipboardHistoryEmpty: string;
  clipboardClear: string;
  clipboardToLeft: string;
  clipboardChars: (count: number) => string;
};

const catalog: Record<string, Messages> = {
  es: {
    homeTitle: 'Entra a tu ordenador',
    homeLead: 'Escribe el nombre de tu acceso para abrir tu escritorio, tu terminal y tus archivos desde el navegador.',
    homeLabel: 'Tu dirección',
    homePlaceholder: 'tu-nombre',
    homeOpen: 'Abrir',
    homeContinue: (name) => `Continuar con ${name}`,
    homeRecent: 'Tus equipos',
    linkOr: 'o',
    linkStart: 'Conectar con mi móvil',
    linkPcTitle: 'Conecta con tu móvil',
    linkPcIntro: 'En tu móvil, abre Wirehaus, entra en tu ordenador y pulsa «Conectar dispositivo» en el menú. Escanea este QR. No necesitas escribir nada más.',
    linkPcManual: 'Código',
    linkPcVerify: 'Debe coincidir con tu móvil',
    linkPcWaiting: 'Esperando a tu móvil…',
    linkPcApproved: 'Aprobado. Preparando el emparejamiento…',
    linkPcExpired: 'El código ha caducado. Generando otro…',
    linkPcBusy: 'Hay demasiadas vinculaciones en curso. Espera un minuto y vuelve a intentarlo.',
    linkPcSlow: 'Espera un momento y vuelve a intentarlo.',
    linkPcFailed: 'No se ha podido iniciar la vinculación. Usa el código de emparejamiento.',
    linkPcRetry: 'Generar otro código',
    linkPcCancel: 'Volver',
    linkPcCopy: 'Copiar enlace',
    linkPcCopied: 'Copiado',
    linkPcCountdown: (seconds) => `Caduca en ${seconds} s`,
    linkOpen: 'Conectar dispositivo',
    linkIntro: 'Da acceso a otro ordenador o navegador sin escribir el código largo: ábrelo en esa página, elige «Conectar con mi móvil» y escanea el código que aparece.',
    linkScan: 'Escanear con la cámara',
    linkManualLabel: 'O escribe el código que ves',
    linkFind: 'Continuar',
    linkScanDenied: 'No se ha podido usar la cámara. Escribe el código a mano.',
    linkScanning: 'Apunta la cámara al código del otro equipo.',
    linkNotFound: 'No encuentro ese código. Comprueba que el otro equipo sigue mostrándolo (caduca a los 3 minutos).',
    linkOther: (name) => `Ese código es de otro ordenador (${name}). Conéctate a ese para vincularlo.`,
    linkInvalid: 'Eso no parece un código de Wirehaus.',
    linkVerifyText: 'Comprueba que el otro equipo muestra este número:',
    linkCodeLabel: 'Código de tu aplicación de autenticación',
    linkCodeHint: 'Este ordenador usa una versión antigua del agente y pide el código otra vez.',
    linkApprove: 'Aprobar',
    linkDone: 'Listo. El otro equipo entra solo y la sesión se queda iniciada.',
    linkWrongCode: 'Código incorrecto.',
    linkTooMany: 'Demasiados intentos. Espera un momento.',
    linkGone: 'El código ha caducado. Genera otro en el otro equipo.',
    linkUnavailable: 'No se ha podido completar. Inténtalo de nuevo.',
    linkClose: 'Cerrar',
    monitor: 'Pantalla',
    monitorOne: (number, primary) => `Pantalla ${number}${primary ? ' (principal)' : ''}`,
    monitorAll: 'Todas a la vez',
    codeRemember: 'Mantener la sesión iniciada en este equipo (30 días)',
    linkNeedsUpdate: 'Este ordenador todavía no admite este tipo de código. Se actualizará solo en unas horas; mientras tanto usa el código corto.',
    installApp: 'Instalar la aplicación',
    installTitle: 'Instala Wirehaus',
    installLead: 'Se abre desde tu pantalla de inicio como cualquier app, sin barra del navegador, más rápido y se actualiza sola.',
    installGo: 'Instalar ahora',
    installStepsIos: [
      'Toca el botón Compartir del navegador.',
      'Desliza y elige «Añadir a pantalla de inicio».',
      'Abre Wirehaus desde el icono nuevo.',
    ],
    installStepsOther: [
      'Abre el menú de tu navegador.',
      'Elige «Instalar aplicación» o «Añadir a pantalla de inicio».',
      'Abre Wirehaus desde el icono nuevo.',
    ],
    installSession: 'La app guarda su propia sesión, aparte de la del navegador. La primera vez que la abras tendrás que entrar una vez más; después se queda iniciada.',
    installLater: 'Ahora no',
    installNever: 'No volver a mostrar',
    installClose: 'Cerrar',
    installPairTitle: 'Instala la app para entrar',
    installPairLead: 'Instálala ahora, en menos de un minuto, y se abrirá ya con tu sesión, sin escribir ningún código. Hazlo antes de que pasen 30 minutos.',
    installPairBrowser: 'Entrar solo en el navegador',
    pairScan: 'Escanear el QR de la terminal con la cámara',
    appFirstNote: 'Esta app guarda su propia sesión. Escribe tu dirección y entra una vez: después se queda iniciada.',
    linkSameStart: 'Traer mi sesión del navegador',
    linkSameTitle: 'Trae tu sesión',
    linkSameIntro: 'Abre Wirehaus en el navegador de este móvil, donde ya entraste. Pega este enlace en la barra de direcciones o, en su menú, elige «Conectar dispositivo» y escribe el código. Pulsa Aprobar y la app entrará sola.',
    homeNoAccess: '¿Todavía no tienes acceso?',
    homeCreate: 'Crear el mío',
    homeInvalid: (name) => `«${name}» no es una dirección válida. Usa de 3 a 32 letras minúsculas, cifras o guiones.`,
    homeShort: 'Escribe el nombre de tu acceso.',
    notFoundTitle: (name) => `«${name}» no existe`,
    notFoundDetail: 'Nadie ha reservado todavía esta dirección. Si quieres, puedes reservarla ahora, gratis y sin contraseña.',
    coolingDetail: 'Esta dirección se liberó hace poco y no se puede reservar durante unos días. Prueba con otra.',
    invalidDetail: 'Esa dirección no se puede usar. Prueba con otra: de 3 a 32 letras minúsculas, cifras o guiones.',
    reserveName: (name) => `Reservar ${name}`,
    searchAnother: 'Buscar otra dirección',
    linkDocs: 'Documentación',
    linkPrivacy: 'Privacidad',
    linkTerms: 'Términos',
    offlineTitle: 'No se puede llegar a tu ordenador',
    offlineDetail: (name) =>
      `${name} existe, pero su ordenador no responde. Puede estar apagado, sin conexión o con el agente parado.`,
    stepPair: 'Emparejar',
    connectedBasic: 'Conectado',
    stepVerify: 'Verificar',
    pairHint: 'El código sale al ejecutar «wirehaus-agent pairing» en tu ordenador y caduca a los 10 minutos.',
    codeHint: 'Abre tu aplicación de autenticación y escribe el código de seis dígitos que muestra.',
    secureNote: 'Cifrado de extremo a extremo. El túnel solo ve datos cifrados.',
    problemTitle: 'No se puede conectar',
    endedTitle: 'Sesión terminada',
    reconnectingTitle: 'Reconectando',
    loadingDetail: 'Preparando el cifrado en tu navegador.',
    connectingDetail: 'Estableciendo una conexión cifrada con tu ordenador.',
    wakingDetail: 'Preparando el acceso a tu ordenador. Solo tarda unos segundos la primera vez tras un rato sin usarlo.',
    keyboard: 'Teclado',
    immersiveOn: 'Pantalla completa y teclado',
    immersiveOff: 'Salir de pantalla completa',
    updateAvailable: 'Hay una versión nueva de Wirehaus.',
    updateReload: 'Recargar',
    updateLater: 'Más tarde',
    captureOn: 'Teclado capturado. Pulsa Ctrl derecho para soltarlo.',
    captureUnsupported: 'Este navegador no puede capturar todo el teclado: Esc y algunos atajos seguirán siendo del navegador. Ctrl derecho sale.',
    captureExit: 'Salir (Ctrl derecho)',
    layoutLabel: 'Teclado táctil',
    languageName: 'Idioma',
    menu: 'Menú',
    typeUnsupported: 'Ese carácter no se puede escribir con el teclado táctil. Usa la pestaña Portapapeles.',
    zoomReset: 'Ajustar a la pantalla',
    touchTip: 'Toca para hacer clic, mantén pulsado para arrastrar, dos dedos para desplazarte o ampliar.',
    loading: 'Cargando',
    connecting: 'Conectando',
    connected: (width, height) => `Conectado, ${width}x${height}`,
    closed: 'Desconectado.',
    reconnecting: (attempt) =>
      `Conexión perdida. Reconectando, intento ${attempt}.${attempt >= 3 ? ' Puede que haya poca cobertura o que tu ordenador esté apagado, dormido o sin conexión; seguimos intentándolo sin que tengas que hacer nada.' : ''}`,
    reconnectingSoft: 'Reconectando…',
    endedIdle: 'Sesión cerrada por inactividad. Vuelve a entrar con tu código.',
    endedRevoked: 'Este equipo ya no tiene acceso al ordenador remoto.',
    endedLocked: 'El agente se ha bloqueado desde el ordenador de casa.',
    reconnect: 'Reconectar',
    forget: 'Olvidar este equipo',
    pairTitle: 'Emparejar este equipo',
    pairIntro:
      'Genera un código en el ordenador al que quieres entrar con "wirehaus-agent pairing" y escríbelo aquí. Después te pedirá un código del segundo factor.',
    pairCodeLabel: 'Código de emparejamiento',
    pairNameLabel: 'Nombre de este equipo',
    pairNameDefault: 'Mi navegador',
    pairSubmit: 'Emparejar',
    pairInvalid: 'El código de emparejamiento no es válido.',
    codeTitle: 'Segundo factor',
    codeLabel: 'Código de seis dígitos de tu aplicación',
    codeSubmit: 'Entrar',
    wrongCode: 'El código es incorrecto.',
    invalidCode: 'Escribe los seis dígitos.',
    locked: (seconds) => `Demasiados intentos. Espera ${seconds} segundos y vuelve a probar.`,
    suspended: 'Este equipo está suspendido. Reactívalo desde el ordenador de casa.',
    busy: 'Ya hay otro visor conectado.',
    unavailable: 'El agente no puede capturar la pantalla ni controlar el equipo. Revisa los permisos en el equipo remoto.',
    refused:
      'El agente no ha aceptado la conexión. Puede estar apagado, bloqueado, o no reconocer este equipo. Si lo revocaste o cambió el agente, olvida este equipo y empareja de nuevo.',
    failed: 'La conexión segura ha fallado.',
    agentMissing: 'Falta la dirección del agente. Abre el enlace completo, que termina en #agent=dirección.',
    agentInvalid: 'La dirección del agente no es válida. Debe ser un nombre de equipo con puerto opcional.',
    tabDesktop: 'Escritorio',
    newTerminal: 'Nueva terminal',
    terminalTitle: (number) => `Terminal ${number}`,
    closeTab: 'Cerrar',
    terminalExited: (code) => `[proceso terminado, código ${code}]`,
    terminalNotAllowed:
      'La terminal está desactivada. Actívalo en ese ordenador con "wirehaus-agent allow-all --code 123456" (los seis dígitos de tu app de autenticación).',
    terminalTooMany: 'Hay demasiadas terminales abiertas.',
    terminalSpawnFailed: 'No se pudo iniciar la terminal.',
    tabFiles: 'Archivos',
    filesNoRoots:
      'No hay carpetas compartidas. Actívalo en ese ordenador con "wirehaus-agent allow-all --code 123456" (los seis dígitos de tu app de autenticación).',
    filesEmpty: 'Esta carpeta está vacía.',
    filesNoMatches: 'Nada coincide con la búsqueda.',
    filesTruncated: 'Solo se muestran los primeros 1000 elementos.',
    filesUp: 'Carpeta superior',
    filesBack: 'Atrás',
    filesForward: 'Adelante',
    filesRefresh: 'Actualizar',
    filesPlaces: 'Carpetas compartidas',
    filesQuickAccess: 'Acceso rápido',
    filesSearch: 'Buscar en esta carpeta',
    filesViewGrid: 'Vista de iconos',
    filesViewList: 'Vista de lista',
    filesSortBy: 'Ordenar por',
    filesNewFolder: 'Carpeta nueva',
    filesNewFile: 'Archivo nuevo',
    filesNewFolderName: 'Carpeta nueva',
    filesNewFileName: 'Archivo nuevo.txt',
    filesUpload: 'Subir archivos',
    filesDownload: 'Descargar',
    filesOpen: 'Abrir',
    filesEdit: 'Editar',
    filesRename: 'Cambiar nombre',
    filesRemove: 'Borrar',
    filesCancel: 'Cancelar',
    filesColumnName: 'Nombre',
    filesColumnSize: 'Tamaño',
    filesColumnModified: 'Modificado',
    filesItems: (count) => (count === 1 ? '1 elemento' : `${count} elementos`),
    filesSelected: (count, size) => (count === 1 ? `1 seleccionado${size === '' ? '' : ` (${size})`}` : `${count} seleccionados${size === '' ? '' : ` (${size})`}`),
    filesDropHint: 'Suelta aquí para subir a esta carpeta',
    filesConfirmRemove: (count, name) =>
      count === 1 ? `¿Borrar "${name}"? No se puede deshacer.` : `¿Borrar ${count} elementos? No se puede deshacer.`,
    filesRemoving: (name) => `Borrando ${name}...`,
    filesMoved: (count) => (count === 1 ? 'Movido.' : `${count} elementos movidos.`),
    filesInvalidName: 'Ese nombre no es válido.',
    filesAgentOld: 'Este agente es de una versión anterior: actualízalo para mover, renombrar y editar archivos.',
    filesDual: 'Doble panel',
    filesToLocal: 'Traer al equipo',
    localTitle: 'Este equipo',
    localChoose: 'Elegir carpeta',
    localReopen: (name) => `Reabrir «${name}»`,
    localUpload: 'Subir al remoto',
    localForget: 'Olvidar la carpeta elegida',
    localUnsupported: 'Este navegador no puede explorar carpetas de tu equipo (hace falta Chrome, Edge u Opera). Puedes seguir usando «Subir archivos» y «Descargar».',
    localNoFolder: 'Elige una carpeta de este equipo para empezar a copiar archivos en los dos sentidos.',
    filesDownloading: (name, percent) => `Descargando ${name}: ${percent} %`,
    filesUploading: (name, percent) => `Subiendo ${name}: ${percent} %`,
    filesFinished: (name) => `${name}: terminado.`,
    filesTooBig: (name) => `${name} es demasiado grande para descargarlo en el navegador (máximo 1 GiB).`,
    filesFailure: {
      notAllowed: 'El agente no permite esta operación.',
      noSuchRoot: 'La carpeta compartida ya no existe.',
      invalidPath: 'La ruta no es válida.',
      notFound: 'No existe.',
      alreadyExists: 'Ya existe.',
      notADirectory: 'No es una carpeta.',
      notAFile: 'No es un archivo.',
      notEmpty: 'La carpeta no está vacía.',
      tooLarge: 'El archivo es demasiado grande.',
      busy: 'Hay demasiadas transferencias a la vez.',
      io: 'Ha ocurrido un error en el disco del agente.',
    },
    filesClosed: 'Desconectado.',
    editorSave: 'Guardar',
    editorSaved: (name) => `${name}: guardado.`,
    editorClose: 'Cerrar',
    editorLines: (count) => (count === 1 ? '1 línea' : `${count} líneas`),
    editorSelected: (count) => (count === 1 ? '1 seleccionado' : `${count} seleccionados`),
    editorLanguage: 'Lenguaje',
    editorModified: 'Sin guardar',
    editorUnsaved: 'Hay cambios sin guardar. ¿Cerrar y descartarlos?',
    editorPosition: (line, column) => `Línea ${line}, columna ${column}`,
    editorBinary: 'Este archivo no es de texto y no se puede editar aquí.',
    editorLoading: 'Abriendo...',
    editorChangedMeanwhile: 'El archivo ha cambiado en el ordenador desde que lo abriste. ¿Guardar y sobrescribir esos cambios?',
    screenshot: 'Captura',
    recordStart: 'Grabar',
    recordStop: (seconds) => `Parar (${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, '0')})`,
    captureSaved: (name) => `Guardado: ${name}`,
    recordUnsupported: 'Este navegador no puede grabar vídeo.',
    captureFailed: 'No se pudo guardar.',
    recordLimit: 'Grabación detenida al llegar al límite de 30 minutos o 1 GiB. Se ha guardado lo grabado.',
    audioListen: 'Escuchar',
    audioStop: 'Dejar de escuchar',
    audioStarting: 'Activando el audio...',
    audioUnsupported: 'Este navegador no puede reproducir el audio del ordenador remoto.',
    audioFailure: {
      notAllowed:
        'El audio está desactivado. Actívalo en ese ordenador con "wirehaus-agent allow-all --code 123456" (los seis dígitos de tu app de autenticación).',
      unavailable: 'El ordenador remoto no puede capturar su audio ahora.',
    },
    tabClipboard: 'Portapapeles',
    clipboardIntro:
      'Escribe o pega aquí el texto que quieres llevar al ordenador remoto, o tráete el que hayas copiado allí. Nada se sincroniza solo.',
    clipboardPlaceholder: 'Texto',
    clipboardSend: 'Enviar al ordenador remoto',
    clipboardFetch: 'Traer del ordenador remoto',
    clipboardSent: 'Enviado. Ya puedes pegarlo allí.',
    clipboardReceived: 'Recibido.',
    clipboardCopied: 'Recibido y copiado a tu portapapeles.',
    clipboardEmpty: 'El portapapeles remoto está vacío o no tiene texto.',
    clipboardTooLong: (limit) => `El texto es demasiado largo (máximo ${limit / 1024} KiB).`,
    clipboardNothingToSend: 'No hay texto que enviar.',
    clipboardFailure: {
      notAllowed:
        'El portapapeles está desactivado. Actívalo en ese ordenador con "wirehaus-agent allow-all --code 123456" (los seis dígitos de tu app de autenticación).',
      tooLarge: 'El texto es demasiado grande.',
      unavailable: 'El portapapeles del ordenador remoto no está disponible ahora.',
      timeout: 'El ordenador remoto no ha respondido a tiempo.',
    },
    clipboardBrowser: 'Este navegador',
    clipboardRemote: 'Ordenador remoto',
    clipboardRemoteEmpty: 'Aquí aparece lo que traigas del ordenador remoto.',
    clipboardPaste: 'Pegar',
    clipboardCopy: 'Copiar',
    clipboardCopiedLocal: 'Copiado a tu portapapeles.',
    clipboardPasteDenied: 'El navegador no deja usar tu portapapeles desde aquí: pega con Ctrl+V.',
    clipboardHistory: 'Historial',
    clipboardHistoryEmpty: 'Todavía no hay nada.',
    clipboardClear: 'Vaciar',
    clipboardToLeft: 'Pasar al cuadro de la izquierda',
    clipboardChars: (count) => `${count} caracteres`,
  },
  en: {
    homeTitle: 'Get into your computer',
    homeLead: 'Enter the name of your access to open your desktop, terminal and files from the browser.',
    homeLabel: 'Your address',
    homePlaceholder: 'your-name',
    homeOpen: 'Open',
    homeContinue: (name) => `Continue with ${name}`,
    homeRecent: 'Your computers',
    linkOr: 'or',
    linkStart: 'Connect with my phone',
    linkPcTitle: 'Connect with your phone',
    linkPcIntro: 'On your phone, open Wirehaus, connect to your computer and press "Connect device" in the menu. Scan this QR. You do not need to type anything else.',
    linkPcManual: 'Code',
    linkPcVerify: 'Must match your phone',
    linkPcWaiting: 'Waiting for your phone…',
    linkPcApproved: 'Approved. Preparing the pairing…',
    linkPcExpired: 'The code has expired. Generating another one…',
    linkPcBusy: 'Too many links are in progress. Wait a minute and try again.',
    linkPcSlow: 'Wait a moment and try again.',
    linkPcFailed: 'The link could not be started. Use the pairing code instead.',
    linkPcRetry: 'Generate another code',
    linkPcCancel: 'Back',
    linkPcCopy: 'Copy link',
    linkPcCopied: 'Copied',
    linkPcCountdown: (seconds) => `Expires in ${seconds} s`,
    linkOpen: 'Connect device',
    linkIntro: 'Give another computer or browser access without typing the long code: open this page there, choose "Connect with my phone" and scan the code it shows.',
    linkScan: 'Scan with the camera',
    linkManualLabel: 'Or type the code you see',
    linkFind: 'Continue',
    linkScanDenied: 'The camera could not be used. Type the code by hand.',
    linkScanning: 'Point the camera at the code on the other device.',
    linkNotFound: 'I cannot find that code. Check that the other device is still showing it (it expires after 3 minutes).',
    linkOther: (name) => `That code belongs to another computer (${name}). Connect to that one to link it.`,
    linkInvalid: 'That does not look like a Wirehaus code.',
    linkVerifyText: 'Check that the other device shows this number:',
    linkCodeLabel: 'Code from your authenticator app',
    linkCodeHint: 'This computer runs an older agent and asks for the code again.',
    linkApprove: 'Approve',
    linkDone: 'Done. The other device signs in by itself and the session stays signed in.',
    linkWrongCode: 'Wrong code.',
    linkTooMany: 'Too many attempts. Wait a moment.',
    linkGone: 'The code has expired. Generate another on the other device.',
    linkUnavailable: 'It could not be completed. Try again.',
    linkClose: 'Close',
    monitor: 'Screen',
    monitorOne: (number, primary) => `Screen ${number}${primary ? ' (main)' : ''}`,
    monitorAll: 'All at once',
    codeRemember: 'Keep me signed in on this device (30 days)',
    linkNeedsUpdate: 'This computer does not support this kind of code yet. It will update itself within a few hours; meanwhile use the short code.',
    installApp: 'Install the app',
    installTitle: 'Install Wirehaus',
    installLead: 'It opens from your home screen like any app, without the browser bar, faster, and it updates itself.',
    installGo: 'Install now',
    installStepsIos: [
      'Tap the Share button of the browser.',
      'Scroll and choose "Add to Home Screen".',
      'Open Wirehaus from the new icon.',
    ],
    installStepsOther: [
      'Open your browser menu.',
      'Choose "Install app" or "Add to Home screen".',
      'Open Wirehaus from the new icon.',
    ],
    installSession: 'The app keeps its own session, separate from the browser. The first time you open it you will sign in once more; after that it stays signed in.',
    installLater: 'Not now',
    installNever: 'Do not show again',
    installClose: 'Close',
    installPairTitle: 'Install the app to sign in',
    installPairLead: 'Install it now, in under a minute, and it will open already signed in, without typing any code. Do it within 30 minutes.',
    installPairBrowser: 'Sign in in the browser only',
    pairScan: 'Scan the terminal QR with the camera',
    appFirstNote: 'This app keeps its own session. Enter your address and sign in once: after that it stays signed in.',
    linkSameStart: 'Bring my session from the browser',
    linkSameTitle: 'Bring your session',
    linkSameIntro: 'Open Wirehaus in the browser of this phone, where you already signed in. Paste this link in the address bar or, in its menu, choose "Connect device" and type the code. Tap Approve and the app will sign in by itself.',
    homeNoAccess: "Don't have an access yet?",
    homeCreate: 'Create mine',
    homeInvalid: (name) => `"${name}" is not a valid address. Use 3 to 32 lowercase letters, digits or hyphens.`,
    homeShort: 'Enter the name of your access.',
    notFoundTitle: (name) => `"${name}" does not exist`,
    notFoundDetail: 'Nobody has claimed this address yet. If you like, you can reserve it now, free and without a password.',
    coolingDetail: 'This address was released recently and cannot be reserved for a few days. Try another one.',
    invalidDetail: 'That address cannot be used. Try another: 3 to 32 lowercase letters, digits or hyphens.',
    reserveName: (name) => `Reserve ${name}`,
    searchAnother: 'Look for another address',
    linkDocs: 'Documentation',
    linkPrivacy: 'Privacy',
    linkTerms: 'Terms',
    offlineTitle: 'Cannot reach your computer',
    offlineDetail: (name) =>
      `${name} exists, but its computer does not answer. It may be off, offline or have the agent stopped.`,
    stepPair: 'Pair',
    connectedBasic: 'Connected',
    stepVerify: 'Verify',
    pairHint: 'The code appears when you run "wirehaus-agent pairing" on your computer and expires after 10 minutes.',
    codeHint: 'Open your authenticator app and enter the six-digit code it shows.',
    secureNote: 'End-to-end encrypted. The tunnel only sees encrypted data.',
    problemTitle: 'Cannot connect',
    endedTitle: 'Session ended',
    reconnectingTitle: 'Reconnecting',
    loadingDetail: 'Preparing encryption in your browser.',
    connectingDetail: 'Setting up an encrypted connection to your computer.',
    wakingDetail: 'Getting access to your computer ready. It only takes a few seconds the first time after a while without use.',
    keyboard: 'Keyboard',
    immersiveOn: 'Full screen and keyboard',
    immersiveOff: 'Leave full screen',
    updateAvailable: 'A new version of Wirehaus is available.',
    updateReload: 'Reload',
    updateLater: 'Later',
    captureOn: 'Keyboard captured. Press right Ctrl to release it.',
    captureUnsupported: 'This browser cannot capture the whole keyboard: Esc and some shortcuts stay with the browser. Right Ctrl leaves.',
    captureExit: 'Leave (right Ctrl)',
    layoutLabel: 'Touch keyboard',
    languageName: 'Language',
    menu: 'Menu',
    typeUnsupported: 'That character cannot be typed with the touch keyboard. Use the Clipboard tab.',
    zoomReset: 'Fit to screen',
    touchTip: 'Tap to click, press and hold to drag, use two fingers to scroll or zoom.',
    loading: 'Loading',
    connecting: 'Connecting',
    connected: (width, height) => `Connected, ${width}x${height}`,
    closed: 'Disconnected.',
    reconnecting: (attempt) =>
      `Connection lost. Reconnecting, attempt ${attempt}.${attempt >= 3 ? ' Coverage may be poor, or your computer may be off, asleep or offline; we keep trying, you do not need to do anything.' : ''}`,
    reconnectingSoft: 'Reconnecting…',
    endedIdle: 'Session closed after inactivity. Sign in again with your code.',
    endedRevoked: 'This device no longer has access to the remote computer.',
    endedLocked: 'The agent was locked from the home computer.',
    reconnect: 'Reconnect',
    forget: 'Forget this device',
    pairTitle: 'Pair this device',
    pairIntro:
      'Generate a code on the computer you want to reach with "wirehaus-agent pairing" and type it here. You will then be asked for a second factor code.',
    pairCodeLabel: 'Pairing code',
    pairNameLabel: 'Name of this device',
    pairNameDefault: 'My browser',
    pairSubmit: 'Pair',
    pairInvalid: 'The pairing code is not valid.',
    codeTitle: 'Second factor',
    codeLabel: 'Six-digit code from your authenticator',
    codeSubmit: 'Sign in',
    wrongCode: 'The code is wrong.',
    invalidCode: 'Type all six digits.',
    locked: (seconds) => `Too many attempts. Wait ${seconds} seconds and try again.`,
    suspended: 'This device is suspended. Reactivate it from the home computer.',
    busy: 'Another viewer is already connected.',
    unavailable: 'The agent cannot capture the screen or control the computer. Check its permissions on the remote computer.',
    refused:
      'The agent did not accept the connection. It may be off, locked, or not recognise this device. If you revoked it or the agent changed, forget this device and pair again.',
    failed: 'The secure connection failed.',
    agentMissing: 'The agent address is missing. Open the full link, which ends in #agent=address.',
    agentInvalid: 'The agent address is not valid. It must be a host name with an optional port.',
    tabDesktop: 'Desktop',
    newTerminal: 'New terminal',
    terminalTitle: (number) => `Terminal ${number}`,
    closeTab: 'Close',
    terminalExited: (code) => `[process exited, code ${code}]`,
    terminalNotAllowed:
      'The terminal is turned off. Turn everything on from that computer with "wirehaus-agent allow-all --code 123456" (the six digits from your authenticator app).',
    terminalTooMany: 'Too many terminals are open.',
    terminalSpawnFailed: 'The terminal could not be started.',
    tabFiles: 'Files',
    filesNoRoots:
      'No folders are shared. Turn everything on from that computer with "wirehaus-agent allow-all --code 123456" (the six digits from your authenticator app).',
    filesEmpty: 'This folder is empty.',
    filesNoMatches: 'Nothing matches the search.',
    filesTruncated: 'Only the first 1000 items are shown.',
    filesUp: 'Parent folder',
    filesBack: 'Back',
    filesForward: 'Forward',
    filesRefresh: 'Refresh',
    filesPlaces: 'Shared folders',
    filesQuickAccess: 'Quick access',
    filesSearch: 'Search this folder',
    filesViewGrid: 'Icon view',
    filesViewList: 'List view',
    filesSortBy: 'Sort by',
    filesNewFolder: 'New folder',
    filesNewFile: 'New file',
    filesNewFolderName: 'New folder',
    filesNewFileName: 'New file.txt',
    filesUpload: 'Upload files',
    filesDownload: 'Download',
    filesOpen: 'Open',
    filesEdit: 'Edit',
    filesRename: 'Rename',
    filesRemove: 'Delete',
    filesCancel: 'Cancel',
    filesColumnName: 'Name',
    filesColumnSize: 'Size',
    filesColumnModified: 'Modified',
    filesItems: (count) => (count === 1 ? '1 item' : `${count} items`),
    filesSelected: (count, size) => `${count} selected${size === '' ? '' : ` (${size})`}`,
    filesDropHint: 'Drop here to upload to this folder',
    filesConfirmRemove: (count, name) =>
      count === 1 ? `Delete "${name}"? This cannot be undone.` : `Delete ${count} items? This cannot be undone.`,
    filesRemoving: (name) => `Deleting ${name}...`,
    filesMoved: (count) => (count === 1 ? 'Moved.' : `${count} items moved.`),
    filesInvalidName: 'That name is not valid.',
    filesAgentOld: 'This agent is an older version: update it to move, rename and edit files.',
    filesDual: 'Dual pane',
    filesToLocal: 'Bring to this computer',
    localTitle: 'This computer',
    localChoose: 'Choose folder',
    localReopen: (name) => `Reopen "${name}"`,
    localUpload: 'Upload to remote',
    localForget: 'Forget the chosen folder',
    localUnsupported: 'This browser cannot browse folders on your computer (Chrome, Edge or Opera are needed). You can still use "Upload files" and "Download".',
    localNoFolder: 'Choose a folder on this computer to start copying files both ways.',
    filesDownloading: (name, percent) => `Downloading ${name}: ${percent} %`,
    filesUploading: (name, percent) => `Uploading ${name}: ${percent} %`,
    filesFinished: (name) => `${name}: done.`,
    filesTooBig: (name) => `${name} is too large to download in the browser (1 GiB maximum).`,
    filesFailure: {
      notAllowed: 'The agent does not allow this operation.',
      noSuchRoot: 'The shared folder no longer exists.',
      invalidPath: 'The path is not valid.',
      notFound: 'It does not exist.',
      alreadyExists: 'It already exists.',
      notADirectory: 'It is not a folder.',
      notAFile: 'It is not a file.',
      notEmpty: 'The folder is not empty.',
      tooLarge: 'The file is too large.',
      busy: 'Too many transfers at once.',
      io: 'A disk error occurred on the agent.',
    },
    filesClosed: 'Disconnected.',
    editorSave: 'Save',
    editorSaved: (name) => `${name}: saved.`,
    editorClose: 'Close',
    editorLines: (count) => (count === 1 ? '1 line' : `${count} lines`),
    editorSelected: (count) => `${count} selected`,
    editorLanguage: 'Language',
    editorModified: 'Unsaved',
    editorUnsaved: 'There are unsaved changes. Close and discard them?',
    editorPosition: (line, column) => `Line ${line}, column ${column}`,
    editorBinary: 'This file is not text and cannot be edited here.',
    editorLoading: 'Opening...',
    editorChangedMeanwhile: 'The file changed on the computer since you opened it. Save and overwrite those changes?',
    screenshot: 'Screenshot',
    recordStart: 'Record',
    recordStop: (seconds) => `Stop (${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, '0')})`,
    captureSaved: (name) => `Saved: ${name}`,
    recordUnsupported: 'This browser cannot record video.',
    captureFailed: 'Could not save.',
    recordLimit: 'Recording stopped at the limit of 30 minutes or 1 GiB. What was recorded has been saved.',
    audioListen: 'Listen',
    audioStop: 'Stop listening',
    audioStarting: 'Starting audio...',
    audioUnsupported: 'This browser cannot play the remote computer audio.',
    audioFailure: {
      notAllowed:
        'Audio is turned off. Turn everything on from that computer with "wirehaus-agent allow-all --code 123456" (the six digits from your authenticator app).',
      unavailable: 'The remote computer cannot capture its audio right now.',
    },
    tabClipboard: 'Clipboard',
    clipboardIntro:
      'Type or paste here the text you want to take to the remote computer, or bring back what you copied there. Nothing syncs by itself.',
    clipboardPlaceholder: 'Text',
    clipboardSend: 'Send to the remote computer',
    clipboardFetch: 'Bring from the remote computer',
    clipboardSent: 'Sent. You can paste it there now.',
    clipboardReceived: 'Received.',
    clipboardCopied: 'Received and copied to your clipboard.',
    clipboardEmpty: 'The remote clipboard is empty or has no text.',
    clipboardTooLong: (limit) => `The text is too long (${limit / 1024} KiB maximum).`,
    clipboardNothingToSend: 'There is no text to send.',
    clipboardFailure: {
      notAllowed:
        'The clipboard is turned off. Turn everything on from that computer with "wirehaus-agent allow-all --code 123456" (the six digits from your authenticator app).',
      tooLarge: 'The text is too large.',
      unavailable: 'The remote clipboard is not available right now.',
      timeout: 'The remote computer did not answer in time.',
    },
    clipboardBrowser: 'This browser',
    clipboardRemote: 'Remote computer',
    clipboardRemoteEmpty: 'What you bring from the remote computer appears here.',
    clipboardPaste: 'Paste',
    clipboardCopy: 'Copy',
    clipboardCopiedLocal: 'Copied to your clipboard.',
    clipboardPasteDenied: 'The browser does not let this page use your clipboard: paste with Ctrl+V.',
    clipboardHistory: 'History',
    clipboardHistoryEmpty: 'Nothing here yet.',
    clipboardClear: 'Clear',
    clipboardToLeft: 'Move to the left box',
    clipboardChars: (count) => `${count} characters`,
  },
};

const fallback = 'es';

export function preferredLanguage(browser: string): string {
  try {
    const stored = localStorage.getItem('wirehaus.lang');
    if (stored === 'es' || stored === 'en') {
      return stored;
    }
  } catch {
    return browser;
  }
  return browser;
}

export function messagesFor(language: string): Messages {
  const base = language.toLowerCase().split('-')[0] ?? fallback;
  return catalog[base] ?? (catalog[fallback] as Messages);
}
