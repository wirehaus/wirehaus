export type Messages = {
  loading: string;
  connecting: string;
  connected: (width: number, height: number) => string;
  closed: string;
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
  refused: string;
  failed: string;
  tabDesktop: string;
  newTerminal: string;
  terminalTitle: (number: number) => string;
  closeTab: string;
  terminalExited: (code: number) => string;
  terminalNotAllowed: string;
  terminalTooMany: string;
  terminalSpawnFailed: string;
};

const catalog: Record<string, Messages> = {
  es: {
    loading: 'Cargando',
    connecting: 'Conectando',
    connected: (width, height) => `Conectado, ${width}x${height}`,
    closed: 'Desconectado.',
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
    refused:
      'El agente no ha aceptado la conexión. Puede estar apagado, bloqueado, o no reconocer este equipo. Si lo revocaste o cambió el agente, olvida este equipo y empareja de nuevo.',
    failed: 'La conexión segura ha fallado.',
    tabDesktop: 'Escritorio',
    newTerminal: 'Nueva terminal',
    terminalTitle: (number) => `Terminal ${number}`,
    closeTab: 'Cerrar',
    terminalExited: (code) => `[proceso terminado, código ${code}]`,
    terminalNotAllowed:
      'La terminal está desactivada en el agente. Actívala en el ordenador de casa con "wirehaus-agent terminal allow --code".',
    terminalTooMany: 'Hay demasiadas terminales abiertas.',
    terminalSpawnFailed: 'No se pudo iniciar la terminal.',
  },
  en: {
    loading: 'Loading',
    connecting: 'Connecting',
    connected: (width, height) => `Connected, ${width}x${height}`,
    closed: 'Disconnected.',
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
    refused:
      'The agent did not accept the connection. It may be off, locked, or not recognise this device. If you revoked it or the agent changed, forget this device and pair again.',
    failed: 'The secure connection failed.',
    tabDesktop: 'Desktop',
    newTerminal: 'New terminal',
    terminalTitle: (number) => `Terminal ${number}`,
    closeTab: 'Close',
    terminalExited: (code) => `[process exited, code ${code}]`,
    terminalNotAllowed:
      'The terminal is disabled on the agent. Enable it on the home computer with "wirehaus-agent terminal allow --code".',
    terminalTooMany: 'Too many terminals are open.',
    terminalSpawnFailed: 'The terminal could not be started.',
  },
};

const fallback = 'es';

export function messagesFor(language: string): Messages {
  const base = language.toLowerCase().split('-')[0] ?? fallback;
  return catalog[base] ?? (catalog[fallback] as Messages);
}
