import {
  type ClipboardFailure,
  type ClipboardMessage,
  MAX_CLIPBOARD_BYTES,
  clipboardFits,
  encodeClipboardRead,
  encodeClipboardWrite,
} from './clipboardprotocol.js';

export type ClipboardIo = {
  send(stream: number, payload: Uint8Array): void;
};

export type ClipboardTexts = {
  intro: string;
  placeholder: string;
  send: string;
  fetch: string;
  sent: string;
  received: string;
  copied: string;
  empty: string;
  tooLong: (limit: number) => string;
  nothingToSend: string;
  failure: (reason: ClipboardFailure) => string;
  closed: string;
};

type Waiting = 'write' | 'read' | null;

export class ClipboardPanel {
  private readonly area: HTMLTextAreaElement;
  private readonly sendButton: HTMLButtonElement;
  private readonly fetchButton: HTMLButtonElement;
  private readonly notice: HTMLElement;
  private waiting: Waiting = null;
  private stream = 1;

  constructor(
    host: HTMLElement,
    private readonly io: ClipboardIo,
    private readonly texts: ClipboardTexts,
  ) {
    const intro = document.createElement('p');
    intro.className = 'clipboard-intro';
    intro.textContent = texts.intro;
    this.area = document.createElement('textarea');
    this.area.className = 'clipboard-text';
    this.area.placeholder = texts.placeholder;
    this.area.setAttribute('aria-label', texts.placeholder);
    this.area.spellcheck = false;
    this.area.autocomplete = 'off';
    this.sendButton = this.button(texts.send, () => this.write());
    this.fetchButton = this.button(texts.fetch, () => this.read());
    const actions = document.createElement('div');
    actions.className = 'clipboard-actions';
    actions.append(this.sendButton, this.fetchButton);
    this.notice = document.createElement('p');
    this.notice.className = 'clipboard-notice';
    this.notice.setAttribute('role', 'status');
    host.append(intro, this.area, actions, this.notice);
  }

  reset(): void {
    this.waiting = null;
    this.area.value = '';
    this.notice.textContent = this.texts.closed;
    this.refresh();
  }

  receive(message: ClipboardMessage): void {
    this.waiting = null;
    this.refresh();
    switch (message.kind) {
      case 'written':
        this.say(this.texts.sent);
        break;
      case 'empty':
        this.say(this.texts.empty);
        break;
      case 'content':
        this.area.value = message.text;
        this.say(this.texts.received);
        void this.copyLocally(message.text);
        break;
      case 'failed':
        this.say(this.texts.failure(message.reason));
        break;
    }
  }

  private write(): void {
    const text = this.area.value;
    if (text === '') {
      this.say(this.texts.nothingToSend);
      return;
    }
    if (!clipboardFits(text)) {
      this.say(this.texts.tooLong(MAX_CLIPBOARD_BYTES));
      return;
    }
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

  private async copyLocally(text: string): Promise<void> {
    try {
      await navigator.clipboard.writeText(text);
      this.say(this.texts.copied);
    } catch {
      this.say(this.texts.received);
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

  private button(label: string, onClick: () => void): HTMLButtonElement {
    const button = document.createElement('button');
    button.type = 'button';
    button.textContent = label;
    button.addEventListener('click', onClick);
    return button;
  }
}
