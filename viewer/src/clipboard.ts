import {
  type ClipboardMessage,
  MAX_CLIPBOARD_BYTES,
  clipboardFits,
  encodeClipboardRead,
  encodeClipboardWrite,
} from './clipboardprotocol.js';
import type { Messages } from './strings.js';

export type ClipboardIo = {
  send(stream: number, payload: Uint8Array): void;
};

type Side = 'local' | 'remote';
type Entry = { text: string; at: number };
type Waiting = 'write' | 'read' | null;

const SVG_NS = 'http://www.w3.org/2000/svg';
const HISTORY_KEY = 'wirehaus.clipboard';
const MAX_ENTRIES = 30;
const MAX_STORED_BYTES = 512 * 1024;

function svg(id: string): SVGSVGElement {
  const node = document.createElementNS(SVG_NS, 'svg');
  node.setAttribute('aria-hidden', 'true');
  const use = document.createElementNS(SVG_NS, 'use');
  use.setAttribute('href', `#${id}`);
  node.append(use);
  return node;
}

function element<K extends keyof HTMLElementTagNameMap>(tag: K, className?: string, text?: string): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (className !== undefined) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
}

export function addToHistory(entries: readonly Entry[], text: string, at: number): Entry[] {
  if (text.trim() === '' || entries[0]?.text === text) {
    return [...entries];
  }
  const next = [{ text, at }, ...entries.filter((entry) => entry.text !== text)].slice(0, MAX_ENTRIES);
  let total = 0;
  return next.filter((entry) => {
    total += entry.text.length * 2;
    return total <= MAX_STORED_BYTES;
  });
}

export function previewOf(text: string, limit = 90): string {
  const line = text.split('\n').find((part) => part.trim() !== '') ?? '';
  const flat = line.trim().replace(/\s+/gu, ' ');
  return flat.length > limit ? `${flat.slice(0, limit)}…` : flat;
}

export class ClipboardPanel {
  private readonly areas: Record<Side, HTMLTextAreaElement>;
  private readonly lists: Record<Side, HTMLElement>;
  private readonly sendButton: HTMLButtonElement;
  private readonly fetchButton: HTMLButtonElement;
  private readonly notice: HTMLElement;
  private history: Record<Side, Entry[]> = { local: [], remote: [] };
  private waiting: Waiting = null;
  private stream = 1;

  constructor(
    host: HTMLElement,
    private readonly io: ClipboardIo,
    private readonly texts: Messages,
  ) {
    this.history = this.load();
    const intro = element('p', 'clipboard-intro', texts.clipboardIntro);
    const grid = element('div', 'cb-grid');

    const localArea = element('textarea', 'clipboard-text');
    const remoteArea = element('textarea', 'clipboard-text');
    this.areas = { local: localArea, remote: remoteArea };
    for (const area of [localArea, remoteArea]) {
      area.spellcheck = false;
      area.autocomplete = 'off';
    }
    localArea.placeholder = texts.clipboardPlaceholder;
    localArea.setAttribute('aria-label', texts.clipboardBrowser);
    remoteArea.placeholder = texts.clipboardRemoteEmpty;
    remoteArea.setAttribute('aria-label', texts.clipboardRemote);
    remoteArea.readOnly = true;
    localArea.addEventListener('keydown', (event) => {
      if ((event.ctrlKey || event.metaKey) && event.key === 'Enter') {
        event.preventDefault();
        this.write();
      }
      event.stopPropagation();
    });
    remoteArea.addEventListener('keydown', (event) => event.stopPropagation());

    const paste = this.button('i-clipboard', texts.clipboardPaste, () => void this.pasteFromBrowser());
    const copyLocal = this.button('i-file', texts.clipboardCopy, () => void this.copyToBrowser(localArea.value));
    this.sendButton = this.button('i-right', texts.clipboardSend, () => this.write(), true);
    this.sendButton.classList.add('primary');
    this.fetchButton = this.button('i-left', texts.clipboardFetch, () => this.read(), true);
    this.fetchButton.classList.add('primary');
    const copyRemote = this.button('i-file', texts.clipboardCopy, () => void this.copyToBrowser(remoteArea.value));

    this.lists = { local: element('div', 'cb-history'), remote: element('div', 'cb-history') };
    grid.append(
      this.column('local', texts.clipboardBrowser, 'i-desktop', localArea, [paste, copyLocal, this.sendButton]),
      this.column('remote', texts.clipboardRemote, 'i-terminal', remoteArea, [this.fetchButton, copyRemote]),
    );
    this.notice = element('p', 'clipboard-notice');
    this.notice.setAttribute('role', 'status');
    host.append(intro, grid, this.notice);
    this.renderHistory();
  }

  reset(): void {
    this.waiting = null;
    this.areas.remote.value = '';
    this.notice.textContent = this.texts.closed;
    this.refresh();
  }

  receive(message: ClipboardMessage): void {
    this.waiting = null;
    this.refresh();
    switch (message.kind) {
      case 'written':
        this.say(this.texts.clipboardSent);
        break;
      case 'empty':
        this.say(this.texts.clipboardEmpty);
        break;
      case 'content':
        this.areas.remote.value = message.text;
        this.remember('remote', message.text);
        this.say(this.texts.clipboardReceived);
        void this.copyToBrowser(message.text, true);
        break;
      case 'failed':
        this.say(this.texts.clipboardFailure[message.reason]);
        break;
    }
  }

