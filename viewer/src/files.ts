import {
  type FileEntry,
  type FileFailure,
  type FilesMessage,
  MAX_FILE_CHUNK,
  WINDOW_BYTES,
  encodeAck,
  encodeCancel,
  encodeData,
  encodeDone,
  encodeDownload,
  encodeList,
  encodeListRoots,
  encodeMkdir,
  encodeRemove,
  encodeUpload,
} from './filesprotocol.js';

export type FilesIo = {
  send(stream: number, payload: Uint8Array): void;
  buffered(): number;
};

export type FilesTexts = {
  noRoots: string;
  empty: string;
  truncated: string;
  up: string;
  refresh: string;
  newFolder: string;
  folderName: string;
  create: string;
  upload: string;
  download: string;
  remove: string;
  confirmRemove: string;
  cancel: string;
  columnName: string;
  columnSize: string;
  columnModified: string;
  downloading: (name: string, percent: number) => string;
  uploading: (name: string, percent: number) => string;
  finished: (name: string) => string;
  tooBig: (name: string) => string;
  failure: (reason: FileFailure) => string;
  closed: string;
};

type Transfer = {
  name: string;
  kind: 'download' | 'upload';
  total: number;
  done: number;
  parts: Uint8Array[];
  acked: number;
  waiting: (() => void) | null;
  cancelled: boolean;
  file: File | null;
  target: string;
};

type Pending = { kind: 'roots' } | { kind: 'list'; root: number; path: string } | { kind: 'change' };

const MAX_MEMORY_DOWNLOAD = 1 << 30;
const ACK_STEP = 256 * 1024;
const CONFIRM_MS = 3000;
const BUFFER_LIMIT = 1 << 20;

export class FilesPanel {
  private readonly pending = new Map<number, Pending>();
  private readonly transfers = new Map<number, Transfer>();
  private readonly list: HTMLElement;
  private readonly notice: HTMLElement;
  private readonly crumbs: HTMLElement;
  private readonly rootSelect: HTMLSelectElement;
  private readonly progress: HTMLElement;
  private readonly folderForm: HTMLFormElement;
  private readonly folderInput: HTMLInputElement;
  private readonly picker: HTMLInputElement;
  private readonly upButton: HTMLButtonElement;
  private roots: string[] | null = null;
  private root = 0;
  private path: string[] = [];
  private next = 1;
  private loaded = false;
  private entries: FileEntry[] = [];

  constructor(
    private readonly host: HTMLElement,
    private readonly io: FilesIo,
    private readonly texts: FilesTexts,
  ) {
    this.rootSelect = document.createElement('select');
    this.rootSelect.className = 'files-root';
    this.rootSelect.addEventListener('change', () => {
      this.root = Number(this.rootSelect.value);
      this.path = [];
      this.refresh();
    });
    this.crumbs = document.createElement('div');
    this.crumbs.className = 'files-path';
    this.upButton = this.button(this.texts.up, () => {
      this.path.pop();
      this.refresh();
    });
    const refresh = this.button(this.texts.refresh, () => this.refresh());
    const newFolder = this.button(this.texts.newFolder, () => this.toggleFolderForm());
    this.picker = document.createElement('input');
    this.picker.type = 'file';
    this.picker.multiple = true;
    this.picker.hidden = true;
    this.picker.addEventListener('change', () => {
      for (const file of this.picker.files ?? []) {
        this.startUpload(file);
      }
      this.picker.value = '';
    });
    const upload = this.button(this.texts.upload, () => this.picker.click());
    const toolbar = document.createElement('div');
    toolbar.className = 'files-toolbar';
    toolbar.append(this.rootSelect, this.crumbs, this.upButton, refresh, newFolder, upload, this.picker);

    this.folderInput = document.createElement('input');
    this.folderInput.type = 'text';
    this.folderInput.maxLength = 255;
    this.folderInput.placeholder = this.texts.folderName;
    this.folderInput.setAttribute('aria-label', this.texts.folderName);
    this.folderInput.autocomplete = 'off';
    const create = document.createElement('button');
    create.type = 'submit';
    create.textContent = this.texts.create;
    this.folderForm = document.createElement('form');
    this.folderForm.className = 'files-folder';
    this.folderForm.hidden = true;
    this.folderForm.append(this.folderInput, create);
    this.folderForm.addEventListener('submit', (event) => {
      event.preventDefault();
      this.makeFolder(this.folderInput.value.trim());
    });

    this.notice = document.createElement('p');
    this.notice.className = 'files-notice';
    this.notice.setAttribute('role', 'status');
    this.progress = document.createElement('div');
    this.progress.className = 'files-progress';
    this.list = document.createElement('div');
    this.list.className = 'files-list';
    this.host.append(toolbar, this.folderForm, this.notice, this.progress, this.list);
    this.render();
  }

