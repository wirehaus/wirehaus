import type { FileEntry } from './filesprotocol.js';
import { CodeArea } from './codearea.js';
import { type Language, LANGUAGES, detectLanguage, languageLabel } from './highlight.js';
import { looksBinary, mimeFor } from './filetypes.js';
import type { Messages } from './strings.js';

export type DialogOps = {
  read(path: string): Promise<Uint8Array | null>;
  write(path: string, data: Uint8Array): Promise<boolean>;
  stat(path: string): Promise<FileEntry | null>;
  download(path: string, entry: FileEntry): void;
  say(text: string): void;
  confirm(message: string, okLabel: string, danger: boolean): Promise<boolean>;
  saved(): void;
};

const SVG_NS = 'http://www.w3.org/2000/svg';

function icon(id: string): SVGSVGElement {
  const node = document.createElementNS(SVG_NS, 'svg');
  node.setAttribute('aria-hidden', 'true');
  const use = document.createElementNS(SVG_NS, 'use');
  use.setAttribute('href', `#${id}`);
  node.append(use);
  return node;
}

function iconButton(id: string, label: string, text?: string): HTMLButtonElement {
  const button = document.createElement('button');
  button.type = 'button';
  button.title = label;
  button.setAttribute('aria-label', label);
  button.append(icon(id));
  if (text !== undefined) {
    const span = document.createElement('span');
    span.className = 'lbl';
    span.textContent = text;
    button.append(span);
  }
  return button;
}

function readAsDataUrl(blob: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result));
    reader.onerror = () => reject(reader.error ?? new Error('read failed'));
    reader.readAsDataURL(blob);
  });
}

export class FileDialog {
  private readonly overlay: HTMLElement;
  private readonly dialog: HTMLElement;
  private opened = false;
  private dirty = false;
  private crlf = false;
  private bom = false;
  private current: { path: string; entry: FileEntry } | null = null;
  private baseline: FileEntry | null = null;
  private saving = false;
  private generation = 0;
  private code: CodeArea | null = null;
  private lines: HTMLElement | null = null;
  private saveButton: HTMLButtonElement | null = null;
  private dirtyBadge: HTMLElement | null = null;
  private position: HTMLElement | null = null;

  constructor(
    host: HTMLElement,
    private readonly texts: Messages,
    private readonly ops: DialogOps,
  ) {
    this.overlay = document.createElement('div');
    this.overlay.className = 'fm-modal';
    this.overlay.hidden = true;
    this.dialog = document.createElement('div');
    this.dialog.className = 'fm-dialog';
    this.dialog.setAttribute('role', 'dialog');
    this.dialog.setAttribute('aria-modal', 'true');
    this.overlay.append(this.dialog);
    host.append(this.overlay);
    this.overlay.addEventListener('keydown', (event) => this.keydown(event));
  }

  get isOpen(): boolean {
    return this.opened;
  }

  async edit(path: string, entry: FileEntry): Promise<void> {
    const generation = this.begin(entry.name);
    const body = this.dialog.querySelector('.fm-dialog-body');
    const note = document.createElement('p');
    note.className = 'fm-dialog-note';
    note.textContent = this.texts.editorLoading;
    body?.append(note);
    const bytes = await this.ops.read(path);
    if (generation !== this.generation) {
      return;
    }
    if (bytes === null) {
      this.dismiss();
      return;
    }
    if (looksBinary(bytes)) {
      this.dismiss();
      this.ops.say(this.texts.editorBinary);
      return;
    }
    let text: string;
    try {
      text = new TextDecoder('utf-8', { fatal: true, ignoreBOM: true }).decode(bytes);
    } catch {
      this.dismiss();
      this.ops.say(this.texts.editorBinary);
      return;
    }
    this.bom = text.startsWith('\ufeff');
    const content = this.bom ? text.slice(1) : text;
    const breaks = content.split('\n').length - 1;
    this.crlf = breaks > 0 && content.split('\r\n').length - 1 === breaks;
    this.current = { path, entry };
    this.baseline = entry;
    this.buildEditor(this.crlf ? content.replaceAll('\r\n', '\n') : content, entry);
  }

  async preview(path: string, entry: FileEntry): Promise<void> {
    const generation = this.begin(entry.name);
    const body = this.dialog.querySelector('.fm-dialog-body');
    const bytes = await this.ops.read(path);
    if (generation !== this.generation) {
      return;
    }
    if (bytes === null) {
      this.dismiss();
      return;
    }
    const url = await readAsDataUrl(new Blob([bytes as BlobPart], { type: mimeFor(entry.name) }));
    if (generation !== this.generation) {
      return;
    }
    const image = document.createElement('img');
    image.className = 'fm-preview';
    image.alt = entry.name;
    image.src = url;
    body?.replaceChildren(image);
    const download = iconButton('i-down', this.texts.filesDownload, this.texts.filesDownload);
    download.addEventListener('click', () => this.ops.download(path, entry));
    this.dialog.querySelector('.fm-dialog-actions')?.prepend(download);
  }

  async requestClose(): Promise<void> {
    if (!this.opened) {
      return;
    }
    if (this.dirty && !(await this.ops.confirm(this.texts.editorUnsaved, this.texts.editorClose, true))) {
      return;
    }
    this.dismiss();
  }

  close(): void {
    this.dismiss();
  }

