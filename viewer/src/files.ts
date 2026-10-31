import { saveBlob } from './download.js';
import { FileDialog } from './fileeditor.js';
import {
  type FileEntry,
  type FileFailure,
  type FilesMessage,
  MAX_FILE_CHUNK,
  WINDOW_BYTES,
  FEATURE_RENAME,
  encodeAck,
  encodeCancel,
  encodeData,
  encodeDone,
  encodeDownload,
  encodeList,
  encodeListRoots,
  encodeMkdir,
  encodeRemove,
  encodeRename,
  encodeUpload,
} from './filesprotocol.js';
import {
  type FileCategory,
  type SortKey,
  categorize,
  isEditable,
  isPreviewable,
  joinPath,
  knownPlaces,
  sortEntries,
  uniqueName,
  validName,
} from './filetypes.js';
import { LocalPane, type RemoteBridge } from './localfiles.js';
import type { Messages } from './strings.js';

export type FilesIo = {
  send(stream: number, payload: Uint8Array): void;
  buffered(): number;
};

export type FilesTexts = Messages;

export type Sink = {
  write(chunk: Uint8Array): Promise<void>;
  close(): Promise<void>;
  abort(): Promise<void>;
};

type Transfer = {
  sink: Sink | null;
  writing: Promise<void>;
  name: string;
  kind: 'download' | 'upload';
  total: number;
  done: number;
  parts: Uint8Array[];
  acked: number;
  waiting: (() => void) | null;
  cancelled: boolean;
  source: Blob | null;
  settle: (ok: boolean) => void;
};

type Place = { root: number; path: string[] };
type View = 'grid' | 'list';
type Preferences = { view: View; sort: SortKey; descending: boolean; dual: boolean };
type MenuItem = { label: string; icon: string; run: () => void; danger?: boolean } | 'separator';

const SVG_NS = 'http://www.w3.org/2000/svg';
const MAX_MEMORY_DOWNLOAD = 1 << 30;
const ACK_STEP = 256 * 1024;
const BUFFER_LIMIT = 1 << 20;
const LONG_PRESS_MS = 480;
const MOVE_TYPE = 'application/x-wirehaus-move';
const PREFERENCES_KEY = 'wirehaus.files';
const MAX_TREE_DEPTH = 64;

const CATEGORY_ICON: Record<FileCategory, string> = {
  folder: 'i-folder',
  text: 'i-file-text',
  code: 'i-code',
  image: 'i-image',
  audio: 'i-music',
  video: 'i-film',
  archive: 'i-archive',
  document: 'i-file-text',
  other: 'i-file',
};

function svg(id: string): SVGSVGElement {
  const node = document.createElementNS(SVG_NS, 'svg');
  node.setAttribute('aria-hidden', 'true');
  const use = document.createElementNS(SVG_NS, 'use');
  use.setAttribute('href', `#${id}`);
  node.append(use);
  return node;
}

function node<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  className?: string,
  text?: string,
): HTMLElementTagNameMap[K] {
  const created = document.createElement(tag);
  if (className !== undefined) {
    created.className = className;
  }
  if (text !== undefined) {
    created.textContent = text;
  }
  return created;
}

function loadPreferences(): Preferences {
  const coarse = typeof matchMedia === 'function' && matchMedia('(pointer: coarse)').matches;
  const fallback: Preferences = { view: coarse ? 'grid' : 'list', sort: 'name', descending: false, dual: false };
  try {
    const raw = localStorage.getItem(PREFERENCES_KEY);
    if (raw === null) {
      return fallback;
    }
    const value = JSON.parse(raw) as Partial<Preferences>;
    return {
      view: value.view === 'grid' || value.view === 'list' ? value.view : fallback.view,
      sort: value.sort === 'name' || value.sort === 'size' || value.sort === 'modified' ? value.sort : 'name',
      descending: value.descending === true,
      dual: value.dual === true,
    };
  } catch {
    return fallback;
  }
}

export class FilesPanel {
  private readonly pending = new Map<number, (message: FilesMessage) => void>();
  private readonly transfers = new Map<number, Transfer>();
  private readonly dialog: FileDialog;
  private readonly local: LocalPane;
  private readonly side: HTMLElement;
  private readonly crumbs: HTMLElement;
  private readonly search: HTMLInputElement;
  private readonly list: HTMLElement;
  private readonly status: HTMLElement;
  private readonly statusRight: HTMLElement;
  private readonly progress: HTMLElement;
  private readonly dropHint: HTMLElement;
  private readonly picker: HTMLInputElement;
  private readonly menu: HTMLElement;
  private readonly modal: HTMLElement;
  private readonly buttons: Record<string, HTMLButtonElement> = {};
  private readonly viewButton: HTMLButtonElement;
  private readonly sortSelect: HTMLSelectElement;
  private readonly itemElements = new Map<string, HTMLElement>();
  private readonly entryByName = new Map<string, FileEntry>();
  private readonly quick = new Map<number, string[]>();
  private readonly back: Place[] = [];
  private readonly forward: Place[] = [];
  private readonly selection = new Set<string>();
  private preferences = loadPreferences();
  onLocation: ((root: number, path: string[]) => void) | null = null;
  private initial: { root: number; path: string[] } | null = null;
  private roots: string[] | null = null;
  private features = 0;
  private root = 0;
  private path: string[] = [];
  private entries: FileEntry[] = [];
  private truncated = false;
  private filter = '';
  private anchor: string | null = null;
  private focused: string | null = null;
  private renaming: string | null = null;
  private dragging: { names: string[] } | null = null;
  private next = 1;
  private loaded = false;
  private loadError: string | null = null;
  private loadToken = 0;
  private notice = '';
  private noticeTimer: number | undefined;
  private typeahead = '';
  private typeaheadTimer: number | undefined;
  private pressTimer: number | undefined;
  private pressed = false;
  private generation = 0;