  private column(side: Side, title: string, icon: string, area: HTMLTextAreaElement, actions: HTMLButtonElement[]): HTMLElement {
    const column = element('section', `cb-col cb-${side}`);
    const head = element('div', 'cb-head');
    head.append(svg(icon), element('h2', undefined, title));
    const bar = element('div', 'clipboard-actions');
    bar.append(...actions);
    const historyHead = element('div', 'cb-history-head');
    historyHead.append(element('span', undefined, this.texts.clipboardHistory));
    const clear = element('button', 'cb-clear', this.texts.clipboardClear);
    clear.type = 'button';
    clear.addEventListener('click', () => {
      this.history[side] = [];
      this.save();
      this.renderHistory();
    });
    historyHead.append(clear);
    column.append(head, area, bar, historyHead, this.lists[side]);
    return column;
  }

  private button(icon: string, label: string, onClick: () => void, withLabel = true): HTMLButtonElement {
    const button = element('button');
    button.type = 'button';
    button.title = label;
    button.setAttribute('aria-label', label);
    button.append(svg(icon));
    if (withLabel) {
      button.append(element('span', undefined, label));
    }
    button.addEventListener('click', onClick);
    return button;
  }

  private write(): void {
    const text = this.areas.local.value;
    if (text === '') {
      this.say(this.texts.clipboardNothingToSend);
      return;
    }
    if (!clipboardFits(text)) {
      this.say(this.texts.clipboardTooLong(MAX_CLIPBOARD_BYTES));
      return;
    }
    this.remember('local', text);
    this.begin('write', encodeClipboardWrite(text));
  }

  private read(): void {
    this.begin('read', encodeClipboardRead());
  }

  private begin(kind: Exclude<Waiting, null>, payload: Uint8Array): void {
    if (this.waiting !== null) {
      return;
    }
    this.waiting = kind;
    this.say('');
    this.refresh();
    this.stream += 1;
    this.io.send(this.stream, payload);
  }

  private async pasteFromBrowser(): Promise<void> {
    try {
      const text = await navigator.clipboard.readText();
      this.areas.local.value = text;
      this.areas.local.focus();
      this.say(text === '' ? this.texts.clipboardNothingToSend : '');
    } catch {
      this.say(this.texts.clipboardPasteDenied);
      this.areas.local.focus();
    }
  }

  private async copyToBrowser(text: string, automatic = false): Promise<void> {
    if (text === '') {
      return;
    }
    try {
      await navigator.clipboard.writeText(text);
      this.say(automatic ? this.texts.clipboardCopied : this.texts.clipboardCopiedLocal);
    } catch {
      this.say(automatic ? this.texts.clipboardReceived : this.texts.clipboardPasteDenied);
    }
  }

  private remember(side: Side, text: string): void {
    this.history[side] = addToHistory(this.history[side], text, Date.now());
    this.save();
    this.renderHistory();
  }

  private renderHistory(): void {
    for (const side of ['local', 'remote'] as const) {
      const entries = this.history[side];
      if (entries.length === 0) {
        this.lists[side].replaceChildren(element('p', 'cb-empty', this.texts.clipboardHistoryEmpty));
        continue;
      }
      this.lists[side].replaceChildren(...entries.map((entry) => this.row(side, entry)));
    }
  }

  private row(side: Side, entry: Entry): HTMLElement {
    const row = element('div', 'cb-entry');
    const use = element('button', 'cb-use');
    use.type = 'button';
    use.title = entry.text.slice(0, 400);
    const time = new Intl.DateTimeFormat(undefined, { hour: '2-digit', minute: '2-digit' }).format(new Date(entry.at));
    use.append(element('span', 'cb-preview', previewOf(entry.text)), element('span', 'cb-time', `${time} · ${this.texts.clipboardChars(entry.text.length)}`));
    use.addEventListener('click', () => {
      this.areas[side].value = entry.text;
      this.areas[side].focus();
    });
    const actions = element('div', 'cb-entry-actions');
    if (side === 'local') {
      actions.append(
        this.button('i-right', this.texts.clipboardSend, () => {
          this.areas.local.value = entry.text;
          this.write();
        }, false),
      );
    } else {
      actions.append(this.button('i-file', this.texts.clipboardCopy, () => void this.copyToBrowser(entry.text), false));
      actions.append(
        this.button('i-left', this.texts.clipboardToLeft, () => {
          this.areas.local.value = entry.text;
          this.areas.local.focus();
        }, false),
      );
    }
    row.append(use, actions);
    return row;
  }

  private load(): Record<Side, Entry[]> {
    try {
      const parsed: unknown = JSON.parse(sessionStorage.getItem(HISTORY_KEY) ?? 'null');
      if (typeof parsed !== 'object' || parsed === null) {
        return { local: [], remote: [] };
      }
      const clean = (value: unknown): Entry[] =>
        Array.isArray(value)
          ? value
              .filter((item): item is Entry => typeof item === 'object' && item !== null && typeof (item as Entry).text === 'string' && typeof (item as Entry).at === 'number')
              .slice(0, MAX_ENTRIES)
          : [];
      const record = parsed as Record<string, unknown>;
      return { local: clean(record['local']), remote: clean(record['remote']) };
    } catch {
      return { local: [], remote: [] };
    }
  }

  private save(): void {
    try {
      sessionStorage.setItem(HISTORY_KEY, JSON.stringify(this.history));
    } catch {
      return;
    }
  }

  private refresh(): void {
    const busy = this.waiting !== null;
    this.sendButton.disabled = busy;
    this.fetchButton.disabled = busy;
  }

  private say(text: string): void {
    this.notice.textContent = text;
  }
}
