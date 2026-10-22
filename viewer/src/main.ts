import {
  encodeTerminalClose,
  encodeTerminalInput,
  encodeTerminalOpen,
  encodeTerminalResize,
} from './protocol.js';
import { DesktopCapture } from './capture.js';
import { ClipboardPanel } from './clipboard.js';
import { FilesPanel } from './files.js';
import { type Messages, messagesFor } from './strings.js';
import { Terminals } from './terminals.js';
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
const tabs = element<HTMLElement>('tabs');
const tools = element<HTMLElement>('tools');
const toolsNotice = element<HTMLElement>('tools-notice');
const terminalHost = element<HTMLElement>('terminals');
const filesHost = element<HTMLElement>('files');
const clipboardHost = element<HTMLElement>('clipboard');
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

function endedText(reason: 'idle' | 'revoked' | 'locked'): string {
  switch (reason) {
    case 'idle':
      return messages.endedIdle;
    case 'revoked':
      return messages.endedRevoked;
    case 'locked':
      return messages.endedLocked;
  }
}

const capture = new DesktopCapture(
  screen,
  element<HTMLButtonElement>('shot'),
  element<HTMLButtonElement>('record'),
  {
    screenshot: messages.screenshot,
    recordStart: messages.recordStart,
    recordStop: messages.recordStop,
    saved: messages.captureSaved,
    unsupported: messages.recordUnsupported,
    failed: messages.captureFailed,
    limit: messages.recordLimit,
  },
  (text) => {
    toolsNotice.textContent = text;
  },
);

let lastKind: ViewerState['kind'] = 'loading';

function show(state: ViewerState): void {
  lastKind = state.kind;
  pairPanel.hidden = state.kind !== 'needsPairing';
  codePanel.hidden = state.kind !== 'needsCode';
  tabs.hidden = state.kind !== 'connected';
  if (state.kind !== 'connected') {
    capture.stop();
    toolsNotice.textContent = '';
  }
  showView();
  forget.hidden = state.kind === 'loading' || state.kind === 'needsPairing';
  reconnect.hidden = state.kind !== 'closed' && state.kind !== 'refused' && state.kind !== 'ended';
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
      terminals.endAll();
      files.reset();
      clipboard.reset();
      status.textContent = refusal(state, messages);
      break;
    case 'closed':
      terminals.endAll();
      files.reset();
      clipboard.reset();
      status.textContent = messages.closed;
      break;
    case 'reconnecting':
      terminals.endAll();
      files.reset();
      clipboard.reset();
      status.textContent = messages.reconnecting(state.attempt);
      break;
    case 'ended':
      terminals.endAll();
      files.reset();
      clipboard.reset();
      status.textContent = endedText(state.reason);
      break;
  }
}

const viewer = new Viewer(screen, show);

function refusedText(reason: 'notAllowed' | 'tooManySessions' | 'spawnFailed'): string {
  switch (reason) {
    case 'notAllowed':
      return messages.terminalNotAllowed;
    case 'tooManySessions':
      return messages.terminalTooMany;
    case 'spawnFailed':
      return messages.terminalSpawnFailed;
  }
}

const terminals = new Terminals(
  terminalHost,
  {
    open: (stream, columns, rows) => viewer.sendTerminal(stream, encodeTerminalOpen(columns, rows)),
    input: (stream, data) => viewer.sendTerminal(stream, encodeTerminalInput(data)),
    resize: (stream, columns, rows) => viewer.sendTerminal(stream, encodeTerminalResize(columns, rows)),
    close: (stream) => viewer.sendTerminal(stream, encodeTerminalClose()),
  },
  { exited: messages.terminalExited, refused: refusedText },
  renderTabs,
);
viewer.terminalListener = (stream, message) => terminals.receive(stream, message);

