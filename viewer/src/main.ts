import { type Messages, messagesFor } from './strings.js';
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
const forget = element<HTMLButtonElement>('forget');
const reconnect = element<HTMLButtonElement>('reconnect');
const pairPanel = element<HTMLElement>('pair');
const codePanel = element<HTMLElement>('code');
const screen = element<HTMLCanvasElement>('screen');
const pairForm = element<HTMLFormElement>('pair-form');
const pairCode = element<HTMLInputElement>('pair-code');
const pairName = element<HTMLInputElement>('pair-name');
const pairError = element<HTMLElement>('pair-error');
const codeForm = element<HTMLFormElement>('code-form');
const codeInput = element<HTMLInputElement>('code-input');
const codeError = element<HTMLElement>('code-error');

const labels: [string, string][] = [
  ['pair-title', messages.pairTitle],
  ['pair-intro', messages.pairIntro],
  ['pair-code-label', messages.pairCodeLabel],
  ['pair-name-label', messages.pairNameLabel],
  ['pair-submit', messages.pairSubmit],
  ['code-title', messages.codeTitle],
  ['code-label', messages.codeLabel],
  ['code-submit', messages.codeSubmit],
];
for (const [id, text] of labels) {
  element(id).textContent = text;
}
forget.textContent = messages.forget;
reconnect.textContent = messages.reconnect;
pairName.value = messages.pairNameDefault;

function refusal(state: Extract<ViewerState, { kind: 'refused' }>, texts: Messages): string {
  switch (state.reason) {
    case 'locked':
      return texts.locked(state.retryAfter);
    case 'suspended':
      return texts.suspended;
    case 'busy':
      return texts.busy;
    case 'failed':
      return texts.failed;
    case 'unknown':
      return texts.refused;
  }
}

function show(state: ViewerState): void {
  pairPanel.hidden = state.kind !== 'needsPairing';
  codePanel.hidden = state.kind !== 'needsCode';
  screen.hidden = state.kind !== 'connected';
  forget.hidden = state.kind === 'loading' || state.kind === 'needsPairing';
  reconnect.hidden = state.kind !== 'closed' && state.kind !== 'refused';
  pairError.textContent = state.kind === 'needsPairing' && state.error !== undefined ? messages.pairInvalid : '';
  codeError.textContent =
    state.kind === 'needsCode' && state.error === 'wrong'
      ? messages.wrongCode
      : state.kind === 'needsCode' && state.error === 'invalid'
        ? messages.invalidCode
        : '';
  switch (state.kind) {
    case 'loading':
      status.textContent = messages.loading;
      break;
    case 'needsPairing':
      status.textContent = '';
      pairCode.focus();
      break;
    case 'connecting':
      status.textContent = messages.connecting;
      break;
    case 'needsCode':
      status.textContent = '';
      codeInput.value = '';
      codeInput.focus();
      break;
    case 'connected':
      status.textContent = messages.connected(state.width, state.height);
      break;
    case 'refused':
      status.textContent = refusal(state, messages);
      break;
    case 'closed':
      status.textContent = messages.closed;
      break;
  }
}

const viewer = new Viewer(screen, show);

pairForm.addEventListener('submit', (event) => {
  event.preventDefault();
  void viewer.pair(pairCode.value.trim(), pairName.value.trim());
});
codeForm.addEventListener('submit', (event) => {
  event.preventDefault();
  viewer.submitCode(codeInput.value.trim());
});
forget.addEventListener('click', () => void viewer.forget());
reconnect.addEventListener('click', () => void viewer.reconnect());
void viewer.start();
