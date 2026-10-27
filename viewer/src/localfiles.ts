import type { FileEntry } from './filesprotocol.js';
import { type FileCategory, categorize, sortEntries, uniqueName, validName } from './filetypes.js';
import type { Sink } from './files.js';
import type { Messages } from './strings.js';

export type RemoteBridge = {
  dir(): string[];
  names(): string[];
  selection(): FileEntry[];
  draggedNames(): string[] | null;
  list(dir: string[]): Promise<FileEntry[] | null>;
  upload(dir: string[], blob: Blob, name: string): Promise<boolean>;
  mkdir(dir: string[], name: string): Promise<boolean>;
  download(dir: string[], entry: FileEntry, sink: Sink): Promise<boolean>;
  refresh(): Promise<boolean>;
  say(text: string): void;
  confirm(message: string, okLabel: string, danger: boolean): Promise<boolean>;
};

type Permission = 'granted' | 'denied' | 'prompt';
type PermissionHandle = FileSystemDirectoryHandle & {
  queryPermission(options: { mode: 'readwrite' }): Promise<Permission>;
  requestPermission(options: { mode: 'readwrite' }): Promise<Permission>;
};
type Picker = () => Promise<FileSystemDirectoryHandle>;
type LocalEntry = FileEntry & { handle: FileSystemHandle };

const SVG_NS = 'http://www.w3.org/2000/svg';
const MAX_ENTRIES = 2000;
const DATABASE = 'wirehaus-local';
const STORE = 'handles';
const KEY = 'root';

const ICONS: Record<FileCategory, string> = {
  folder: 'i-folder', text: 'i-file-text', code: 'i-code', image: 'i-image', audio: 'i-music',
  video: 'i-film', archive: 'i-archive', document: 'i-file-text', other: 'i-file',
};

function svg(id: string): SVGSVGElement {
  const node = document.createElementNS(SVG_NS, 'svg');
  node.setAttribute('aria-hidden', 'true');
  const use = document.createElementNS(SVG_NS, 'use');
  use.setAttribute('href', `#${id}`);
  node.append(use);
  return node;
}

function node<K extends keyof HTMLElementTagNameMap>(tag: K, className?: string, text?: string): HTMLElementTagNameMap[K] {
  const created = document.createElement(tag);
  if (className !== undefined) created.className = className;
  if (text !== undefined) created.textContent = text;
  return created;
}

function pickerAvailable(): Picker | null {
  const picker = (window as unknown as { showDirectoryPicker?: (options?: object) => Promise<FileSystemDirectoryHandle> }).showDirectoryPicker;
  return typeof picker === 'function' ? () => picker.call(window, { mode: 'readwrite' }) : null;
}

function openDatabase(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open(DATABASE, 1);
    request.onupgradeneeded = () => request.result.createObjectStore(STORE);
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error ?? new Error('cannot open the local store'));
  });
}

async function storedHandle(): Promise<FileSystemDirectoryHandle | null> {
  try {
    const database = await openDatabase();
    try {
      return await new Promise((resolve) => {
        const request = database.transaction(STORE, 'readonly').objectStore(STORE).get(KEY);
        request.onsuccess = () => resolve((request.result as FileSystemDirectoryHandle | undefined) ?? null);
        request.onerror = () => resolve(null);
      });
    } finally {
      database.close();
    }
  } catch {
    return null;
  }
}

async function rememberHandle(handle: FileSystemDirectoryHandle | null): Promise<void> {
  try {
    const database = await openDatabase();
    try {
      await new Promise<void>((resolve) => {
        const store = database.transaction(STORE, 'readwrite').objectStore(STORE);
        const request = handle === null ? store.delete(KEY) : store.put(handle, KEY);
        request.onsuccess = () => resolve();
        request.onerror = () => resolve();
      });
    } finally {
      database.close();
    }
  } catch {
    return;
  }
}