  constructor(
    private readonly host: HTMLElement,
    private readonly io: FilesIo,
    private readonly texts: FilesTexts,
  ) {
    const root = node('div', 'fm fm-remote');
    const bar = node('div', 'fm-bar');
    this.buttons['back'] = this.tool('i-left', texts.filesBack, () => this.goBack());
    this.buttons['forward'] = this.tool('i-right', texts.filesForward, () => this.goForward());
    this.buttons['up'] = this.tool('i-up', texts.filesUp, () => this.goUp());
    this.crumbs = node('nav', 'fm-crumbs');
    this.crumbs.setAttribute('aria-label', texts.tabFiles);
    this.search = node('input', 'fm-search');
    this.search.type = 'search';
    this.search.placeholder = texts.filesSearch;
    this.search.setAttribute('aria-label', texts.filesSearch);
    this.search.autocomplete = 'off';
    this.search.spellcheck = false;
    this.search.addEventListener('input', () => {
      this.filter = this.search.value.trim().toLowerCase();
      this.renderList();
      this.renderStatus();
    });
    this.search.addEventListener('keydown', (event) => {
      if (event.key === 'Escape') {
        this.search.value = '';
        this.filter = '';
        this.renderList();
        this.list.focus();
      }
      event.stopPropagation();
    });
    const searchBox = node('label', 'fm-searchbox');
    searchBox.append(svg('i-search'), this.search);
    this.viewButton = this.tool('i-grid', '', () => this.toggleView());
    this.sortSelect = node('select', 'fm-sort');
    this.sortSelect.setAttribute('aria-label', texts.filesSortBy);
    for (const [key, label] of [
      ['name', texts.filesColumnName],
      ['modified', texts.filesColumnModified],
      ['size', texts.filesColumnSize],
    ] as const) {
      const option = node('option', undefined, label);
      option.value = key;
      this.sortSelect.append(option);
    }
    this.sortSelect.addEventListener('change', () => this.sortBy(this.sortSelect.value as SortKey, false));
    bar.append(
      this.buttons['back'],
      this.buttons['forward'],
      this.buttons['up'],
      this.crumbs,
      searchBox,
      this.sortSelect,
      this.viewButton,
    );

    const actions = node('div', 'fm-actions');
    this.buttons['newFolder'] = this.tool('i-folder-plus', texts.filesNewFolder, () => void this.createFolder(), true);
    this.buttons['newFile'] = this.tool('i-file-plus', texts.filesNewFile, () => void this.createFile(), true);
    this.buttons['upload'] = this.tool('i-upload', texts.filesUpload, () => this.picker.click(), true);
    this.buttons['refresh'] = this.tool('i-refresh', texts.filesRefresh, () => void this.refresh());
    this.buttons['dual'] = this.tool('i-desktop', texts.filesDual, () => this.toggleDual(), true);
    this.buttons['toLocal'] = this.tool('i-left', texts.filesToLocal, () => void this.local.receiveFromRemote(), true);
    this.buttons['toLocal'].classList.add('fm-dual-only');
    const divider = node('span', 'fm-divider');
    this.buttons['open'] = this.tool('i-folder', texts.filesOpen, () => this.openSelected(), true);
    this.buttons['edit'] = this.tool('i-edit', texts.filesEdit, () => this.editSelected(), true);
    this.buttons['download'] = this.tool('i-down', texts.filesDownload, () => this.downloadSelected(), true);
    this.buttons['rename'] = this.tool('i-edit', texts.filesRename, () => this.startRename(), true);
    this.buttons['remove'] = this.tool('i-trash', texts.filesRemove, () => void this.deleteSelected(), true);
    this.buttons['remove']?.classList.add('danger');
    divider.classList.add('fm-sel');
    for (const name of ['open', 'edit', 'download', 'rename', 'remove']) {
      this.buttons[name]?.classList.add('fm-sel');
    }
    actions.append(
      this.buttons['newFolder'],
      this.buttons['newFile'],
      this.buttons['upload'],
      this.buttons['refresh'],
      this.buttons['dual'],
      this.buttons['toLocal'],
      divider,
      this.buttons['open'],
      this.buttons['edit'],
      this.buttons['download'],
      this.buttons['rename'],
      this.buttons['remove'],
    );

    this.picker = node('input');
    this.picker.type = 'file';
    this.picker.multiple = true;
    this.picker.hidden = true;
    this.picker.addEventListener('change', () => {
      const files = [...(this.picker.files ?? [])];
      this.picker.value = '';
      void this.uploadFiles(files, this.path);
    });

    this.side = node('aside', 'fm-side');
    this.list = node('div', 'fm-list');
    this.list.tabIndex = 0;
    this.list.setAttribute('role', 'listbox');
    this.list.setAttribute('aria-multiselectable', 'true');
    this.list.setAttribute('aria-label', texts.tabFiles);
    this.dropHint = node('div', 'fm-drophint', texts.filesDropHint);
    this.dropHint.hidden = true;
    const main = node('section', 'fm-main');
    main.append(this.list, this.dropHint);
    const body = node('div', 'fm-body');
    body.append(this.side, main);

    this.progress = node('div', 'fm-transfers');
    this.status = node('span', 'fm-status-left');
    this.status.setAttribute('role', 'status');
    this.statusRight = node('span', 'fm-status-right');
    const statusBar = node('div', 'fm-status');
    statusBar.append(this.status, this.statusRight);

    this.menu = node('div', 'fm-menu');
    this.menu.setAttribute('role', 'menu');
    this.menu.hidden = true;
    this.modal = node('div', 'fm-modal');
    this.modal.hidden = true;

    root.append(bar, actions, body, this.progress, statusBar, this.picker);
    const stage = node('div', 'fm-split');
    const localHost = node('div');
    stage.append(localHost, root);
    this.host.append(stage, this.menu, this.modal);
    this.local = new LocalPane(localHost, texts, this.bridge());
    this.host.classList.toggle('fm-dual-on', this.preferences.dual);
    this.dialog = new FileDialog(this.host, texts, {
      read: (path) => this.readFile(path),
      write: (path, data) => this.writeFile(path, data),
      stat: (path) => this.stat(path),
      download: (path, entry) => void this.saveToDisk(path, entry),
      say: (text) => this.say(text),
      confirm: (message, ok, danger) => this.confirm(message, ok, danger),
      saved: () => void this.refresh(),
    });

    this.bindList();
    this.bindHost();
    this.render();
  }

  restore(root: number, path: string[]): void {
    this.initial = { root, path };
  }

  show(): void {
    if (!this.loaded) {
      this.loaded = true;
      this.loadError = null;
      void this.loadRoots();
    }
  }

  suspend(): void {
    if (this.roots !== null) {
      this.initial = { root: this.root, path: [...this.path] };
    }
    this.generation += 1;
    for (const transfer of this.transfers.values()) {
      transfer.cancelled = true;
      transfer.waiting?.();
      transfer.settle(false);
    }
    this.transfers.clear();
    for (const resolve of this.pending.values()) {
      resolve({ kind: 'failed', reason: 'io' });
    }
    this.pending.clear();
    this.dialog.close();
    this.closeMenu();
    this.closeModal(false);
    this.loaded = false;
  }

  reset(): void {
    if (this.roots !== null) {
      this.initial = { root: this.root, path: [...this.path] };
    }
    this.generation += 1;
    for (const transfer of this.transfers.values()) {
      transfer.cancelled = true;
      transfer.waiting?.();
      transfer.settle(false);
    }
    this.transfers.clear();
    for (const resolve of this.pending.values()) {
      resolve({ kind: 'failed', reason: 'io' });
    }
    this.pending.clear();
    this.dialog.close();
    this.closeMenu();
    this.closeModal(false);
    this.roots = null;
    this.loadError = null;
    this.features = 0;
    this.loaded = false;
    this.path = [];
    this.root = 0;
    this.entries = [];
    this.back.length = 0;
    this.forward.length = 0;
    this.selection.clear();
    this.quick.clear();
    this.say(this.texts.filesClosed);
    this.render();
  }

  receive(stream: number, message: FilesMessage): void {
    const transfer = this.transfers.get(stream);
    if (transfer !== undefined) {
      this.transferMessage(stream, transfer, message);
      return;
    }
    const resolve = this.pending.get(stream);
    if (resolve !== undefined) {
      this.pending.delete(stream);
      resolve(message);
    }
  }

  private call(payload: Uint8Array): Promise<FilesMessage> {
    const stream = this.next;
    this.next += 1;
    return new Promise((resolve) => {
      this.pending.set(stream, resolve);
      this.io.send(stream, payload);
    });
  }

  private async must(payload: Uint8Array): Promise<boolean> {
    const reply = await this.call(payload);
    if (reply.kind === 'done') {
      return true;
    }
    if (reply.kind === 'failed') {
      this.say(this.texts.filesFailure[reply.reason]);
    }
    return false;
  }

  private async listDir(root: number, parts: readonly string[]): Promise<{ entries: FileEntry[]; truncated: boolean } | null> {
    const reply = await this.call(encodeList(root, joinPath(parts)));
    if (reply.kind === 'listing') {
      return { entries: reply.entries, truncated: reply.truncated };
    }
    if (reply.kind === 'failed') {
      this.say(this.texts.filesFailure[reply.reason]);
    }
    return null;
  }

