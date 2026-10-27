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
  keyboard: string;
  immersiveOn: string;
  immersiveOff: string;
  layoutLabel: string;
  menu: string;
  typeUnsupported: string;
  zoomReset: string;
  touchTip: string;
  loading: string;
  connecting: string;
  connected: (width: number, height: number) => string;
  closed: string;
  reconnecting: (attempt: number) => string;
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
  filesTruncated: string;
  filesUp: string;
  filesRefresh: string;
  filesNewFolder: string;
  filesFolderName: string;
  filesCreate: string;
  filesUpload: string;
  filesDownload: string;
  filesRemove: string;
  filesConfirmRemove: string;
  filesCancel: string;
  filesColumnName: string;
  filesColumnSize: string;
  filesColumnModified: string;
  filesDownloading: (name: string, percent: number) => string;
  filesUploading: (name: string, percent: number) => string;
  filesFinished: (name: string) => string;
  filesTooBig: (name: string) => string;
  filesFailure: Record<FileFailure, string>;
  filesClosed: string;
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
};

const catalog: Record<string, Messages> = {
  es: {
    homeTitle: 'Entra a tu ordenador',
    homeLead: 'Escribe el nombre de tu acceso para abrir tu escritorio, tu terminal y tus archivos desde el navegador.',
    homeLabel: 'Tu dirección',
    homePlaceholder: 'tu-nombre',
    homeOpen: 'Abrir',
    homeContinue: (name) => `Continuar con ${name}`,
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
    keyboard: 'Teclado',
    immersiveOn: 'Pantalla completa',
    immersiveOff: 'Salir de pantalla completa',
    layoutLabel: 'Teclado del ordenador remoto',
    menu: 'Menú',
    typeUnsupported: 'Ese carácter no se puede escribir con el teclado táctil. Usa la pestaña Portapapeles.',
    zoomReset: 'Ajustar a la pantalla',
    touchTip: 'Toca para hacer clic, mantén pulsado para arrastrar, dos dedos para desplazarte o ampliar.',
    loading: 'Cargando',
    connecting: 'Conectando',
    connected: (width, height) => `Conectado, ${width}x${height}`,
    closed: 'Desconectado.',
    reconnecting: (attempt) => `Conexión perdida. Reconectando, intento ${attempt}.`,
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
      'La terminal está desactivada en el agente. Actívala en el ordenador de casa con "wirehaus-agent terminal allow --code".',
    terminalTooMany: 'Hay demasiadas terminales abiertas.',
    terminalSpawnFailed: 'No se pudo iniciar la terminal.',
    tabFiles: 'Archivos',
    filesNoRoots:
      'No hay carpetas compartidas. Comparte una en el ordenador de casa con "wirehaus-agent files add <carpeta> --code".',
    filesEmpty: 'Carpeta vacía.',
    filesTruncated: 'Solo se muestran los primeros 1000 elementos.',
    filesUp: 'Subir un nivel',
    filesRefresh: 'Actualizar',
    filesNewFolder: 'Nueva carpeta',
    filesFolderName: 'Nombre de la carpeta',
    filesCreate: 'Crear',
    filesUpload: 'Subir archivos',
    filesDownload: 'Descargar',
    filesRemove: 'Borrar',
    filesConfirmRemove: '¿Seguro?',
    filesCancel: 'Cancelar',
    filesColumnName: 'Nombre',
    filesColumnSize: 'Tamaño',
    filesColumnModified: 'Modificado',
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
        'El audio está desactivado. Actívalo en el ordenador de casa con "wirehaus-agent audio allow --code".',
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
        'El portapapeles está desactivado en esa dirección. Actívalo en el ordenador de casa con "wirehaus-agent clipboard allow --code".',
      tooLarge: 'El texto es demasiado grande.',
      unavailable: 'El portapapeles del ordenador remoto no está disponible ahora.',
      timeout: 'El ordenador remoto no ha respondido a tiempo.',
    },
  },
  en: {
    homeTitle: 'Get into your computer',
    homeLead: 'Enter the name of your access to open your desktop, terminal and files from the browser.',
    homeLabel: 'Your address',
    homePlaceholder: 'your-name',
    homeOpen: 'Open',
    homeContinue: (name) => `Continue with ${name}`,
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
    keyboard: 'Keyboard',
    immersiveOn: 'Full screen',
    immersiveOff: 'Leave full screen',
    layoutLabel: 'Remote computer keyboard',
    menu: 'Menu',
    typeUnsupported: 'That character cannot be typed with the touch keyboard. Use the Clipboard tab.',
    zoomReset: 'Fit to screen',
    touchTip: 'Tap to click, press and hold to drag, use two fingers to scroll or zoom.',
    loading: 'Loading',
    connecting: 'Connecting',
    connected: (width, height) => `Connected, ${width}x${height}`,
    closed: 'Disconnected.',
    reconnecting: (attempt) => `Connection lost. Reconnecting, attempt ${attempt}.`,
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
      'The terminal is disabled on the agent. Enable it on the home computer with "wirehaus-agent terminal allow --code".',
    terminalTooMany: 'Too many terminals are open.',
    terminalSpawnFailed: 'The terminal could not be started.',
    tabFiles: 'Files',
    filesNoRoots:
      'No folders are shared. Share one on the home computer with "wirehaus-agent files add <folder> --code".',
    filesEmpty: 'Empty folder.',
    filesTruncated: 'Only the first 1000 items are shown.',
    filesUp: 'Up one level',
    filesRefresh: 'Refresh',
    filesNewFolder: 'New folder',
    filesFolderName: 'Folder name',
    filesCreate: 'Create',
    filesUpload: 'Upload files',
    filesDownload: 'Download',
    filesRemove: 'Delete',
    filesConfirmRemove: 'Sure?',
    filesCancel: 'Cancel',
    filesColumnName: 'Name',
    filesColumnSize: 'Size',
    filesColumnModified: 'Modified',
    filesDownloading: (name, percent) => `Downloading ${name}: ${percent}%`,
    filesUploading: (name, percent) => `Uploading ${name}: ${percent}%`,
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
        'Audio is turned off. Enable it on the home computer with "wirehaus-agent audio allow --code".',
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
        'The clipboard is disabled in that direction. Enable it on the home computer with "wirehaus-agent clipboard allow --code".',
      tooLarge: 'The text is too large.',
      unavailable: 'The remote clipboard is not available right now.',
      timeout: 'The remote computer did not answer in time.',
    },
  },
};

const fallback = 'es';

export function messagesFor(language: string): Messages {
  const base = language.toLowerCase().split('-')[0] ?? fallback;
  return catalog[base] ?? (catalog[fallback] as Messages);
}