function formatSize(bytes: number): string {
  const units = ['B', 'KiB', 'MiB', 'GiB', 'TiB'];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value.toFixed(unit === 0 || value >= 100 ? 0 : 1)} ${units[unit]}`;
}

export class LocalPane {
  private readonly pick = pickerAvailable();
  private readonly crumbs = node('nav', 'fm-crumbs');
  private readonly list = node('div', 'fm-list details');
  private readonly status = node('div', 'fm-status');
  private readonly buttons: Record<string, HTMLButtonElement> = {};
  private readonly selected = new Set<string>();
  private readonly byName = new Map<string, LocalEntry>();
  private trail: { name: string; handle: FileSystemDirectoryHandle }[] = [];
  private dragged: string[] | null = null;
  private busy = false;
  private anchor: string | null = null;
  private pending: FileSystemDirectoryHandle | null = null;
  private entries: LocalEntry[] = [];
  private notice = '';

  constructor(
    host: HTMLElement,
    private readonly texts: Messages,
    private readonly remote: RemoteBridge,
  ) {
    host.classList.add('fm', 'fm-local');
    const bar = node('div', 'fm-bar');
    this.buttons['choose'] = this.tool('i-folder', texts.localChoose, () => void this.choose(), true);
    this.buttons['up'] = this.tool('i-up', texts.filesUp, () => void this.up());
    this.buttons['refresh'] = this.tool('i-refresh', texts.filesRefresh, () => void this.load());
    bar.append(this.buttons['choose'], this.buttons['up'], this.buttons['refresh'], this.crumbs);
    const actions = node('div', 'fm-actions');
    this.buttons['send'] = this.tool('i-upload', texts.localUpload, () => void this.sendToRemote(), true);
    this.buttons['send'].classList.add('primary');
    this.buttons['newFolder'] = this.tool('i-folder-plus', texts.filesNewFolder, () => void this.newFolder(), true);
    this.buttons['remove'] = this.tool('i-trash', texts.filesRemove, () => void this.removeSelected(), true);
    this.buttons['remove'].classList.add('danger');
    actions.append(this.buttons['send'], this.buttons['newFolder'], this.buttons['remove']);
    const main = node('section', 'fm-main');
    this.list.tabIndex = 0;
    this.list.setAttribute('role', 'listbox');
    this.list.setAttribute('aria-multiselectable', 'true');
    this.list.setAttribute('aria-label', texts.localTitle);
    main.append(this.list);
    const title = node('div', 'fm-local-title');
    title.append(svg('i-desktop'), node('span', undefined, texts.localTitle));
    host.append(title, bar, actions, main, this.status);
    this.bind();
    void this.restore();
    this.render();
  }

  isDragging(): boolean {
    return this.dragged !== null;
  }

  sendSelection(): void {
    void this.sendToRemote();
  }

  async receiveFromRemote(): Promise<void> {
    const dir = this.current();
    const entries = this.remote.selection();
    if (dir === null || entries.length === 0 || this.busy) {
      return;
    }
    this.busy = true;
    try {
      const taken = new Set(this.entries.map((entry) => entry.name));
      for (const entry of entries) {
        const name = uniqueName(taken, entry.name);
        taken.add(name);
        if (!(await this.downloadEntry(dir, entry, name, this.remote.dir()))) {
          break;
        }
      }
    } finally {
      this.busy = false;
    }
    await this.load();
  }

  private tool(icon: string, label: string, run: () => void, withLabel = false): HTMLButtonElement {
    const button = node('button', 'fm-tool');
    button.type = 'button';
    button.title = label;
    button.setAttribute('aria-label', label);
    button.append(svg(icon));
    if (withLabel) button.append(node('span', 'lbl', label));
    button.addEventListener('click', run);
    return button;
  }

  private current(): FileSystemDirectoryHandle | null {
    return this.trail[this.trail.length - 1]?.handle ?? null;
  }

  private say(text: string): void {
    this.notice = text;
    this.renderStatus();
  }

  private async restore(): Promise<void> {
    if (this.pick === null) {
      return;
    }
    const handle = await storedHandle();
    if (handle === null) {
      return;
    }
    const permission = await (handle as PermissionHandle).queryPermission({ mode: 'readwrite' }).catch(() => 'denied' as Permission);
    if (permission === 'granted') {
      await this.open(handle);
    } else {
      this.pending = handle;
      this.render();
    }
  }

  private async choose(): Promise<void> {
    if (this.pending !== null) {
      const handle = this.pending;
      const permission = await (handle as PermissionHandle).requestPermission({ mode: 'readwrite' }).catch(() => 'denied' as Permission);
      if (permission === 'granted') {
        this.pending = null;
        await this.open(handle);
        return;
      }
    }
    if (this.pick === null) {
      return;
    }
    try {
      const handle = await this.pick();
      this.pending = null;
      await rememberHandle(handle);
      await this.open(handle);
    } catch {
      return;
    }
  }

  private async open(handle: FileSystemDirectoryHandle): Promise<void> {
    this.trail = [{ name: handle.name, handle }];
    await this.load();
  }

  private async up(): Promise<void> {
    if (this.trail.length > 1) {
      this.trail.pop();
      await this.load();
    }
  }

  private async enter(name: string): Promise<void> {
    const entry = this.byName.get(name);
    if (entry?.kind === 'directory') {
      this.trail.push({ name, handle: entry.handle as FileSystemDirectoryHandle });
      await this.load();
    }
  }

  private async load(): Promise<void> {
    const dir = this.current();
    this.selected.clear();
    this.byName.clear();
    this.entries = [];
    if (dir === null) {
      this.render();
      return;
    }
    const found: LocalEntry[] = [];
    try {
      for await (const [name, handle] of dir.entries()) {
        if (found.length >= MAX_ENTRIES) {
          break;
        }
        if (handle.kind === 'directory') {
          found.push({ name, kind: 'directory', size: 0, modified: 0, handle });
        } else {
          const file = await (handle as FileSystemFileHandle).getFile().catch(() => null);
          found.push({ name, kind: 'file', size: file?.size ?? 0, modified: Math.floor((file?.lastModified ?? 0) / 1000), handle });
        }
      }
    } catch {
      this.say(this.texts.filesFailure.io);
    }
    const order = new Map(found.map((entry) => [entry.name, entry]));
    this.entries = sortEntries(found, 'name', false).flatMap((entry) => {
      const full = order.get(entry.name);
      return full === undefined ? [] : [full];
    });
    for (const entry of this.entries) {
      this.byName.set(entry.name, entry);
    }
    this.render();
  }

  private render(): void {
    const ready = this.current() !== null;
    this.buttons['up']!.disabled = this.trail.length <= 1;
    this.buttons['refresh']!.disabled = !ready;
    this.buttons['newFolder']!.disabled = !ready;
    this.buttons['send']!.disabled = !ready || this.selected.size === 0;
    this.buttons['remove']!.disabled = !ready || this.selected.size === 0;
    const choose = this.buttons['choose']!;
    choose.disabled = this.pick === null;
    const label = choose.querySelector('.lbl');
    if (label !== null) {
      label.textContent = this.pending === null ? this.texts.localChoose : this.texts.localReopen(this.pending.name);
    }
    this.crumbs.replaceChildren(
      ...this.trail.flatMap((step, index) => {
        const crumb = node('button', 'fm-crumb', step.name);
        crumb.type = 'button';
        crumb.addEventListener('click', () => {
          if (index < this.trail.length - 1) {
            this.trail = this.trail.slice(0, index + 1);
            void this.load();
          }
        });
        return index === 0 ? [crumb] : [node('span', 'fm-sep', '/'), crumb];
      }),
    );
    if (this.pick === null) {
      this.list.replaceChildren(node('p', 'fm-empty', this.texts.localUnsupported));
    } else if (!ready) {
      this.list.replaceChildren(node('p', 'fm-empty', this.texts.localNoFolder));
    } else if (this.entries.length === 0) {
      this.list.replaceChildren(node('p', 'fm-empty', this.texts.filesEmpty));
    } else {
      this.list.replaceChildren(...this.entries.map((entry) => this.row(entry)));
    }
    this.renderStatus();
  }

  private row(entry: LocalEntry): HTMLElement {
    const row = node('div', `fm-item cat-${categorize(entry)}`);
    row.dataset['name'] = entry.name;
    row.setAttribute('role', 'option');
    row.setAttribute('aria-selected', String(this.selected.has(entry.name)));
    row.classList.toggle('selected', this.selected.has(entry.name));
    row.draggable = true;
    const icon = node('span', 'fm-icon');
    icon.append(svg(ICONS[categorize(entry)]));
    const name = node('span', 'fm-name c-name', entry.name);
    name.title = entry.name;
    const date = node('span', 'fm-meta c-date', entry.modified === 0 ? '' : new Intl.DateTimeFormat(undefined, { dateStyle: 'medium', timeStyle: 'short' }).format(new Date(entry.modified * 1000)));
    const size = node('span', 'fm-meta c-size', entry.kind === 'file' ? formatSize(entry.size) : '');
    row.append(icon, name, date, size);
    return row;
  }

  private renderStatus(): void {
    const count = this.selected.size;
    this.status.replaceChildren(
      node('span', undefined, this.notice !== '' ? this.notice : this.texts.filesItems(this.entries.length)),
      node('span', undefined, count > 0 ? this.texts.filesSelected(count, '') : ''),
    );
  }

  private paintSelection(): void {
    for (const element of this.list.querySelectorAll<HTMLElement>('.fm-item')) {
      const on = this.selected.has(element.dataset['name'] ?? '');
      element.classList.toggle('selected', on);
      element.setAttribute('aria-selected', String(on));
    }
    this.render();
  }

  private bind(): void {
    this.list.addEventListener('click', (event) => {
      const name = (event.target as Element).closest<HTMLElement>('.fm-item')?.dataset['name'];
      if (name === undefined) {
        this.selected.clear();
        this.paintSelection();
        return;
      }
      if (event.shiftKey && this.anchor !== null) {
        const names = this.entries.map((entry) => entry.name);
        const from = names.indexOf(this.anchor);
        const to = names.indexOf(name);
        this.selected.clear();
        for (const between of names.slice(Math.min(from, to), Math.max(from, to) + 1)) this.selected.add(between);
      } else if (event.ctrlKey || event.metaKey) {
        if (!this.selected.delete(name)) this.selected.add(name);
        this.anchor = name;
      } else {
        this.selected.clear();
        this.selected.add(name);
        this.anchor = name;
      }
      this.paintSelection();
    });
    this.list.addEventListener('dblclick', (event) => {
      const name = (event.target as Element).closest<HTMLElement>('.fm-item')?.dataset['name'];
      if (name !== undefined) void this.enter(name);
    });
    this.list.addEventListener('dragstart', (event) => {
      const name = (event.target as Element).closest<HTMLElement>('.fm-item')?.dataset['name'];
      if (name === undefined || event.dataTransfer === null) {
        event.preventDefault();
        return;
      }
      if (!this.selected.has(name)) {
        this.selected.clear();
        this.selected.add(name);
        this.paintSelection();
      }
      this.dragged = [...this.selected];
      event.dataTransfer.effectAllowed = 'copy';
      event.dataTransfer.setData('text/plain', this.dragged.join('\n'));
    });
    this.list.addEventListener('dragend', () => {
      this.dragged = null;
      this.list.classList.remove('drop');
    });
    this.list.addEventListener('dragover', (event) => {
      if (this.remote.draggedNames() !== null && this.current() !== null) {
        event.preventDefault();
        this.list.classList.add('drop');
      }
    });
    this.list.addEventListener('dragleave', () => this.list.classList.remove('drop'));
    this.list.addEventListener('drop', (event) => {
      this.list.classList.remove('drop');
      if (this.remote.draggedNames() !== null) {
        event.preventDefault();
        void this.receiveFromRemote();
      }
    });
    this.list.addEventListener('keydown', (event) => {
      if (event.key === 'Delete') {
        event.preventDefault();
        void this.removeSelected();
      } else if (event.key === 'Backspace') {
        event.preventDefault();
        void this.up();
      } else if (event.key === 'Enter' && this.selected.size === 1) {
        void this.enter([...this.selected][0] ?? '');
      } else if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'a') {
        event.preventDefault();
        for (const entry of this.entries) this.selected.add(entry.name);
        this.paintSelection();
      }
    });
  }

  private async newFolder(): Promise<void> {
    const dir = this.current();
    if (dir === null) {
      return;
    }
    const name = uniqueName(this.entries.map((entry) => entry.name), this.texts.filesNewFolderName);
    if (!validName(name)) {
      return;
    }
    try {
      await dir.getDirectoryHandle(name, { create: true });
    } catch {
      this.say(this.texts.filesFailure.io);
      return;
    }
    await this.load();
    this.selected.add(name);
    this.paintSelection();
  }

  private async removeSelected(): Promise<void> {
    const dir = this.current();
    const names = [...this.selected];
    const first = names[0];
    if (dir === null || first === undefined) {
      return;
    }
    if (!(await this.remote.confirm(this.texts.filesConfirmRemove(names.length, first), this.texts.filesRemove, true))) {
      return;
    }
    for (const name of names) {
      try {
        await dir.removeEntry(name, { recursive: true });
      } catch {
        this.say(`${name}: ${this.texts.filesFailure.io}`);
        break;
      }
    }
    await this.load();
  }

  private async sendToRemote(): Promise<void> {
    if (this.busy || this.selected.size === 0) {
      return;
    }
    this.busy = true;
    const picked = [...this.selected].flatMap((name) => {
      const entry = this.byName.get(name);
      return entry === undefined ? [] : [entry];
    });
    try {
      const taken = new Set(this.remote.names());
      const target = this.remote.dir();
      for (const entry of picked) {
        const name = uniqueName(taken, entry.name);
        taken.add(name);
        if (!(await this.uploadEntry(entry, target, name))) {
          break;
        }
      }
    } finally {
      this.busy = false;
    }
    await this.remote.refresh();
  }

  private async uploadEntry(entry: LocalEntry, dir: string[], name: string): Promise<boolean> {
    if (entry.kind === 'file') {
      const file = await (entry.handle as FileSystemFileHandle).getFile().catch(() => null);
      return file !== null && (await this.remote.upload(dir, file, name));
    }
    if (!(await this.remote.mkdir(dir, name))) {
      return false;
    }
    const child = entry.handle as FileSystemDirectoryHandle;
    for await (const [childName, handle] of child.entries()) {
      const next: LocalEntry =
        handle.kind === 'directory'
          ? { name: childName, kind: 'directory', size: 0, modified: 0, handle }
          : { name: childName, kind: 'file', size: 0, modified: 0, handle };
      if (!(await this.uploadEntry(next, [...dir, name], childName))) {
        return false;
      }
    }
    return true;
  }

  private async downloadEntry(into: FileSystemDirectoryHandle, entry: FileEntry, name: string, from: string[]): Promise<boolean> {
    if (entry.kind === 'directory') {
      const folder = await into.getDirectoryHandle(name, { create: true }).catch(() => null);
      const children = await this.remote.list([...from, entry.name]);
      if (folder === null || children === null) {
        return false;
      }
      for (const child of children) {
        if (child.kind === 'other') continue;
        if (!(await this.downloadEntry(folder, child, child.name, [...from, entry.name]))) {
          return false;
        }
      }
      return true;
    }
    if (entry.kind !== 'file') {
      return true;
    }
    let handle: FileSystemFileHandle;
    let writable: FileSystemWritableFileStream;
    try {
      handle = await into.getFileHandle(name, { create: true });
      writable = await handle.createWritable();
    } catch {
      this.remote.say(`${name}: ${this.texts.filesFailure.io}`);
      return false;
    }
    const sink: Sink = {
      write: (chunk) => writable.write(chunk as unknown as BufferSource),
      close: () => writable.close(),
      abort: () => writable.abort().then(() => into.removeEntry(name).catch(() => undefined)),
    };
    return this.remote.download(from, entry, sink);
  }
}

