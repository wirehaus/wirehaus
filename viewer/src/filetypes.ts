import type { FileEntry } from './filesprotocol.js';

export type FileCategory =
  | 'folder'
  | 'text'
  | 'code'
  | 'image'
  | 'audio'
  | 'video'
  | 'archive'
  | 'document'
  | 'other';

export type SortKey = 'name' | 'size' | 'modified';

const TEXT = new Set(['txt', 'md', 'markdown', 'log', 'csv', 'tsv', 'ini', 'cfg', 'conf', 'env', 'rst', 'tex']);
const CODE = new Set([
  'js', 'mjs', 'cjs', 'ts', 'tsx', 'jsx', 'json', 'jsonc', 'yml', 'yaml', 'toml', 'xml', 'html', 'htm', 'css', 'scss',
  'py', 'rs', 'go', 'c', 'h', 'cpp', 'hpp', 'cc', 'java', 'kt', 'rb', 'php', 'sh', 'bash', 'zsh', 'fish', 'ps1',
  'bat', 'cmd', 'sql', 'lua', 'pl', 'swift', 'cs', 'vue', 'svelte', 'service', 'desktop', 'lock', 'gitignore',
]);
const IMAGE = new Set(['png', 'jpg', 'jpeg', 'gif', 'webp', 'bmp', 'ico', 'svg', 'avif']);
const AUDIO = new Set(['mp3', 'wav', 'flac', 'ogg', 'opus', 'm4a', 'aac']);
const VIDEO = new Set(['mp4', 'mkv', 'webm', 'mov', 'avi', 'm4v']);
const ARCHIVE = new Set(['zip', 'tar', 'gz', 'tgz', 'bz2', 'xz', '7z', 'rar', 'zst', 'deb', 'rpm', 'iso']);
const DOCUMENT = new Set(['pdf', 'doc', 'docx', 'odt', 'xls', 'xlsx', 'ods', 'ppt', 'pptx', 'odp', 'epub']);
const TEXT_NAMES = new Set(['makefile', 'dockerfile', 'readme', 'license', 'authors', 'changelog', 'notice']);

export const MAX_EDIT_BYTES = 2 * 1024 * 1024;
export const MAX_PREVIEW_BYTES = 16 * 1024 * 1024;

export function extension(name: string): string {
  const dot = name.lastIndexOf('.');
  return dot <= 0 ? '' : name.slice(dot + 1).toLowerCase();
}

export function categorize(entry: Pick<FileEntry, 'name' | 'kind'>): FileCategory {
  if (entry.kind === 'directory') {
    return 'folder';
  }
  if (entry.kind === 'other') {
    return 'other';
  }
  const ext = extension(entry.name);
  const lower = entry.name.toLowerCase();
  if (CODE.has(ext)) return 'code';
  if (TEXT.has(ext) || TEXT_NAMES.has(lower) || (lower.startsWith('.') && ext === '')) return 'text';
  if (IMAGE.has(ext)) return 'image';
  if (AUDIO.has(ext)) return 'audio';
  if (VIDEO.has(ext)) return 'video';
  if (ARCHIVE.has(ext)) return 'archive';
  if (DOCUMENT.has(ext)) return 'document';
  return 'other';
}

export const isEditable = (entry: FileEntry): boolean =>
  entry.kind === 'file' && entry.size <= MAX_EDIT_BYTES && ['text', 'code'].includes(categorize(entry));

export const isPreviewable = (entry: FileEntry): boolean =>
  entry.kind === 'file' && entry.size <= MAX_PREVIEW_BYTES && categorize(entry) === 'image';

export function mimeFor(name: string): string {
  switch (extension(name)) {
    case 'svg':
      return 'image/svg+xml';
    case 'jpg':
    case 'jpeg':
      return 'image/jpeg';
    case 'ico':
      return 'image/x-icon';
    default:
      return `image/${extension(name)}`;
  }
}

export function sortEntries(entries: FileEntry[], key: SortKey, descending: boolean): FileEntry[] {
  const collator = new Intl.Collator(undefined, { numeric: true, sensitivity: 'base' });
  const sign = descending ? -1 : 1;
  return [...entries].sort((a, b) => {
    const aDir = a.kind === 'directory';
    const bDir = b.kind === 'directory';
    if (aDir !== bDir) {
      return aDir ? -1 : 1;
    }
    let order = 0;
    if (key === 'size') {
      order = a.size - b.size;
    } else if (key === 'modified') {
      order = a.modified - b.modified;
    }
    if (order === 0) {
      order = collator.compare(a.name, b.name);
      return order * (key === 'name' ? sign : 1);
    }
    return order * sign;
  });
}

export function validName(name: string): boolean {
  return (
    name.length > 0 &&
    name.length <= 255 &&
    name !== '.' &&
    name !== '..' &&
    !/[\\/:\u0000-\u001f\u007f]/u.test(name)
  );
}

export function uniqueName(existing: Iterable<string>, base: string): string {
  const taken = new Set(existing);
  if (!taken.has(base)) {
    return base;
  }
  const ext = extension(base);
  const stem = ext === '' ? base : base.slice(0, base.length - ext.length - 1);
  for (let n = 2; n < 10_000; n += 1) {
    const candidate = ext === '' ? `${stem} ${n}` : `${stem} ${n}.${ext}`;
    if (!taken.has(candidate)) {
      return candidate;
    }
  }
  return `${base}-${Date.now()}`;
}

export function joinPath(parts: readonly string[], name?: string): string {
  return (name === undefined ? [...parts] : [...parts, name]).join('/');
}

const KNOWN_PLACES = [
  ['desktop', 'escritorio'],
  ['documents', 'documentos'],
  ['downloads', 'descargas'],
  ['pictures', 'imágenes', 'imagenes'],
  ['music', 'música', 'musica'],
  ['videos', 'vídeos', 'video'],
] as const;

export function knownPlaces(entries: readonly FileEntry[]): string[] {
  const folders = new Map(entries.filter((e) => e.kind === 'directory').map((e) => [e.name.toLowerCase(), e.name]));
  const found: string[] = [];
  for (const aliases of KNOWN_PLACES) {
    const hit = aliases.map((alias) => folders.get(alias)).find((name) => name !== undefined);
    if (hit !== undefined) {
      found.push(hit);
    }
  }
  return found;
}

export function looksBinary(bytes: Uint8Array): boolean {
  const sample = bytes.subarray(0, 8192);
  return sample.includes(0);
}