  private async loadRoots(): Promise<void> {
    const generation = this.generation;
    const reply = await this.call(encodeListRoots());
    if (generation !== this.generation) {
      return;
    }
    if (reply.kind !== 'roots') {
      this.loaded = false;
      this.loadError = reply.kind === 'failed' ? this.texts.filesFailure[reply.reason] : this.texts.filesFailure.io;
      this.render();
      return;
    }
    this.loadError = null;
    this.roots = reply.names;
    this.features = reply.features;
    const wanted = this.initial;
    this.initial = null;
    const usable = wanted !== null && wanted.root >= 0 && wanted.root < reply.names.length;
    this.root = usable ? wanted.root : 0;
    this.path = usable ? wanted.path : [];
    this.say(reply.names.length === 0 || this.canRename ? '' : this.texts.filesAgentOld, !this.canRename);
    this.render();
    if (reply.names.length > 0 && !(await this.refresh()) && this.path.length > 0) {
      this.path = [];
      this.say('');
      await this.refresh();
    }
  }

  private async refresh(): Promise<boolean> {
    if (this.roots === null || this.roots.length === 0) {
      return true;
    }
    const token = (this.loadToken += 1);
    const generation = this.generation;
    const listing = await this.listDir(this.root, this.path);
    if (token !== this.loadToken || generation !== this.generation) {
      return true;
    }
    if (listing === null) {
      return false;
    }
    this.entries = listing.entries;
    this.truncated = listing.truncated;
    this.entryByName.clear();
    for (const entry of listing.entries) {
      this.entryByName.set(entry.name, entry);
    }
    for (const name of [...this.selection]) {
      if (!this.entryByName.has(name)) {
        this.selection.delete(name);
      }
    }
    if (this.path.length === 0) {
      this.quick.set(this.root, knownPlaces(listing.entries));
    }
    if (this.truncated) {
      this.say(this.texts.filesTruncated);
    }
    this.render();
    this.onLocation?.(this.root, [...this.path]);
    return true;
  }

  private async navigate(root: number, path: string[], record = true): Promise<void> {
    if (record) {
      this.back.push({ root: this.root, path: [...this.path] });
      this.forward.length = 0;
    }
    this.root = root;
    this.path = path;
    this.selection.clear();
    this.anchor = null;
    this.focused = null;
    this.renaming = null;
    this.search.value = '';
    this.filter = '';
    this.entries = [];
    this.entryByName.clear();
    this.render();
    this.onLocation?.(this.root, [...this.path]);
    await this.refresh();
  }

  private goBack(): void {
    const place = this.back.pop();
    if (place !== undefined) {
      this.forward.push({ root: this.root, path: [...this.path] });
      void this.navigate(place.root, place.path, false);
    }
  }

  private goForward(): void {
    const place = this.forward.pop();
    if (place !== undefined) {
      this.back.push({ root: this.root, path: [...this.path] });
      void this.navigate(place.root, place.path, false);
    }
  }

  private goUp(): void {
    if (this.path.length > 0) {
      void this.navigate(this.root, this.path.slice(0, -1));
    }
  }

  private get canRename(): boolean {
    return (this.features & FEATURE_RENAME) !== 0;
  }

  private visible(): FileEntry[] {
    const filtered =
      this.filter === '' ? this.entries : this.entries.filter((entry) => entry.name.toLowerCase().includes(this.filter));
    return sortEntries(filtered, this.preferences.sort, this.preferences.descending);
  }

  private say(text: string, sticky = false): void {
    window.clearTimeout(this.noticeTimer);
    this.notice = text;
    if (text !== '' && !sticky) {
      this.noticeTimer = window.setTimeout(() => {
        this.notice = '';
        this.renderStatus();
      }, 6000);
    }
    this.renderStatus();
  }

  private savePreferences(): void {
    try {
      localStorage.setItem(PREFERENCES_KEY, JSON.stringify(this.preferences));
    } catch {
      return;
    }
  }

  private toggleDual(): void {
    this.preferences.dual = !this.preferences.dual;
    this.savePreferences();
    this.host.classList.toggle('fm-dual-on', this.preferences.dual);
    this.renderTools();
  }

  private bridge(): RemoteBridge {
    return {
      root: () => this.root,
      dir: () => [...this.path],
      names: () => [...this.entryByName.keys()],
      selection: () => this.selectedEntries(),
      draggedNames: () => this.dragging?.names ?? null,
      list: async (root, dir) => (await this.listDir(root, dir))?.entries ?? null,
      upload: (root, dir, blob, name) => this.upload(blob, joinPath(dir, name), name, false, root),
      mkdir: (root, dir, name) => this.must(encodeMkdir(root, joinPath(dir, name))),
      download: (root, dir, entry, sink) => this.fetchToSink(joinPath(dir, entry.name), entry.name, entry.size, sink, root),
      refresh: () => this.refresh(),
      say: (text) => this.say(text),
      confirm: (message, ok, danger) => this.confirm(message, ok, danger),
    };
  }

  private toggleView(): void {
    this.preferences.view = this.preferences.view === 'grid' ? 'list' : 'grid';
    this.savePreferences();
    this.render();
  }

  private sortBy(key: SortKey, toggle: boolean): void {
    if (toggle && this.preferences.sort === key) {
      this.preferences.descending = !this.preferences.descending;
    } else {
      this.preferences.sort = key;
      this.preferences.descending = !toggle ? false : key === 'modified';
    }
    this.savePreferences();
    this.render();
  }

  private tool(iconId: string, label: string, run: () => void, withLabel = false): HTMLButtonElement {
    const button = node('button', 'fm-tool');
    button.type = 'button';
    button.append(svg(iconId));
    if (label !== '') {
      button.title = label;
      button.setAttribute('aria-label', label);
    }
    if (withLabel) {
      button.append(node('span', 'lbl', label));
    }
    button.addEventListener('click', run);
    return button;
  }

  private render(): void {
    const roots = this.roots ?? [];
    const has = roots.length > 0;
    this.host.classList.toggle('fm-empty-roots', !has);
    this.renderSide();
    this.renderCrumbs();
    this.renderTools();
    this.sortSelect.value = this.preferences.sort;
    this.renderList();
    this.renderStatus();
    this.renderProgress();
  }

  private renderTools(): void {
    const has = (this.roots ?? []).length > 0;
    const selected = this.selectedEntries();
    const single = selected.length === 1 ? selected[0] : undefined;
    const files = selected.filter((entry) => entry.kind === 'file');
    const set = (name: string, enabled: boolean): void => {
      const button = this.buttons[name];
      if (button !== undefined) {
        button.disabled = !enabled;
      }
    };
    set('back', this.back.length > 0);
    set('forward', this.forward.length > 0);
    set('up', this.path.length > 0);
    set('newFolder', has);
    set('newFile', has);
    set('upload', has);
    set('refresh', has);
    set('dual', has || this.preferences.dual);
    set('toLocal', has && files.length > 0);
    this.buttons['dual']?.setAttribute('aria-pressed', String(this.preferences.dual));
    this.buttons['dual']?.classList.toggle('on', this.preferences.dual);
    set('open', single !== undefined);
    set('edit', single !== undefined && isEditable(single) && this.canRename);
    set('download', files.length > 0);
    set('rename', single !== undefined && this.canRename);
    set('remove', selected.length > 0);
    const grid = this.preferences.view === 'grid';
    this.host.classList.toggle('fm-view-grid', grid);
    this.host.classList.toggle('fm-has-selection', selected.length > 0);
    const label = grid ? this.texts.filesViewList : this.texts.filesViewGrid;
    this.viewButton.title = label;
    this.viewButton.setAttribute('aria-label', label);
    this.viewButton.replaceChildren(svg(grid ? 'i-list' : 'i-grid'));
  }