  show(): void {
    if (!this.loaded) {
      this.loaded = true;
      this.request({ kind: 'roots' }, encodeListRoots());
    }
  }

  reset(): void {
    for (const transfer of this.transfers.values()) {
      transfer.cancelled = true;
      transfer.waiting?.();
    }
    this.transfers.clear();
    this.pending.clear();
    this.roots = null;
    this.loaded = false;
    this.path = [];
    this.root = 0;
    this.say(this.texts.closed);
    this.render();
  }

  receive(stream: number, message: FilesMessage): void {
    const transfer = this.transfers.get(stream);
    if (transfer !== undefined) {
      this.transferMessage(stream, transfer, message);
      return;
    }
    const pending = this.pending.get(stream);
    if (pending === undefined) {
      return;
    }
    this.pending.delete(stream);
    if (message.kind === 'failed') {
      this.say(this.texts.failure(message.reason));
      if (pending.kind === 'roots') {
        this.loaded = false;
      }
      return;
    }
    this.settle(pending, message);
  }

  private settle(pending: Pending, message: FilesMessage): void {
    if (pending.kind === 'roots' && message.kind === 'roots') {
      this.roots = message.names;
      this.root = 0;
      this.path = [];
      this.say(message.names.length === 0 ? this.texts.noRoots : '');
      this.render();
      if (message.names.length > 0) {
        this.refresh();
      }
    } else if (pending.kind === 'list' && message.kind === 'listing') {
      if (pending.root === this.root && pending.path === this.path.join('/')) {
        this.entries = message.entries;
        this.say(message.truncated ? this.texts.truncated : '');
        this.render();
      }
    } else if (pending.kind === 'change' && message.kind === 'done') {
      this.refresh();
    }
  }

  private refresh(): void {
    if (this.roots === null || this.roots.length === 0) {
      return;
    }
    const path = this.path.join('/');
    this.request({ kind: 'list', root: this.root, path }, encodeList(this.root, path));
  }

  private request(pending: Pending, payload: Uint8Array): number {
    const stream = this.next;
    this.next += 1;
    this.pending.set(stream, pending);
    this.io.send(stream, payload);
    return stream;
  }

  private makeFolder(name: string): void {
    if (name === '') {
      return;
    }
    this.folderInput.value = '';
    this.folderForm.hidden = true;
    this.request({ kind: 'change' }, encodeMkdir(this.root, this.child(name)));
  }

  private toggleFolderForm(): void {
    this.folderForm.hidden = !this.folderForm.hidden;
    if (!this.folderForm.hidden) {
      this.folderInput.focus();
    }
  }

  private child(name: string): string {
    return [...this.path, name].join('/');
  }

  private startDownload(entry: FileEntry): void {
    if (entry.size > MAX_MEMORY_DOWNLOAD) {
      this.say(this.texts.tooBig(entry.name));
      return;
    }
    const target = this.child(entry.name);
    const stream = this.next;
    this.next += 1;
    this.transfers.set(stream, {
      name: entry.name,
      kind: 'download',
      total: entry.size,
      done: 0,
      parts: [],
      acked: 0,
      waiting: null,
      cancelled: false,
      file: null,
      target,
    });
    this.io.send(stream, encodeDownload(this.root, target));
    this.renderProgress();
  }

