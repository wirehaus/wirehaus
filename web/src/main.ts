import { messagesFor } from './strings.js';
import { Viewer, type ViewerState } from './viewer.js';

function element<T extends HTMLElement>(id: string): T {
  const found = document.getElementById(id);
  if (found === null) {
    throw new Error(`missing element #${id}`);
  }
  return found as T;
}

const messages = messagesFor(navigator.language);
document.documentElement.lang = navigator.language.split('-')[0] ?? 'es';

const status = element<HTMLSpanElement>('status');
const reconnect = element<HTMLButtonElement>('reconnect');
reconnect.textContent = messages.reconnect;

function show(state: ViewerState): void {
  reconnect.hidden = state.kind !== 'closed';
  switch (state.kind) {
    case 'connecting':
      status.textContent = messages.connecting;
      break;
    case 'connected':
      status.textContent = messages.connected(state.width, state.height);
      break;
    case 'closed':
      status.textContent = messages.closed;
      break;
  }
}

const viewer = new Viewer(element<HTMLCanvasElement>('screen'), show);
reconnect.addEventListener('click', () => viewer.connect());
viewer.connect();