  private renderSide(): void {
    const roots = this.roots ?? [];
    const sections: HTMLElement[] = [];
    if (roots.length > 0) {
      const places = node('div', 'fm-side-group');
      places.append(node('h3', undefined, this.texts.filesPlaces));
      roots.forEach((name, index) => {
        const active = index === this.root && this.path.length === 0;
        places.append(this.sideItem('i-drive', name === '' ? '/' : name, active, index, [], index === this.root && this.path.length > 0));
      });
      sections.push(places);
      const quick = this.quick.get(this.root) ?? [];
      if (quick.length > 0) {
        const group = node('div', 'fm-side-group');
        group.append(node('h3', undefined, this.texts.filesQuickAccess));
        for (const name of quick) {
          const active = this.path.length === 1 && this.path[0] === name;
          group.append(this.sideItem('i-folder', name, active, this.root, [name], false));
        }
        sections.push(group);
      }
    }
    this.side.replaceChildren(...sections);
  }

  private sideItem(icon: string, label: string, active: boolean, root: number, path: string[], within: boolean): HTMLElement {
    const item = node('button', 'fm-side-item');
    item.type = 'button';
    item.append(svg(icon), node('span', undefined, label));
    item.classList.toggle('active', active);
    item.classList.toggle('within', within);
    item.addEventListener('click', () => void this.navigate(root, path));
    this.makeDropTarget(item, root === this.root ? path : null);
    return item;
  }

  private renderCrumbs(): void {
    const roots = this.roots ?? [];
    const first = roots[this.root] ?? '';
    const parts = [first === '' ? '/' : first, ...this.path];
    const items: HTMLElement[] = [];
    parts.forEach((part, index) => {
      if (index > 0) {
        items.push(node('span', 'fm-sep', '/'));
      }
      const crumb = node('button', 'fm-crumb', part);
      crumb.type = 'button';
      const target = this.path.slice(0, index);
      if (index === parts.length - 1) {
        crumb.setAttribute('aria-current', 'page');
      }
      crumb.addEventListener('click', () => {
        if (index < parts.length - 1) {
          void this.navigate(this.root, target);
        }
      });
      this.makeDropTarget(crumb, target);
      items.push(crumb);
    });
    this.crumbs.replaceChildren(...items);
    const last = this.crumbs.lastElementChild;
    if (last instanceof HTMLElement) {
      last.scrollIntoView({ block: 'nearest', inline: 'end' });
    }
  }

  private renderList(): void {
    this.itemElements.clear();
    const roots = this.roots ?? [];
    this.list.classList.toggle('grid', this.preferences.view === 'grid');
    this.list.classList.toggle('details', this.preferences.view === 'list');
    if (this.roots === null && this.loadError !== null) {
      const retry = node('button', 'tool', this.texts.filesRefresh);
      retry.type = 'button';
      retry.addEventListener('click', () => {
        this.show();
        this.render();
      });
      const failed = node('div', 'fm-loading failed');
      failed.append(node('span', '', this.loadError), retry);
      this.list.replaceChildren(failed);
      return;
    }
    if (this.roots === null) {
      const waiting = node('div', 'fm-loading');
      waiting.setAttribute('role', 'status');
      const spinner = node('span', 'spinner');
      spinner.setAttribute('aria-hidden', 'true');
      waiting.append(spinner, node('span', '', this.texts.filesLoading));
      this.list.replaceChildren(waiting);
      return;
    }
    if (roots.length === 0) {
      this.list.replaceChildren(node('p', 'fm-empty', this.texts.filesNoRoots));
      return;
    }
    const visible = this.visible();
    if (visible.length === 0) {
      const empty = this.entries.length === 0 ? this.texts.filesEmpty : this.texts.filesNoMatches;
      this.list.replaceChildren(node('p', 'fm-empty', empty));
      return;
    }
    const children: HTMLElement[] = [];
    if (this.preferences.view === 'list') {
      children.push(this.header());
    }
    for (const entry of visible) {
      children.push(this.item(entry));
    }
    this.list.replaceChildren(...children);
    this.paintSelection();
  }

  private header(): HTMLElement {
    const row = node('div', 'fm-head');
    row.setAttribute('role', 'presentation');
    for (const [key, label, className] of [
      ['name', this.texts.filesColumnName, 'c-name'],
      ['modified', this.texts.filesColumnModified, 'c-date'],
      ['size', this.texts.filesColumnSize, 'c-size'],
    ] as const) {
      const cell = node('button', `fm-col ${className}`, label);
      cell.type = 'button';
      if (this.preferences.sort === key) {
        cell.classList.add('sorted');
        cell.append(svg(this.preferences.descending ? 'i-down' : 'i-up'));
      }
      cell.addEventListener('click', () => this.sortBy(key, true));
      row.append(cell);
    }
    return row;
  }

  private item(entry: FileEntry): HTMLElement {
    const category = categorize(entry);
    const item = node('div', `fm-item cat-${category}`);
    item.dataset['name'] = entry.name;
    item.setAttribute('role', 'option');
    item.draggable = this.renaming !== entry.name && this.canRename;
    const icon = node('span', 'fm-icon');
    icon.append(svg(CATEGORY_ICON[category]));
    item.append(icon);
    if (this.renaming === entry.name) {
      item.append(this.renameInput(entry));
    } else {
      const label = node('span', 'fm-name c-name', entry.name);
      label.title = entry.name;
      item.append(label);
    }
    if (this.preferences.view === 'list') {
      item.append(
        node('span', 'fm-meta c-date', entry.modified === 0 ? '' : formatDate(entry.modified)),
        node('span', 'fm-meta c-size', entry.kind === 'file' ? formatSize(entry.size) : ''),
      );
    }
    this.itemElements.set(entry.name, item);
    if (entry.kind === 'directory') {
      this.makeDropTarget(item, [...this.path, entry.name]);
    }
    return item;
  }

  private renameInput(entry: FileEntry): HTMLElement {
    const input = node('input', 'fm-rename');
    input.type = 'text';
    input.value = entry.name;
    input.maxLength = 255;
    input.spellcheck = false;
    input.autocomplete = 'off';
    input.setAttribute('aria-label', this.texts.filesRename);
    let finished = false;
    const finish = (commit: boolean): void => {
      if (finished) {
        return;
      }
      finished = true;
      this.renaming = null;
      if (commit) {
        void this.commitRename(entry, input.value.trim());
      } else {
        this.renderList();
        this.list.focus();
      }
    };
    input.addEventListener('keydown', (event) => {
      event.stopPropagation();
      if (event.key === 'Enter') {
        event.preventDefault();
        finish(true);
      } else if (event.key === 'Escape') {
        event.preventDefault();
        finish(false);
      }
    });
    input.addEventListener('blur', () => finish(true));
    input.addEventListener('pointerdown', (event) => event.stopPropagation());
    input.addEventListener('click', (event) => event.stopPropagation());
    input.addEventListener('dblclick', (event) => event.stopPropagation());
    queueMicrotask(() => {
      input.focus();
      const dot = entry.kind === 'file' ? entry.name.lastIndexOf('.') : -1;
      input.setSelectionRange(0, dot > 0 ? dot : entry.name.length);
    });
    return input;
  }