  private startUpload(file: File): void {
    const target = this.child(file.name);
    const stream = this.next;
    this.next += 1;
    this.transfers.set(stream, {
      name: file.name,
      kind: 'upload',
      total: file.size,
      done: 0,
      parts: [],
      acked: 0,
      waiting: null,
      cancelled: false,
      file,
      target,
    });
    this.io.send(stream, encodeUpload(this.root, target, file.size));
    this.renderProgress();
  }

  private transferMessage(stream: number, transfer: Transfer, message: FilesMessage): void {
    switch (message.kind) {
      case 'ready':
        if (transfer.kind === 'upload') {
          void this.pump(stream, transfer);
        } else {
          transfer.total = message.size;
        }
        break;
      case 'data':
        this.receiveData(stream, transfer, message.data);
        break;
      case 'ack':
        transfer.acked = message.bytes;
        transfer.waiting?.();
        break;
      case 'done':
        this.finish(stream, transfer);
        break;
      case 'failed':
        this.end(stream);
        this.say(`${transfer.name}: ${this.texts.failure(message.reason)}`);
        break;
      default:
        this.end(stream);
        break;
    }
    this.renderProgress();
  }

  private receiveData(stream: number, transfer: Transfer, data: Uint8Array): void {
    transfer.parts.push(data.slice());
    const before = transfer.done;
    transfer.done += data.length;
    if (Math.floor(transfer.done / ACK_STEP) !== Math.floor(before / ACK_STEP)) {
      this.io.send(stream, encodeAck(transfer.done));
    }
  }

  private finish(stream: number, transfer: Transfer): void {
    this.end(stream);
    if (transfer.kind === 'download') {
      const blob = new Blob(transfer.parts as BlobPart[]);
      const url = URL.createObjectURL(blob);
      const link = document.createElement('a');
      link.href = url;
      link.download = transfer.name;
      link.click();
      setTimeout(() => URL.revokeObjectURL(url), 10_000);
    } else {
      this.refresh();
    }
    this.say(this.texts.finished(transfer.name));
  }

  private async pump(stream: number, transfer: Transfer): Promise<void> {
    const file = transfer.file;
    if (file === null) {
      return;
    }
    let offset = 0;
    while (offset < file.size && !transfer.cancelled) {
      while (offset - transfer.acked >= WINDOW_BYTES && !transfer.cancelled) {
        await new Promise<void>((resolve) => {
          transfer.waiting = resolve;
        });
        transfer.waiting = null;
      }
      while (this.io.buffered() > BUFFER_LIMIT && !transfer.cancelled) {
        await new Promise((resolve) => setTimeout(resolve, 20));
      }
      if (transfer.cancelled || !this.transfers.has(stream)) {
        return;
      }
      const end = Math.min(offset + MAX_FILE_CHUNK, file.size);
      const chunk = new Uint8Array(await file.slice(offset, end).arrayBuffer());
      if (chunk.length === 0) {
        break;
      }
      this.io.send(stream, encodeData(chunk));
      offset += chunk.length;
      transfer.done = offset;
      this.renderProgress();
    }
    if (!transfer.cancelled && this.transfers.has(stream)) {
      this.io.send(stream, encodeDone());
    }
  }

  private end(stream: number): void {
    const transfer = this.transfers.get(stream);
    if (transfer !== undefined) {
      transfer.cancelled = true;
      transfer.waiting?.();
    }
    this.transfers.delete(stream);
  }

  private cancel(stream: number): void {
    this.io.send(stream, encodeCancel());
    this.end(stream);
    this.renderProgress();
  }

