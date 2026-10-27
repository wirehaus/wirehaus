import {
  encodeTerminalClose,
  encodeTerminalInput,
  encodeTerminalOpen,
  encodeTerminalResize,
} from './protocol.js';
import { AudioControl } from './audio.js';
import { DesktopCapture } from './capture.js';
import { ClipboardPanel } from './clipboard.js';
import { FilesPanel } from './files.js';
import { KeyBar } from './keybar.js';
import type { Layout } from './softkeys.js';
import { TouchLayer } from './touchlayer.js';
import { type Messages, messagesFor } from './strings.js';
import { bridgeTerminalStyles } from './stylebridge.js';
import { Terminals } from './terminals.js';
import { resolveTarget } from './target.js';
import { Viewer, type ViewerState } from './viewer.js';

function element<T extends HTMLElement>(id: string): T {
  const found = document.getElementById(id);
  if (found === null) {
    throw new Error(`missing element #${id}`);
  }
  return found as T;
}

bridgeTerminalStyles();

const messages = messagesFor(navigator.language);
document.documentElement.lang = navigator.language.split('-')[0] ?? 'es';

const coarse = window.matchMedia('(pointer: coarse)').matches;
const phone = window.matchMedia('(pointer: coarse) and (max-width: 720px), (pointer: coarse) and (max-height: 500px)').matches;
let zoomedNow = false;
if (coarse) {
  document.body.classList.add('touch');
}
if (phone) {
  document.body.classList.add('phone');
}

const status = element<HTMLSpanElement>('status');
const agentLabel = element<HTMLSpanElement>('agent');
const forget = element<HTMLButtonElement>('forget');
const reconnect = element<HTMLButtonElement>('reconnect');
const pairPanel = element<HTMLElement>('pair');
const codePanel = element<HTMLElement>('code');
const screen = element<HTMLCanvasElement>('screen');
const immersiveExit = element<HTMLButtonElement>('immersive-exit');
const screenArea = element<HTMLElement>('screen-area');
const splash = element<HTMLElement>('splash');
const splashTitle = element<HTMLElement>('splash-title');
const splashDetail = element<HTMLElement>('splash-detail');
const splashSpinner = element<HTMLElement>('splash-spinner');
const splashIcon = element<HTMLElement>('splash-icon') as unknown as SVGElement;
const splashReconnect = element<HTMLButtonElement>('splash-reconnect');
const splashForget = element<HTMLButtonElement>('splash-forget');
const menuButton = element<HTMLButtonElement>('menu-button');
const menu = element<HTMLElement>('menu');
const layoutSelect = element<HTMLSelectElement>('layout');
const immersive = element<HTMLButtonElement>('immersive');
const keys = element<HTMLElement>('keys');
const softKeys = element<HTMLTextAreaElement>('softkeys');
const keyboardButton = element<HTMLButtonElement>('keyboard');
const zoomReset = element<HTMLButtonElement>('zoom-reset');
const pairSubmit = element<HTMLButtonElement>('pair-submit');
const codeSubmit = element<HTMLButtonElement>('code-submit');
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
  ['step-pair-label', messages.stepPair],
  ['step-verify-label', messages.stepVerify],
  ['step-pair-done-label', messages.stepPair],
  ['step-verify-on-label', messages.stepVerify],
  ['pair-hint', messages.pairHint],
  ['code-hint', messages.codeHint],
  ['secure-pair', messages.secureNote],
  ['secure-code', messages.secureNote],
  ['layout-label', messages.layoutLabel],
  ['keyboard-label', messages.keyboard],
];
for (const [id, text] of labels) {
  element(id).textContent = text;
}
forget.textContent = messages.forget;
splashForget.textContent = messages.forget;
splashReconnect.textContent = messages.reconnect;
immersive.textContent = messages.immersiveOn;
menuButton.setAttribute('aria-label', messages.menu);
zoomReset.setAttribute('aria-label', messages.zoomReset);
zoomReset.title = messages.zoomReset;
reconnect.textContent = messages.reconnect;
pairName.value = messages.pairNameDefault;
pairCode.placeholder = 'XXXX-XXXX-XXXX-XXXX';

