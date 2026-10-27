import { ProtocolError } from './protocol.js';

export const FILES_CHANNEL = 2;
export const MAX_FILE_CHUNK = 32 * 1024;
export const MAX_PATH_LEN = 4096;
export const MAX_NAME_LEN = 255;
export const WINDOW_BYTES = 1 << 20;
export const FEATURE_RENAME = 1;

export type EntryKind = 'file' | 'directory' | 'other';

export type FileEntry = {
  name: string;
  kind: EntryKind;
  size: number;
  modified: number;
};

export type FileFailure =
  | 'notAllowed'
  | 'noSuchRoot'
  | 'invalidPath'
  | 'notFound'
  | 'alreadyExists'
  | 'notADirectory'
  | 'notAFile'
  | 'notEmpty'
  | 'tooLarge'
  | 'busy'
  | 'io';

export type FilesMessage =
  | { kind: 'roots'; names: string[]; features: number }
  | { kind: 'listing'; entries: FileEntry[]; truncated: boolean }
  | { kind: 'ready'; size: number }
  | { kind: 'data'; data: Uint8Array }
  | { kind: 'done' }
  | { kind: 'failed'; reason: FileFailure }
  | { kind: 'ack'; bytes: number };

const FAILURES: FileFailure[] = [
  'notAllowed',
  'noSuchRoot',
  'invalidPath',
  'notFound',
  'alreadyExists',
  'notADirectory',
  'notAFile',
  'notEmpty',
  'tooLarge',
  'busy',
  'io',
];

const KINDS: EntryKind[] = ['file', 'directory', 'other'];
const MAX_ROOTS = 16;
const MAX_ENTRIES = 1000;
const MAX_SAFE = BigInt(Number.MAX_SAFE_INTEGER);

class Reader {
  private at = 0;
  private readonly view: DataView;
  private readonly decoder = new TextDecoder('utf-8', { fatal: true });

  constructor(private readonly bytes: Uint8Array) {
    this.view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  }

  private take(count: number): number {
    if (this.at + count > this.bytes.length) {
      throw new ProtocolError('truncated files message');
    }
    const start = this.at;
    this.at += count;
    return start;
  }

  u8(): number {
    return this.view.getUint8(this.take(1));
  }

  u16(): number {
    return this.view.getUint16(this.take(2));
  }

  u64(): number {
    const value = this.view.getBigUint64(this.take(8));
    if (value > MAX_SAFE) {
      throw new ProtocolError('number out of range');
    }
    return Number(value);
  }

  text(limit: number): string {
    const length = this.u16();
    if (length > limit) {
      throw new ProtocolError('text too long');
    }
    const start = this.take(length);
    try {
      return this.decoder.decode(this.bytes.subarray(start, start + length));
    } catch {
      throw new ProtocolError('text is not valid UTF-8');
    }
  }

  flag(): boolean {
    const value = this.u8();
    if (value > 1) {
      throw new ProtocolError('bad flag');
    }
    return value === 1;
  }

  atEnd(): boolean {
    return this.at === this.bytes.length;
  }

  finish(): void {
    if (this.at !== this.bytes.length) {
      throw new ProtocolError('trailing bytes in files message');
    }
  }
}