  private renderStatus(): void {
    const total = this.visible().length;
    const selected = this.selectedEntries();
    this.status.textContent = this.notice !== '' ? this.notice : this.texts.filesItems(total);
    if (selected.length > 0) {
      const bytes = selected.reduce((sum, entry) => sum + (entry.kind === 'file' ? entry.size : 0), 0);
      this.statusRight.textContent = this.texts.filesSelected(selected.length, bytes > 0 ? formatSize(bytes) : '');
    } else {
      this.statusRight.textContent = '';
    }
  }

  private renderProgress(): void {
    const items = [...this.transfers].map(([stream, transfer]) => {
      const percent = transfer.total === 0 ? 100 : Math.min(100, Math.floor((transfer.done * 100) / transfer.total));
      const line = node('div', 'fm-transfer');
      const label = node(
        'span',
        undefined,
        transfer.kind === 'download'
          ? this.texts.filesDownloading(transfer.name, percent)
          : this.texts.filesUploading(transfer.name, percent),
      );
      const bar = node('progress');
      bar.max = 100;
      bar.value = percent;
      const cancel = this.tool('i-close', this.texts.filesCancel, () => this.cancel(stream));
      line.append(label, bar, cancel);
      return line;
    });
    this.progress.replaceChildren(...items);
  }

  private selectedEntries(): FileEntry[] {
    return [...this.selection].map((name) => this.entryByName.get(name)).filter((entry): entry is FileEntry => entry !== undefined);
  }

  private paintSelection(): void {
    for (const [name, element] of this.itemElements) {
      const on = this.selection.has(name);
      element.classList.toggle('selected', on);
      element.setAttribute('aria-selected', String(on));
      element.classList.toggle('focused', name === this.focused);
    }
    this.renderTools();
    this.renderStatus();
  }

  private selectOnly(name: string): void {
    this.selection.clear();
    this.selection.add(name);
    this.anchor = name;
    this.focused = name;
    this.paintSelection();
  }

  private selectRange(name: string): void {
    const names = this.visible().map((entry) => entry.name);
    const from = names.indexOf(this.anchor ?? name);
    const to = names.indexOf(name);
    if (from < 0 || to < 0) {
      this.selectOnly(name);
      return;
    }
    this.selection.clear();
    for (const between of names.slice(Math.min(from, to), Math.max(from, to) + 1)) {
      this.selection.add(between);
    }
    this.focused = name;
    this.paintSelection();
  }

  private toggle(name: string): void {
    if (this.selection.has(name)) {
      this.selection.delete(name);
    } else {
      this.selection.add(name);
    }
    this.anchor = name;
    this.focused = name;
    this.paintSelection();
  }

  private clearSelection(): void {
    this.selection.clear();
    this.focused = null;
    this.paintSelection();
  }

  private itemAt(target: EventTarget | null): string | null {
    if (!(target instanceof Element)) {
      return null;
    }
    return target.closest<HTMLElement>('.fm-item')?.dataset['name'] ?? null;
  }

  private coarse(): boolean {
    return typeof matchMedia === 'function' && matchMedia('(pointer: coarse)').matches;
  }

  private bindList(): void {
    this.list.addEventListener('click', (event) => {
      if (this.pressed) {
        this.pressed = false;
        return;
      }
      this.closeMenu();
      const name = this.itemAt(event.target);
      if (name === null) {
        if (!(event.target instanceof Element && event.target.closest('.fm-head'))) {
          this.clearSelection();
        }
        return;
      }
      if (event.shiftKey) {
        this.selectRange(name);
      } else if (event.ctrlKey || event.metaKey) {
        this.toggle(name);
      } else if (this.coarse()) {
        if (this.selection.size > 0) {
          this.toggle(name);
        } else {
          this.selectOnly(name);
          this.openEntry(name);
        }
      } else {
        this.selectOnly(name);
      }
    });
    this.list.addEventListener('dblclick', (event) => {
      const name = this.itemAt(event.target);
      if (name !== null && !this.coarse()) {
        this.openEntry(name);
      }
    });
    this.list.addEventListener('contextmenu', (event) => {
      event.preventDefault();
      this.contextAt(event.target, event.clientX, event.clientY);
    });
    this.list.addEventListener('pointerdown', (event) => {
      if (event.pointerType !== 'touch') {
        return;
      }
      const { clientX, clientY, target } = event;
      window.clearTimeout(this.pressTimer);
      this.pressTimer = window.setTimeout(() => {
        this.pressed = true;
        this.contextAt(target, clientX, clientY);
      }, LONG_PRESS_MS);
    });
    for (const name of ['pointerup', 'pointercancel', 'pointermove', 'scroll'] as const) {
      this.list.addEventListener(name, () => window.clearTimeout(this.pressTimer), { passive: true });
    }
    this.list.addEventListener('keydown', (event) => this.listKey(event));
    this.list.addEventListener('dragstart', (event) => {
      const name = this.itemAt(event.target);
      if (name === null || event.dataTransfer === null) {
        event.preventDefault();
        return;
      }
      if (!this.selection.has(name)) {
        this.selectOnly(name);
      }
      const names = [...this.selection];
      this.dragging = { names };
      event.dataTransfer.effectAllowed = 'move';
      event.dataTransfer.setData(MOVE_TYPE, JSON.stringify(names));
      event.dataTransfer.setData('text/plain', names.join('\n'));
    });
    this.list.addEventListener('dragend', () => {
      this.dragging = null;
      this.clearDropMarks();
    });
    this.list.addEventListener('dragover', (event) => {
      if (this.local.isDragging() && !event.defaultPrevented) {
        event.preventDefault();
        this.dropHint.hidden = false;
        return;
      }
      if (this.hasFiles(event) && !event.defaultPrevented) {
        event.preventDefault();
        if (event.dataTransfer !== null) {
          event.dataTransfer.dropEffect = 'copy';
        }
        this.dropHint.hidden = false;
      }
    });
    this.list.addEventListener('dragleave', (event) => {
      if (event.target === this.list || !this.list.contains(event.relatedTarget as Node | null)) {
        this.dropHint.hidden = true;
      }
    });
    this.list.addEventListener('drop', (event) => {
      this.dropHint.hidden = true;
      if (this.local.isDragging() && !event.defaultPrevented) {
        event.preventDefault();
        this.local.sendSelection();
        return;
      }
      if (event.defaultPrevented || !this.hasFiles(event) || event.dataTransfer === null) {
        return;
      }
      event.preventDefault();
      void this.dropFiles(event.dataTransfer, this.path);
    });
  }

