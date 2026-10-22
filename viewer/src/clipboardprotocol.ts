import { ProtocolError } from './protocol.js';

export const CLIPBOARD_CHANNEL = 3;
export const MAX_CLIPBOARD_BYTES = 32 * 1024;

export type ClipboardFailure = 'notAllowed' | 'tooLarge' | 'unavailable' | 'timeout';

export type ClipboardMessage =
  | { kind: 'content'; text: string }
  | { kind: 'empty' }
  | { kind: 'written' }
  | { kind: 'failed'; reason: ClipboardFailure };

const FAILURES: ClipboardFailure[] = ['notAllowed', 'tooLarge', 'unavailable', 'timeout'];

export function encodeClipboardWrite(text: string): Uint8Array {
  const bytes = new TextEncoder().encode(text);
  if (bytes.length === 0 || bytes.length > MAX_CLIPBOARD_BYTES) {
    throw new ProtocolError('clipboard text out of range');
  }
  const out = new Uint8Array(1 + bytes.length);
  out.set(bytes, 1);
  return out;
}

export const encodeClipboardRead = (): Uint8Array => Uint8Array.of(1);

export function clipboardFits(text: string): boolean {
  const length = new TextEncoder().encode(text).length;
  return length > 0 && length <= MAX_CLIPBOARD_BYTES;
}

export function decodeClipboard(payload: Uint8Array): ClipboardMessage {
  const tag = payload[0];
  if (tag === undefined) {
    throw new ProtocolError('empty clipboard message');
  }
  switch (tag) {
    case 2: {
      if (payload.length < 2 || payload.length > 1 + MAX_CLIPBOARD_BYTES) {
        throw new ProtocolError('bad clipboard text length');
      }
      try {
        return { kind: 'content', text: new TextDecoder('utf-8', { fatal: true }).decode(payload.subarray(1)) };
      } catch {
        throw new ProtocolError('clipboard text is not valid UTF-8');
      }
    }
    case 3:
    case 4:
      if (payload.length !== 1) {
        throw new ProtocolError('bad clipboard message length');
      }
      return { kind: tag === 3 ? 'empty' : 'written' };
    case 5: {
      if (payload.length !== 2) {
        throw new ProtocolError('bad clipboard failure length');
      }
      const reason = FAILURES[payload[1] ?? 255];
      if (reason === undefined) {
        throw new ProtocolError('unknown clipboard failure');
      }
      return { kind: 'failed', reason };
    }
    default:
      throw new ProtocolError(`unknown clipboard message ${tag}`);
  }
}