function refusal(state: Extract<ViewerState, { kind: 'refused' }>, texts: Messages): string {
  switch (state.reason) {
    case 'locked':
      return texts.locked(state.retryAfter);
    case 'suspended':
      return texts.suspended;
    case 'busy':
      return texts.busy;
    case 'unavailable':
      return texts.unavailable;
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

const audio = new AudioControl(
  element<HTMLButtonElement>('listen'),
  { send: (payload) => viewer.sendAudio(payload) },
  {
    listen: messages.audioListen,
    stop: messages.audioStop,
    starting: messages.audioStarting,
    unsupported: messages.audioUnsupported,
    failure: (reason) => messages.audioFailure[reason],
  },
  (text) => {
    toolsNotice.textContent = text;
  },
);

let lastKind: ViewerState['kind'] = 'loading';

function setError(target: HTMLElement, text: string): void {
  target.textContent = text;
  target.classList.remove('shake');
  if (text !== '') {
    void target.offsetWidth;
    target.classList.add('shake');
  }
}

function paintSplash(state: ViewerState): void {
  const waiting = state.kind === 'loading' || state.kind === 'connecting' || state.kind === 'reconnecting';
  const failed =
    state.kind === 'noAgent' || state.kind === 'refused' || state.kind === 'closed' || state.kind === 'ended';
  splash.hidden = !waiting && !failed;
  splashSpinner.hidden = !waiting;
  splashIcon.toggleAttribute('hidden', !failed);
  const canRetry = state.kind === 'refused' || state.kind === 'closed' || state.kind === 'ended';
  splashReconnect.hidden = !canRetry;
  splashForget.hidden = !(canRetry && state.kind !== 'closed');
  switch (state.kind) {
    case 'loading':
      splashTitle.textContent = messages.loading;
      splashDetail.textContent = messages.loadingDetail;
      break;
    case 'connecting':
      splashTitle.textContent = messages.connecting;
      splashDetail.textContent = messages.connectingDetail;
      break;
    case 'reconnecting':
      splashTitle.textContent = messages.reconnectingTitle;
      splashDetail.textContent = messages.reconnecting(state.attempt);
      break;
    case 'noAgent':
      splashTitle.textContent = messages.problemTitle;
      splashDetail.textContent = state.error === 'missing' ? messages.agentMissing : messages.agentInvalid;
      break;
    case 'refused':
      splashTitle.textContent = messages.problemTitle;
      splashDetail.textContent = refusal(state, messages);
      break;
    case 'closed':
      splashTitle.textContent = messages.endedTitle;
      splashDetail.textContent = messages.closed;
      break;
    case 'ended':
      splashTitle.textContent = messages.endedTitle;
      splashDetail.textContent = endedText(state.reason);
      break;
    default:
      break;
  }
}

function show(state: ViewerState): void {
  lastKind = state.kind;
  document.body.dataset['state'] = state.kind;
  if (state.kind !== 'connected') {
    phoneStarted = false;
  }
  pairPanel.hidden = state.kind !== 'needsPairing';
  codePanel.hidden = state.kind !== 'needsCode';
  paintSplash(state);
  pairSubmit.removeAttribute('aria-busy');
  codeSubmit.removeAttribute('aria-busy');
  tabs.hidden = state.kind !== 'connected';
  if (state.kind !== 'connected') {
    closeMenu();
    document.body.classList.remove('immersive');
    immersiveExit.hidden = true;
  }
  if (state.kind !== 'connected') {
    capture.stop();
    toolsNotice.textContent = '';
  }
  showView();
  const canForget = state.kind !== 'loading' && state.kind !== 'needsPairing' && state.kind !== 'noAgent';
  forget.hidden = !canForget;
  menuButton.hidden = !canForget;
  reconnect.hidden = true;
  setError(pairError, state.kind === 'needsPairing' && state.error !== undefined ? messages.pairInvalid : '');
  setError(
    codeError,
    state.kind === 'needsCode' && state.error === 'wrong'
      ? messages.wrongCode
      : state.kind === 'needsCode' && state.error === 'invalid'
        ? messages.invalidCode
        : '',
  );
  switch (state.kind) {
    case 'loading':
      status.textContent = messages.loading;
      break;
    case 'noAgent':
      status.textContent = state.error === 'missing' ? messages.agentMissing : messages.agentInvalid;
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
      status.textContent = phone ? messages.connectedBasic : messages.connected(state.width, state.height);
      if (phone) {
        if (!phoneStarted) {
          phoneStarted = true;
          selectPanel('files');
        }
      } else {
        maybeTip();
      }
      break;
    case 'refused':
      terminals.endAll();
      files.reset();
      clipboard.reset();
      audio.reset();
      status.textContent = refusal(state, messages);
      break;
    case 'closed':
      terminals.endAll();
      files.reset();
      clipboard.reset();
      audio.reset();
      status.textContent = messages.closed;
      break;
    case 'reconnecting':
      terminals.endAll();
      files.reset();
      clipboard.reset();
      audio.reset();
      status.textContent = messages.reconnecting(state.attempt);
      break;
    case 'ended':
      terminals.endAll();
      files.reset();
      clipboard.reset();
      audio.reset();
      status.textContent = endedText(state.reason);
      break;
  }
}

const configurable =
  document.querySelector('meta[name="wirehaus-agent-address"]')?.getAttribute('content') === 'configurable';
const target = resolveTarget(location, configurable);
if (typeof target === 'object' && target.foreign) {
  agentLabel.textContent = target.authority;
  agentLabel.hidden = false;
}

const viewer = new Viewer(screen, show, target);
viewer.desktopEnabled = !phone;

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
viewer.audioListener = (message) => audio.receive(message);

type Panel = 'desktop' | 'files' | 'clipboard';

let panel: Panel = phone ? 'files' : 'desktop';
let phoneStarted = false;

function showView(): void {
  const connected = lastKind === 'connected';
  const onTerminal = terminals.activeStream() !== null;
  const shows = (wanted: Panel): boolean => connected && !onTerminal && panel === wanted;
  screen.hidden = !shows('desktop');
  screenArea.hidden = !shows('desktop');
  tools.hidden = !shows('desktop');
  filesHost.hidden = !shows('files');
  clipboardHost.hidden = !shows('clipboard');
  terminalHost.hidden = !connected || !onTerminal;
  keys.hidden = !(coarse && connected && (onTerminal || panel === 'desktop'));
  zoomReset.hidden = !(zoomedNow && !onTerminal && panel === 'desktop');
  document.body.classList.toggle('keys-on', !keys.hidden);
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

function icon(name: string): SVGSVGElement {
  const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
  svg.setAttribute('aria-hidden', 'true');
  const use = document.createElementNS('http://www.w3.org/2000/svg', 'use');
  use.setAttribute('href', `#i-${name}`);
  svg.append(use);
  return svg;
}

function tabButton(label: string, selected: boolean, onSelect: () => void, glyph: string): HTMLButtonElement {
  const button = document.createElement('button');
  button.type = 'button';
  button.className = 'tab';
  button.setAttribute('role', 'tab');
  button.setAttribute('aria-selected', String(selected));
  const text = document.createElement('span');
  text.textContent = label;
  button.append(icon(glyph), text);
  button.addEventListener('click', onSelect);
  return button;
}

function renderTabs(): void {
  const active = terminals.activeStream();
  const items: HTMLElement[] = phone
    ? [tabButton(messages.tabFiles, active === null && panel === 'files', () => selectPanel('files'), 'folder')]
    : [
        tabButton(messages.tabDesktop, active === null && panel === 'desktop', () => selectPanel('desktop'), 'desktop'),
        tabButton(messages.tabFiles, active === null && panel === 'files', () => selectPanel('files'), 'folder'),
        tabButton(messages.tabClipboard, active === null && panel === 'clipboard', () => selectPanel('clipboard'), 'clipboard'),
      ];
  for (const stream of terminals.streams()) {
    const wrapper = tabButton(messages.terminalTitle(stream), active === stream, () => selectTerminal(stream), 'terminal');
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
  add.append(icon('plus'));
  add.addEventListener('click', () => terminals.create());
  items.push(add);
  tabs.replaceChildren(...items);
  showView();
}

renderTabs();

function savedLayout(): Layout {
  try {
    const value = localStorage.getItem('wirehaus.layout');
    if (value === 'es' || value === 'us') {
      return value;
    }
  } catch {
    return navigator.language.toLowerCase().startsWith('es') ? 'es' : 'us';
  }
  return navigator.language.toLowerCase().startsWith('es') ? 'es' : 'us';
}

let layout: Layout = savedLayout();
layoutSelect.value = layout;
layoutSelect.addEventListener('change', () => {
  layout = layoutSelect.value === 'us' ? 'us' : 'es';
  try {
    localStorage.setItem('wirehaus.layout', layout);
  } catch {
    return;
  }
});

let toastTimer: number | undefined;
function toast(text: string): void {
  document.querySelector('.toast')?.remove();
  const note = document.createElement('div');
  note.className = 'toast';
  note.setAttribute('role', 'status');
  note.textContent = text;
  document.body.append(note);
  window.clearTimeout(toastTimer);
  toastTimer = window.setTimeout(() => note.remove(), 4500);
}

new KeyBar(keys, softKeys, keyboardButton, {
  target: () => (terminals.activeStream() === null ? 'desktop' : 'terminal'),
  keys: (changes) => viewer.sendKeyChanges(changes),
  terminal: (text) => terminals.sendToActive(text),
  focusTerminal: () => terminals.focusActive(),
  layout: () => layout,
  unsupported: () => toast(messages.typeUnsupported),
});
new TouchLayer(screenArea, viewer);
viewer.zoomListener = (zoomed) => {
  zoomedNow = zoomed;
  showView();
};
zoomReset.addEventListener('click', () => viewer.resetZoom());

function closeMenu(): void {
  menu.hidden = true;
  menuButton.setAttribute('aria-expanded', 'false');
}

menuButton.addEventListener('click', (event) => {
  event.stopPropagation();
  menu.hidden = !menu.hidden;
  menuButton.setAttribute('aria-expanded', String(!menu.hidden));
});
document.addEventListener('click', (event) => {
  if (!menu.hidden && !menu.contains(event.target as Node)) {
    closeMenu();
  }
});
document.addEventListener('keydown', (event) => {
  if (event.key === 'Escape' && !menu.hidden) {
    closeMenu();
  }
});

function setImmersive(on: boolean): void {
  document.body.classList.toggle('immersive', on);
  immersiveExit.hidden = !on;
  immersive.textContent = on ? messages.immersiveOff : messages.immersiveOn;
  if (on) {
    void document.documentElement.requestFullscreen?.().catch(() => undefined);
  } else if (document.fullscreenElement !== null) {
    void document.exitFullscreen().catch(() => undefined);
  }
}
immersive.addEventListener('click', () => {
  closeMenu();
  setImmersive(!document.body.classList.contains('immersive'));
});
immersiveExit.addEventListener('click', () => setImmersive(false));
document.addEventListener('fullscreenchange', () => {
  if (document.fullscreenElement === null && document.body.classList.contains('immersive')) {
    setImmersive(false);
  }
});

codeInput.addEventListener('input', () => {
  const digits = codeInput.value.replace(/\D/g, '').slice(0, 6);
  if (digits !== codeInput.value) {
    codeInput.value = digits;
  }
  if (digits.length === 6) {
    codeForm.requestSubmit();
  }
});

let tipShown = false;
function maybeTip(): void {
  if (!coarse || tipShown) {
    return;
  }
  tipShown = true;
  try {
    if (localStorage.getItem('wirehaus.tip') === '1') {
      return;
    }
    localStorage.setItem('wirehaus.tip', '1');
  } catch {
    return;
  }
  toast(messages.touchTip);
}

pairForm.addEventListener('submit', (event) => {
  event.preventDefault();
  if (pairSubmit.getAttribute('aria-busy') === 'true') {
    return;
  }
  pairSubmit.setAttribute('aria-busy', 'true');
  void viewer.pair(pairCode.value.trim().toUpperCase(), pairName.value.trim());
});
codeForm.addEventListener('submit', (event) => {
  event.preventDefault();
  if (codeSubmit.getAttribute('aria-busy') === 'true') {
    return;
  }
  codeSubmit.setAttribute('aria-busy', 'true');
  viewer.submitCode(codeInput.value.trim());
});
forget.addEventListener('click', () => {
  closeMenu();
  void viewer.forget();
});
splashForget.addEventListener('click', () => void viewer.forget());
reconnect.addEventListener('click', () => void viewer.reconnect());
splashReconnect.addEventListener('click', () => void viewer.reconnect());
void viewer.start();