const filesTexts = messages;
const files = new FilesPanel(
  filesHost,
  { send: (stream, payload) => viewer.sendFiles(stream, payload), buffered: () => viewer.bufferedBytes() },
  {
    noRoots: filesTexts.filesNoRoots,
    empty: filesTexts.filesEmpty,
    truncated: filesTexts.filesTruncated,
    up: filesTexts.filesUp,
    refresh: filesTexts.filesRefresh,
    newFolder: filesTexts.filesNewFolder,
    folderName: filesTexts.filesFolderName,
    create: filesTexts.filesCreate,
    upload: filesTexts.filesUpload,
    download: filesTexts.filesDownload,
    remove: filesTexts.filesRemove,
    confirmRemove: filesTexts.filesConfirmRemove,
    cancel: filesTexts.filesCancel,
    columnName: filesTexts.filesColumnName,
    columnSize: filesTexts.filesColumnSize,
    columnModified: filesTexts.filesColumnModified,
    downloading: filesTexts.filesDownloading,
    uploading: filesTexts.filesUploading,
    finished: filesTexts.filesFinished,
    tooBig: filesTexts.filesTooBig,
    failure: (reason) => filesTexts.filesFailure[reason],
    closed: filesTexts.filesClosed,
  },
);
viewer.filesListener = (stream, message) => files.receive(stream, message);

const clipboard = new ClipboardPanel(
  clipboardHost,
  { send: (stream, payload) => viewer.sendClipboard(stream, payload) },
  {
    intro: messages.clipboardIntro,
    placeholder: messages.clipboardPlaceholder,
    send: messages.clipboardSend,
    fetch: messages.clipboardFetch,
    sent: messages.clipboardSent,
    received: messages.clipboardReceived,
    copied: messages.clipboardCopied,
    empty: messages.clipboardEmpty,
    tooLong: messages.clipboardTooLong,
    nothingToSend: messages.clipboardNothingToSend,
    failure: (reason) => messages.clipboardFailure[reason],
    closed: messages.closed,
  },
);
viewer.clipboardListener = (message) => clipboard.receive(message);

type Panel = 'desktop' | 'files' | 'clipboard';

let panel: Panel = 'desktop';

function showView(): void {
  const connected = lastKind === 'connected';
  const onTerminal = terminals.activeStream() !== null;
  const shows = (wanted: Panel): boolean => connected && !onTerminal && panel === wanted;
  screen.hidden = !shows('desktop');
  tools.hidden = !shows('desktop');
  filesHost.hidden = !shows('files');
  clipboardHost.hidden = !shows('clipboard');
  terminalHost.hidden = !connected || !onTerminal;
}

function selectPanel(wanted: Panel): void {
  panel = wanted;
  terminals.activate(null);
  if (wanted === 'files') {
    files.show();
  }
}

function selectDesktop(): void {
  selectPanel('desktop');
}

function selectTerminal(stream: number): void {
  terminals.activate(stream);
}

function tabButton(label: string, selected: boolean, onSelect: () => void): HTMLButtonElement {
  const button = document.createElement('button');
  button.type = 'button';
  button.className = 'tab';
  button.setAttribute('role', 'tab');
  button.setAttribute('aria-selected', String(selected));
  button.textContent = label;
  button.addEventListener('click', onSelect);
  return button;
}

function renderTabs(): void {
  const active = terminals.activeStream();
  const items: HTMLElement[] = [
    tabButton(messages.tabDesktop, active === null && panel === 'desktop', () => selectPanel('desktop')),
    tabButton(messages.tabFiles, active === null && panel === 'files', () => selectPanel('files')),
    tabButton(messages.tabClipboard, active === null && panel === 'clipboard', () => selectPanel('clipboard')),
  ];
  for (const stream of terminals.streams()) {
    const wrapper = tabButton(messages.terminalTitle(stream), active === stream, () => selectTerminal(stream));
    const close = document.createElement('span');
    close.className = 'close';
    close.setAttribute('role', 'button');
    close.setAttribute('aria-label', messages.closeTab);
    close.textContent = '\u00d7';
    close.addEventListener('click', (event) => {
      event.stopPropagation();
      terminals.close(stream);
      if (terminals.activeStream() === null) {
        selectDesktop();
      }
    });
    wrapper.append(close);
    items.push(wrapper);
  }
  const add = document.createElement('button');
  add.type = 'button';
  add.className = 'add';
  add.setAttribute('aria-label', messages.newTerminal);
  add.title = messages.newTerminal;
  add.textContent = '+';
  add.addEventListener('click', () => terminals.create());
  items.push(add);
  tabs.replaceChildren(...items);
  showView();
}

renderTabs();

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