  private begin(name: string): number {
    this.dismiss();
    this.generation += 1;
    this.opened = true;
    this.dirty = false;
    this.dialog.replaceChildren();
    const head = document.createElement('div');
    head.className = 'fm-dialog-head';
    const title = document.createElement('h2');
    title.textContent = name;
    this.dirtyBadge = document.createElement('span');
    this.dirtyBadge.className = 'fm-badge';
    this.dirtyBadge.textContent = this.texts.editorModified;
    this.dirtyBadge.hidden = true;
    const actions = document.createElement('div');
    actions.className = 'fm-dialog-actions';
    const close = iconButton('i-close', this.texts.editorClose);
    close.addEventListener('click', () => void this.requestClose());
    actions.append(close);
    head.append(title, this.dirtyBadge, actions);
    const body = document.createElement('div');
    body.className = 'fm-dialog-body';
    this.dialog.append(head, body);
    this.overlay.hidden = false;
    close.focus();
    return this.generation;
  }

  private buildEditor(text: string, entry: FileEntry): void {
    const body = this.dialog.querySelector('.fm-dialog-body');
    const actions = this.dialog.querySelector('.fm-dialog-actions');
    if (body === null || actions === null) {
      return;
    }
    const language = detectLanguage(entry.name, text);
    const code = new CodeArea(text, language, entry.name, {
      changed: () => this.markDirty(true),
      caret: (line, column, selected) => this.updatePosition(line, column, selected),
      save: () => void this.save(),
    });
    this.code = code;
    this.position = document.createElement('span');
    this.position.className = 'fm-pos';
    const select = document.createElement('select');
    select.className = 'fm-lang';
    select.setAttribute('aria-label', this.texts.editorLanguage);
    for (const option of LANGUAGES) {
      const item = document.createElement('option');
      item.value = option;
      item.textContent = languageLabel(option);
      select.append(item);
    }
    select.value = language;
    select.addEventListener('change', () => {
      code.setLanguage(select.value as Language);
      code.focus();
    });
    const encoding = document.createElement('span');
    encoding.textContent = `UTF-8${this.bom ? ' BOM' : ''} · ${this.crlf ? 'CRLF' : 'LF'}`;
    const footer = document.createElement('div');
    footer.className = 'fm-dialog-foot';
    const right = document.createElement('span');
    right.className = 'fm-foot-right';
    this.lines = document.createElement('span');
    right.append(this.lines, select, encoding);
    footer.append(this.position, right);
    body.replaceChildren(code.root, footer);
    code.mount();
    this.lines.textContent = this.texts.editorLines(text === '' ? 1 : text.split('\n').length);

    this.saveButton = iconButton('i-save', this.texts.editorSave, this.texts.editorSave);
    this.saveButton.classList.add('primary');
    this.saveButton.disabled = true;
    this.saveButton.addEventListener('click', () => void this.save());
    actions.prepend(this.saveButton);
    code.focus();
    code.textarea.setSelectionRange(0, 0);
  }

  private updatePosition(line: number, column: number, selected: number): void {
    if (this.position === null) {
      return;
    }
    const base = this.texts.editorPosition(line, column);
    this.position.textContent = selected > 0 ? `${base} · ${this.texts.editorSelected(selected)}` : base;
  }

  private markDirty(value: boolean): void {
    this.dirty = value;
    if (this.saveButton !== null) {
      this.saveButton.disabled = !value || this.saving;
    }
    if (this.dirtyBadge !== null) {
      this.dirtyBadge.hidden = !value;
    }
  }

  private keydown(event: KeyboardEvent): void {
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 's') {
      event.preventDefault();
      if (this.code !== null) {
        void this.save();
      }
    } else if (event.key === 'Escape') {
      event.preventDefault();
      event.stopPropagation();
      void this.requestClose();
    } else {
      event.stopPropagation();
    }
  }

  private async save(): Promise<void> {
    const area = this.code;
    const current = this.current;
    if (area === null || current === null || this.saving || !this.dirty) {
      return;
    }
    this.saving = true;
    this.markDirty(true);
    try {
      const latest = await this.ops.stat(current.path);
      const known = this.baseline;
      if (
        latest !== null &&
        known !== null &&
        (latest.size !== known.size || latest.modified !== known.modified) &&
        !(await this.ops.confirm(this.texts.editorChangedMeanwhile, this.texts.editorSave, true))
      ) {
        return;
      }
      const value = area.value;
      const text = this.crlf ? value.replaceAll('\n', '\r\n') : value;
      const ok = await this.ops.write(current.path, new TextEncoder().encode(this.bom ? `\ufeff${text}` : text));
      if (!ok) {
        return;
      }
      this.baseline = await this.ops.stat(current.path);
      if (area.value === value) {
        this.markDirty(false);
      }
      this.ops.say(this.texts.editorSaved(current.entry.name));
      this.ops.saved();
    } finally {
      this.saving = false;
      if (this.saveButton !== null) {
        this.saveButton.disabled = !this.dirty;
      }
    }
  }

  private dismiss(): void {
    this.generation += 1;
    this.opened = false;
    this.dirty = false;
    this.code?.dispose();
    this.code = null;
    this.lines = null;
    this.saveButton = null;
    this.dirtyBadge = null;
    this.position = null;
    this.current = null;
    this.baseline = null;
    this.overlay.hidden = true;
    this.dialog.replaceChildren();
  }
}