export function decodeFiles(payload: Uint8Array): FilesMessage {
  const reader = new Reader(payload);
  const tag = reader.u8();
  let message: FilesMessage;
  switch (tag) {
    case 7: {
      const count = reader.u8();
      if (count > MAX_ROOTS) {
        throw new ProtocolError('too many roots');
      }
      const names: string[] = [];
      for (let index = 0; index < count; index += 1) {
        names.push(reader.text(MAX_PATH_LEN));
      }
      message = { kind: 'roots', names, features: reader.atEnd() ? 0 : reader.u8() };
      break;
    }
    case 8: {
      const truncated = reader.flag();
      const count = reader.u16();
      if (count > MAX_ENTRIES) {
        throw new ProtocolError('too many entries');
      }
      const entries: FileEntry[] = [];
      for (let index = 0; index < count; index += 1) {
        const name = reader.text(MAX_NAME_LEN);
        const kind = KINDS[reader.u8()];
        if (kind === undefined) {
          throw new ProtocolError('unknown entry kind');
        }
        entries.push({ name, kind, size: reader.u64(), modified: reader.u64() });
      }
      message = { kind: 'listing', entries, truncated };
      break;
    }
    case 9:
      message = { kind: 'ready', size: reader.u64() };
      break;
    case 10:
      if (payload.length < 2 || payload.length > 1 + MAX_FILE_CHUNK) {
        throw new ProtocolError('bad data length');
      }
      return { kind: 'data', data: payload.subarray(1) };
    case 11:
      message = { kind: 'done' };
      break;
    case 12: {
      const reason = FAILURES[reader.u8()];
      if (reason === undefined) {
        throw new ProtocolError('unknown failure');
      }
      message = { kind: 'failed', reason };
      break;
    }
    case 13:
      message = { kind: 'ack', bytes: reader.u64() };
      break;
    default:
      throw new ProtocolError(`unknown files message ${tag}`);
  }
  reader.finish();
  return message;
}

function pathMessage(tag: number, root: number, path: string, extra = 0): Uint8Array {
  const text = new TextEncoder().encode(path);
  if (root < 0 || root > 255 || text.length > MAX_PATH_LEN) {
    throw new ProtocolError('path out of range');
  }
  const out = new Uint8Array(4 + text.length + extra);
  const view = new DataView(out.buffer);
  view.setUint8(0, tag);
  view.setUint8(1, root);
  view.setUint16(2, text.length);
  out.set(text, 4);
  return out;
}

export const encodeListRoots = (): Uint8Array => Uint8Array.of(0);
export const encodeList = (root: number, path: string): Uint8Array => pathMessage(1, root, path);
export const encodeDownload = (root: number, path: string): Uint8Array => pathMessage(2, root, path);
export const encodeMkdir = (root: number, path: string): Uint8Array => pathMessage(4, root, path);
export const encodeRemove = (root: number, path: string): Uint8Array => pathMessage(5, root, path);
export function encodeRename(root: number, from: string, to: string, replace: boolean): Uint8Array {
  const head = pathMessage(14, root, from);
  const target = new TextEncoder().encode(to);
  if (target.length > MAX_PATH_LEN) {
    throw new ProtocolError('path out of range');
  }
  const out = new Uint8Array(head.length + 2 + target.length + 1);
  out.set(head, 0);
  new DataView(out.buffer).setUint16(head.length, target.length);
  out.set(target, head.length + 2);
  out[out.length - 1] = replace ? 1 : 0;
  return out;
}
export const encodeCancel = (): Uint8Array => Uint8Array.of(6);
export const encodeDone = (): Uint8Array => Uint8Array.of(11);

export function encodeUpload(root: number, path: string, size: number): Uint8Array {
  if (!Number.isSafeInteger(size) || size < 0) {
    throw new ProtocolError('size out of range');
  }
  const out = pathMessage(3, root, path, 8);
  new DataView(out.buffer).setBigUint64(out.length - 8, BigInt(size));
  return out;
}

export function encodeData(data: Uint8Array): Uint8Array {
  if (data.length === 0 || data.length > MAX_FILE_CHUNK) {
    throw new ProtocolError('data chunk out of range');
  }
  const out = new Uint8Array(1 + data.length);
  out[0] = 10;
  out.set(data, 1);
  return out;
}

export function encodeAck(bytes: number): Uint8Array {
  if (!Number.isSafeInteger(bytes) || bytes < 0) {
    throw new ProtocolError('acknowledgement out of range');
  }
  const out = new Uint8Array(9);
  out[0] = 13;
  new DataView(out.buffer).setBigUint64(1, BigInt(bytes));
  return out;
}