  private remove(entry: FileEntry): void {
    this.request({ kind: 'change' }, encodeRemove(this.root, this.child(entry.name)));
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

  private render(): void {
    const roots = this.roots ?? [];
    this.rootSelect.replaceChildren(
      ...roots.map((name, index) => {
        const option = document.createElement('option');
        option.value = String(index);
        option.textContent = name;
        return option;
      }),
    );
    this.rootSelect.value = String(this.root);
    this.rootSelect.hidden = roots.length < 2;
    this.upButton.disabled = this.path.length === 0;
    this.renderCrumbs();
    this.renderList();
    this.renderProgress();
  }

  private renderCrumbs(): void {
    const roots = this.roots ?? [];
    const parts = [roots[this.root] ?? '', ...this.path];
    this.crumbs.replaceChildren(
      ...parts.map((part, index) => {
        const crumb = document.createElement('button');
        crumb.type = 'button';
        crumb.className = 'crumb';
        crumb.textContent = part === '' ? '/' : part;
        crumb.addEventListener('click', () => {
          this.path = this.path.slice(0, index);
          this.refresh();
        });
        return crumb;
      }),
    );
  }

  private renderList(): void {
    if (this.roots === null || this.roots.length === 0) {
      this.list.replaceChildren();
      return;
    }
    if (this.entries.length === 0) {
      const empty = document.createElement('p');
      empty.className = 'files-empty';
      empty.textContent = this.texts.empty;
      this.list.replaceChildren(empty);
      return;
    }
    const table = document.createElement('table');
    const head = table.createTHead().insertRow();
    for (const label of [this.texts.columnName, this.texts.columnSize, this.texts.columnModified, '']) {
      const cell = document.createElement('th');
      cell.textContent = label;
      head.append(cell);
    }
    const body = table.createTBody();
    for (const entry of this.entries) {
      body.append(this.row(entry));
    }
    this.list.replaceChildren(table);
  }

  private row(entry: FileEntry): HTMLTableRowElement {
    const row = document.createElement('tr');
    const name = row.insertCell();
    if (entry.kind === 'directory') {
      const open = this.button(entry.name, () => {
        this.path.push(entry.name);
        this.refresh();
      });
      open.className = 'entry-name directory';
      name.append(open);
    } else {
      name.textContent = entry.name;
      name.className = entry.kind === 'other' ? 'entry-name muted' : 'entry-name';
    }
    const size = row.insertCell();
    size.className = 'number';
    size.textContent = entry.kind === 'file' ? formatSize(entry.size) : '';
    const modified = row.insertCell();
    modified.className = 'number';
    modified.textContent = entry.modified === 0 ? '' : formatDate(entry.modified);
    const actions = row.insertCell();
    actions.className = 'actions';
    if (entry.kind === 'file') {
      actions.append(this.button(this.texts.download, () => this.startDownload(entry)));
    }
    actions.append(this.confirmButton(entry));
    return row;
  }

  private confirmButton(entry: FileEntry): HTMLButtonElement {
    const button = this.button(this.texts.remove, () => {
      if (button.dataset['armed'] === '1') {
        this.remove(entry);
        return;
      }
      button.dataset['armed'] = '1';
      button.textContent = this.texts.confirmRemove;
      setTimeout(() => {
        button.dataset['armed'] = '';
        button.textContent = this.texts.remove;
      }, CONFIRM_MS);
    });
    return button;
  }

  private renderProgress(): void {
    const items = [...this.transfers].map(([stream, transfer]) => {
      const percent = transfer.total === 0 ? 100 : Math.min(100, Math.floor((transfer.done * 100) / transfer.total));
      const line = document.createElement('div');
      line.className = 'transfer';
      const label = document.createElement('span');
      label.textContent =
        transfer.kind === 'download'
          ? this.texts.downloading(transfer.name, percent)
          : this.texts.uploading(transfer.name, percent);
      line.append(label, this.button(this.texts.cancel, () => this.cancel(stream)));
      return line;
    });
    this.progress.replaceChildren(...items);
  }
}

export function formatSize(bytes: number): string {
  const units = ['B', 'KiB', 'MiB', 'GiB', 'TiB'];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  const digits = unit === 0 || value >= 100 ? 0 : 1;
  return `${value.toFixed(digits)} ${units[unit]}`;
}

function formatDate(seconds: number): string {
  return new Intl.DateTimeFormat(navigator.language, { dateStyle: 'medium', timeStyle: 'short' }).format(
    new Date(seconds * 1000),
  );
}