  private bindHost(): void {
    this.host.addEventListener('pointerdown', (event) => {
      if (this.menu.hidden || (event.target instanceof Node && this.menu.contains(event.target))) {
        return;
      }
      this.closeMenu();
    });
    this.host.addEventListener('keydown', (event) => {
      if (event.key === 'Escape') {
        this.closeMenu();
      }
      if (event.altKey && event.key === 'ArrowLeft') {
        event.preventDefault();
        this.goBack();
      } else if (event.altKey && event.key === 'ArrowRight') {
        event.preventDefault();
        this.goForward();
      } else if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'f' && !this.dialog.isOpen) {
        event.preventDefault();
        this.search.focus();
        this.search.select();
      }
    });
    window.addEventListener('blur', () => this.closeMenu());
    this.list.addEventListener('scroll', () => this.closeMenu(), { passive: true });
  }

  private listKey(event: KeyboardEvent): void {
    if (this.renaming !== null) {
      return;
    }
    const names = this.visible().map((entry) => entry.name);
    const modifier = event.ctrlKey || event.metaKey;
    switch (event.key) {
      case 'ArrowDown':
      case 'ArrowUp':
      case 'ArrowLeft':
      case 'ArrowRight': {
        if (event.altKey) {
          return;
        }
        event.preventDefault();
        const step = this.step(event.key, names);
        const current = this.focused === null ? -1 : names.indexOf(this.focused);
        const index = Math.max(0, Math.min(names.length - 1, current < 0 ? 0 : current + step));
        const target = names[index];
        if (target === undefined) {
          return;
        }
        if (event.shiftKey) {
          this.selectRange(target);
        } else {
          this.selectOnly(target);
        }
        this.itemElements.get(target)?.scrollIntoView({ block: 'nearest' });
        return;
      }
      case 'Enter':
        event.preventDefault();
        this.openSelected();
        return;
      case 'F2':
        event.preventDefault();
        this.startRename();
        return;
      case 'Delete':
        event.preventDefault();
        void this.deleteSelected();
        return;
      case 'Backspace':
        event.preventDefault();
        if (modifier && this.selection.size > 0) {
          void this.deleteSelected();
        } else {
          this.goUp();
        }
        return;
      case 'Escape':
        this.clearSelection();
        return;
      case 'Home':
      case 'End': {
        event.preventDefault();
        const target = event.key === 'Home' ? names[0] : names[names.length - 1];
        if (target !== undefined) {
          this.selectOnly(target);
          this.itemElements.get(target)?.scrollIntoView({ block: 'nearest' });
        }
        return;
      }
      default:
        break;
    }
    if (modifier && event.key.toLowerCase() === 'a') {
      event.preventDefault();
      this.selection.clear();
      for (const name of names) {
        this.selection.add(name);
      }
      this.paintSelection();
    } else if (!modifier && !event.altKey && event.key.length === 1 && event.key !== ' ') {
      this.findByTyping(event.key, names);
    }
  }

  private step(key: string, names: string[]): number {
    if (this.preferences.view === 'list') {
      return key === 'ArrowDown' || key === 'ArrowRight' ? 1 : -1;
    }
    if (key === 'ArrowLeft') return -1;
    if (key === 'ArrowRight') return 1;
    const first = names[0] === undefined ? undefined : this.itemElements.get(names[0]);
    let columns = 1;
    if (first !== undefined) {
      const top = first.offsetTop;
      columns = names.filter((name) => this.itemElements.get(name)?.offsetTop === top).length || 1;
    }
    return key === 'ArrowDown' ? columns : -columns;
  }

  private findByTyping(key: string, names: string[]): void {
    window.clearTimeout(this.typeaheadTimer);
    this.typeahead += key.toLowerCase();
    this.typeaheadTimer = window.setTimeout(() => {
      this.typeahead = '';
    }, 800);
    const hit = names.find((name) => name.toLowerCase().startsWith(this.typeahead));
    if (hit !== undefined) {
      this.selectOnly(hit);
      this.itemElements.get(hit)?.scrollIntoView({ block: 'nearest' });
    }
  }

  private contextAt(target: EventTarget | null, x: number, y: number): void {
    const name = this.itemAt(target);
    if (name !== null && !this.selection.has(name)) {
      this.selectOnly(name);
    } else if (name === null) {
      this.clearSelection();
    }
    this.openMenu(this.menuItems(), x, y);
  }

  private menuItems(): MenuItem[] {
    const t = this.texts;
    const selected = this.selectedEntries();
    const single = selected.length === 1 ? selected[0] : undefined;
    if (selected.length === 0) {
      return [
        { label: t.filesNewFolder, icon: 'i-folder-plus', run: () => void this.createFolder() },
        { label: t.filesNewFile, icon: 'i-file-plus', run: () => void this.createFile() },
        { label: t.filesUpload, icon: 'i-upload', run: () => this.picker.click() },
        'separator',
        { label: t.filesRefresh, icon: 'i-refresh', run: () => void this.refresh() },
      ];
    }
    const items: MenuItem[] = [];
    if (single !== undefined) {
      items.push({ label: t.filesOpen, icon: 'i-folder', run: () => this.openSelected() });
      if (isEditable(single) && this.canRename) {
        items.push({ label: t.filesEdit, icon: 'i-edit', run: () => this.editSelected() });
      }
    }
    if (selected.some((entry) => entry.kind === 'file')) {
      items.push({ label: t.filesDownload, icon: 'i-down', run: () => this.downloadSelected() });
    }
    if (single !== undefined && this.canRename) {
      items.push({ label: t.filesRename, icon: 'i-edit', run: () => this.startRename() });
    }
    items.push('separator', { label: t.filesRemove, icon: 'i-trash', run: () => void this.deleteSelected(), danger: true });
    return items;
  }

  private openMenu(items: MenuItem[], x: number, y: number): void {
    const children = items.map((item) => {
      if (item === 'separator') {
        return node('div', 'fm-menu-sep');
      }
      const button = node('button', item.danger === true ? 'fm-menu-item danger' : 'fm-menu-item');
      button.type = 'button';
      button.setAttribute('role', 'menuitem');
      button.append(svg(item.icon), node('span', undefined, item.label));
      button.addEventListener('click', () => {
        this.closeMenu();
        item.run();
      });
      return button;
    });
    this.menu.replaceChildren(...children);
    this.menu.hidden = false;
    const bounds = this.host.getBoundingClientRect();
    const width = this.menu.offsetWidth;
    const height = this.menu.offsetHeight;
    const left = Math.max(4, Math.min(x - bounds.left, bounds.width - width - 4));
    const top = Math.max(4, Math.min(y - bounds.top, bounds.height - height - 4));
    this.menu.style.left = `${left}px`;
    this.menu.style.top = `${top}px`;
    this.menu.querySelector<HTMLElement>('.fm-menu-item')?.focus();
  }

  private closeMenu(): void {
    this.menu.hidden = true;
    this.menu.replaceChildren();
  }

  private confirm(message: string, okLabel: string, danger: boolean): Promise<boolean> {
    this.closeModal(false);
    return new Promise((resolve) => {
      const box = node('div', 'fm-dialog fm-confirm');
      box.setAttribute('role', 'alertdialog');
      box.setAttribute('aria-modal', 'true');
      const text = node('p', undefined, message);
      const actions = node('div', 'fm-confirm-actions');
      const cancel = node('button', 'fm-tool', this.texts.filesCancel);
      cancel.type = 'button';
      const ok = node('button', danger ? 'fm-tool danger solid' : 'fm-tool primary', okLabel);
      ok.type = 'button';
      actions.append(cancel, ok);
      box.append(text, actions);
      this.modal.replaceChildren(box);
      this.modal.hidden = false;
      this.modalResolve = resolve;
      cancel.addEventListener('click', () => this.closeModal(false));
      ok.addEventListener('click', () => this.closeModal(true));
      this.modal.onkeydown = (event) => {
        event.stopPropagation();
        if (event.key === 'Escape') {
          event.preventDefault();
          this.closeModal(false);
        }
      };
      (danger ? cancel : ok).focus();
    });
  }

  private modalResolve: ((value: boolean) => void) | null = null;

  private closeModal(value: boolean): void {
    const resolve = this.modalResolve;
    this.modalResolve = null;
    this.modal.hidden = true;
    this.modal.replaceChildren();
    resolve?.(value);
  }

  private openEntry(name: string): void {
    const entry = this.entryByName.get(name);
    if (entry === undefined) {
      return;
    }
    const path = joinPath(this.path, name);
    if (entry.kind === 'directory') {
      void this.navigate(this.root, [...this.path, name]);
    } else if (entry.kind === 'file' && isPreviewable(entry)) {
      void this.dialog.preview(path, entry);
    } else if (entry.kind === 'file' && isEditable(entry) && this.canRename) {
      void this.dialog.edit(path, entry);
    } else if (entry.kind === 'file') {
      void this.saveToDisk(path, entry);
    }
  }

  private openSelected(): void {
    const [name] = [...this.selection];
    if (this.selection.size === 1 && name !== undefined) {
      this.openEntry(name);
    }
  }

  private editSelected(): void {
    const [entry] = this.selectedEntries();
    if (this.selection.size === 1 && entry !== undefined && isEditable(entry) && this.canRename) {
      void this.dialog.edit(joinPath(this.path, entry.name), entry);
    }
  }

  private downloadSelected(): void {
    const files = this.selectedEntries().filter((entry) => entry.kind === 'file');
    const path = [...this.path];
    const root = this.root;
    void (async () => {
      for (const entry of files) {
        await this.saveToDisk(joinPath(path, entry.name), entry, root);
      }
    })();
  }

  private startRename(): void {
    const [name] = [...this.selection];
    if (this.selection.size !== 1 || name === undefined || !this.canRename) {
      return;
    }
    this.renaming = name;
    this.renderList();
    this.paintSelection();
  }

  private async commitRename(entry: FileEntry, wanted: string): Promise<void> {
    if (wanted === entry.name || wanted === '') {
      this.renderList();
      this.paintSelection();
      this.list.focus();
      return;
    }
    if (!validName(wanted)) {
      this.say(this.texts.filesInvalidName);
      this.renderList();
      this.paintSelection();
      return;
    }
    const ok = await this.must(
      encodeRename(this.root, joinPath(this.path, entry.name), joinPath(this.path, wanted), false),
    );
    if (ok) {
      this.selection.clear();
      this.selection.add(wanted);
      this.focused = wanted;
      this.anchor = wanted;
    }
    await this.refresh();
    this.paintSelection();
    this.list.focus();
  }

  private async createFolder(): Promise<void> {
    const name = uniqueName(this.entryByName.keys(), this.texts.filesNewFolderName);
    if (!(await this.must(encodeMkdir(this.root, joinPath(this.path, name))))) {
      return;
    }
    await this.refresh();
    this.selectOnly(name);
    this.startRename();
  }

  private async createFile(): Promise<void> {
    const name = uniqueName(this.entryByName.keys(), this.texts.filesNewFileName);
    if (!(await this.upload(new Blob([]), joinPath(this.path, name), name))) {
      return;
    }
    await this.refresh();
    this.selectOnly(name);
    this.startRename();
  }

  private async deleteSelected(): Promise<void> {
    const names = [...this.selection];
    const first = names[0];
    if (first === undefined) {
      return;
    }
    if (!(await this.confirm(this.texts.filesConfirmRemove(names.length, first), this.texts.filesRemove, true))) {
      this.list.focus();
      return;
    }
    const generation = this.generation;
    const root = this.root;
    const base = [...this.path];
    const targets = names.flatMap((name) => {
      const entry = this.entryByName.get(name);
      return entry === undefined ? [] : [entry];
    });
    for (const entry of targets) {
      this.say(this.texts.filesRemoving(entry.name), true);
      const ok =
        entry.kind === 'directory'
          ? await this.removeTree(root, [...base, entry.name], 0)
          : await this.must(encodeRemove(root, joinPath(base, entry.name)));
      if (!ok || generation !== this.generation) {
        break;
      }
    }
    if (generation === this.generation) {
      if (this.notice !== '' && this.status.textContent === this.notice) {
        this.say('');
      }
      this.selection.clear();
      await this.refresh();
      this.list.focus();
    }
  }

  private async removeTree(root: number, parts: string[], depth: number): Promise<boolean> {
    if (depth > MAX_TREE_DEPTH) {
      this.say(this.texts.filesFailure.invalidPath);
      return false;
    }
    for (let round = 0; round < 200; round += 1) {
      const listing = await this.listDir(root, parts);
      if (listing === null) {
        return false;
      }
      if (listing.entries.length === 0) {
        break;
      }
      for (const child of listing.entries) {
        const childParts = [...parts, child.name];
        const ok =
          child.kind === 'directory'
            ? await this.removeTree(root, childParts, depth + 1)
            : await this.must(encodeRemove(root, joinPath(childParts)));
        if (!ok) {
          return false;
        }
      }
      if (!listing.truncated) {
        break;
      }
    }
    return this.must(encodeRemove(root, joinPath(parts)));
  }

  private makeDropTarget(element: HTMLElement, destination: string[] | null): void {
    if (destination === null) {
      return;
    }
    const accepts = (event: DragEvent): boolean => this.dragging !== null || this.hasFiles(event);
    element.addEventListener('dragover', (event) => {
      if (!accepts(event) || event.dataTransfer === null) {
        return;
      }
      if (this.dragging !== null) {
        const same = joinPath(destination) === joinPath(this.path);
        const intoItself = this.dragging.names.some((name) => joinPath(destination) === joinPath(this.path, name));
        if (same || intoItself) {
          return;
        }
      }
      event.preventDefault();
      event.dataTransfer.dropEffect = this.dragging !== null ? 'move' : 'copy';
      element.classList.add('drop');
    });
    element.addEventListener('dragleave', () => element.classList.remove('drop'));
    element.addEventListener('drop', (event) => {
      element.classList.remove('drop');
      if (event.defaultPrevented || !accepts(event) || event.dataTransfer === null) {
        return;
      }
      event.preventDefault();
      this.dropHint.hidden = true;
      if (this.dragging !== null) {
        const names = this.dragging.names;
        this.dragging = null;
        void this.moveInto(names, destination);
      } else {
        void this.dropFiles(event.dataTransfer, destination);
      }
    });
  }

  private clearDropMarks(): void {
    for (const marked of this.host.querySelectorAll('.drop')) {
      marked.classList.remove('drop');
    }
  }

  private hasFiles(event: DragEvent): boolean {
    return event.dataTransfer !== null && [...event.dataTransfer.types].includes('Files');
  }

  private async moveInto(names: string[], destination: string[]): Promise<void> {
    const root = this.root;
    const base = [...this.path];
    let moved = 0;
    for (const name of names) {
      const from = joinPath(base, name);
      const to = joinPath(destination, name);
      if (from === to || (joinPath(destination) + '/').startsWith(`${from}/`)) {
        continue;
      }
      if (await this.must(encodeRename(root, from, to, false))) {
        moved += 1;
      }
    }
    if (moved > 0) {
      this.say(this.texts.filesMoved(moved));
    }
    this.selection.clear();
    await this.refresh();
  }

  private async dropFiles(data: DataTransfer, destination: string[]): Promise<void> {
    const entries = [...data.items]
      .map((item) => (typeof item.webkitGetAsEntry === 'function' ? item.webkitGetAsEntry() : null))
      .filter((entry): entry is FileSystemEntry => entry !== null);
    if (entries.length === 0) {
      await this.uploadFiles([...data.files], destination);
      return;
    }
    await this.uploadTree(entries, destination);
  }

  private async uploadFiles(files: File[], destination: string[]): Promise<void> {
    const taken = new Set(joinPath(destination) === joinPath(this.path) ? this.entryByName.keys() : []);
    for (const file of files) {
      const name = uniqueName(taken, file.name);
      taken.add(name);
      if (!(await this.upload(file, joinPath(destination, name), name))) {
        break;
      }
    }
    await this.refresh();
  }

  private async uploadTree(entries: FileSystemEntry[], destination: string[]): Promise<void> {
    const taken = new Set(joinPath(destination) === joinPath(this.path) ? this.entryByName.keys() : []);
    const walk = async (entry: FileSystemEntry, into: string[], top: boolean): Promise<boolean> => {
      const name = top ? uniqueName(taken, entry.name) : entry.name;
      if (top) {
        taken.add(name);
      }
      if (entry.isFile) {
        const file = await new Promise<File | null>((resolve) =>
          (entry as FileSystemFileEntry).file(resolve, () => resolve(null)),
        );
        return file === null ? true : this.upload(file, joinPath(into, name), name);
      }
      if (!(await this.must(encodeMkdir(this.root, joinPath(into, name))))) {
        return false;
      }
      const reader = (entry as FileSystemDirectoryEntry).createReader();
      for (;;) {
        const batch = await new Promise<FileSystemEntry[]>((resolve) => reader.readEntries(resolve, () => resolve([])));
        if (batch.length === 0) {
          return true;
        }
        for (const child of batch) {
          if (!(await walk(child, [...into, name], false))) {
            return false;
          }
        }
      }
    };
    for (const entry of entries) {
      if (!(await walk(entry, destination, true))) {
        break;
      }
    }
    await this.refresh();
  }

  private async stat(path: string): Promise<FileEntry | null> {
    const parts = path.split('/');
    const name = parts.pop() ?? '';
    const listing = await this.listDir(this.root, parts);
    return listing?.entries.find((entry) => entry.name === name) ?? null;
  }

  private async readFile(path: string): Promise<Uint8Array | null> {
    const parts = await this.fetch(path, path.split('/').pop() ?? path, 0);
    if (parts === null) {
      return null;
    }
    const total = parts.reduce((sum, part) => sum + part.length, 0);
    const out = new Uint8Array(total);
    let offset = 0;
    for (const part of parts) {
      out.set(part, offset);
      offset += part.length;
    }
    return out;
  }

  private async writeFile(path: string, data: Uint8Array): Promise<boolean> {
    const parts = path.split('/');
    const name = parts.pop() ?? '';
    const temp = joinPath(parts, `.${name}.${Date.now().toString(36)}.wirehaus-edit`);
    if (!(await this.upload(new Blob([data as BlobPart]), temp, name, true))) {
      return false;
    }
    if (await this.must(encodeRename(this.root, temp, path, true))) {
      return true;
    }
    await this.call(encodeRemove(this.root, temp));
    return false;
  }

  private async saveToDisk(path: string, entry: FileEntry, root = this.root): Promise<void> {
    if (entry.size > MAX_MEMORY_DOWNLOAD) {
      this.say(this.texts.filesTooBig(entry.name));
      return;
    }
    const parts = await this.fetch(path, entry.name, entry.size, root);
    if (parts !== null) {
      saveBlob(new Blob(parts as BlobPart[]), entry.name);
      this.say(this.texts.filesFinished(entry.name));
    }
  }

  private fetch(path: string, name: string, size: number, root = this.root): Promise<Uint8Array[] | null> {
    const stream = this.next;
    this.next += 1;
    return new Promise((resolve) => {
      const transfer: Transfer = {
        sink: null,
        writing: Promise.resolve(),
        name,
        kind: 'download',
        total: size,
        done: 0,
        parts: [],
        acked: 0,
        waiting: null,
        cancelled: false,
        source: null,
        settle: (ok) => resolve(ok ? transfer.parts : null),
      };
      this.transfers.set(stream, transfer);
      this.io.send(stream, encodeDownload(root, path));
      this.renderProgress();
    });
  }

  private fetchToSink(path: string, name: string, size: number, sink: Sink, root = this.root): Promise<boolean> {
    const stream = this.next;
    this.next += 1;
    return new Promise((resolve) => {
      const transfer: Transfer = {
        sink,
        writing: Promise.resolve(),
        name,
        kind: 'download',
        total: size,
        done: 0,
        parts: [],
        acked: 0,
        waiting: null,
        cancelled: false,
        source: null,
        settle: (ok) => resolve(ok),
      };
      this.transfers.set(stream, transfer);
      this.io.send(stream, encodeDownload(root, path));
      this.renderProgress();
    });
  }

  private upload(source: Blob, path: string, name: string, quiet = false, root = this.root): Promise<boolean> {
    const stream = this.next;
    this.next += 1;
    return new Promise((resolve) => {
      this.transfers.set(stream, {
        sink: null,
        writing: Promise.resolve(),
        name,
        kind: 'upload',
        total: source.size,
        done: 0,
        parts: [],
        acked: 0,
        waiting: null,
        cancelled: false,
        source,
        settle: (ok) => {
          if (ok && !quiet) {
            this.say(this.texts.filesFinished(name));
          }
          resolve(ok);
        },
      });
      this.io.send(stream, encodeUpload(root, path, source.size));
      this.renderProgress();
    });
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
      case 'data': {
        const chunk = message.data.slice();
        const before = transfer.done;
        transfer.done += chunk.length;
        const crossed = Math.floor(transfer.done / ACK_STEP) !== Math.floor(before / ACK_STEP);
        const total = transfer.done;
        if (transfer.sink === null) {
          transfer.parts.push(chunk);
          if (crossed) {
            this.io.send(stream, encodeAck(total));
          }
          break;
        }
        const sink = transfer.sink;
        transfer.writing = transfer.writing
          .then(() => sink.write(chunk))
          .then(() => {
            if (crossed && this.transfers.has(stream)) {
              this.io.send(stream, encodeAck(total));
            }
          })
          .catch(() => {
            if (this.transfers.has(stream)) {
              this.io.send(stream, encodeCancel());
              this.end(stream);
              this.say(`${transfer.name}: ${this.texts.filesFailure.io}`);
              void sink.abort().finally(() => transfer.settle(false));
            }
          });
        break;
      }
      case 'ack':
        transfer.acked = message.bytes;
        transfer.waiting?.();
        break;
      case 'done':
        this.end(stream);
        if (transfer.sink === null) {
          transfer.settle(true);
        } else {
          const sink = transfer.sink;
          void transfer.writing.then(
            () => sink.close().then(() => transfer.settle(true), () => transfer.settle(false)),
            () => transfer.settle(false),
          );
        }
        break;
      case 'failed':
        this.end(stream);
        this.say(`${transfer.name}: ${this.texts.filesFailure[message.reason as FileFailure]}`);
        void (transfer.sink?.abort() ?? Promise.resolve()).finally(() => transfer.settle(false));
        break;
      default:
        this.end(stream);
        void (transfer.sink?.abort() ?? Promise.resolve()).finally(() => transfer.settle(false));
        break;
    }
    this.renderProgress();
  }

  private async pump(stream: number, transfer: Transfer): Promise<void> {
    const source = transfer.source;
    if (source === null) {
      return;
    }
    let offset = 0;
    while (offset < source.size && !transfer.cancelled) {
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
      const end = Math.min(offset + MAX_FILE_CHUNK, source.size);
      const chunk = new Uint8Array(await source.slice(offset, end).arrayBuffer());
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
    const transfer = this.transfers.get(stream);
    this.io.send(stream, encodeCancel());
    this.end(stream);
    void (transfer?.sink?.abort() ?? Promise.resolve()).finally(() => transfer?.settle(false));
    this.renderProgress();
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
