export type Messages = {
  connecting: string;
  connected: (width: number, height: number) => string;
  closed: string;
  reconnect: string;
};

const catalog: Record<string, Messages> = {
  es: {
    connecting: 'Conectando',
    connected: (width, height) => `Conectado, ${width}x${height}`,
    closed: 'Desconectado. Si no se reconecta, puede haber otro visor abierto.',
    reconnect: 'Reconectar',
  },
  en: {
    connecting: 'Connecting',
    connected: (width, height) => `Connected, ${width}x${height}`,
    closed: 'Disconnected. If it does not reconnect, another viewer may be open.',
    reconnect: 'Reconnect',
  },
};

const fallback = 'es';

export function messagesFor(language: string): Messages {
  const base = language.toLowerCase().split('-')[0] ?? fallback;
  return catalog[base] ?? (catalog[fallback] as Messages);
}
