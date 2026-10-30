import { ProtocolError } from './protocol.js';

export const LINK_CHANNEL = 7;
export const LINK_ID_LEN = 16;

export type LinkFailure = 'wrongCode' | 'tooManyAttempts' | 'gone' | 'unavailable';

export type LinkReply =
  | { kind: 'info'; id: Uint8Array; verification: string }
  | { kind: 'notFound' }
  | { kind: 'approved' }
  | { kind: 'failed'; reason: LinkFailure };

const FAILURES: LinkFailure[] = ['wrongCode', 'tooManyAttempts', 'gone', 'unavailable'];
const encoder = new TextEncoder();
const decoder = new TextDecoder('utf-8', { fatal: true });

export function encodeLookup(key: string): Uint8Array {
  if (!/^[A-Za-z0-9-]{4,40}$/u.test(key)) {
    throw new ProtocolError('invalid link key');
  }
  return Uint8Array.of(0, ...encoder.encode(key));
}

export function encodeApprove(id: Uint8Array, code: string): Uint8Array {
  if (id.length !== LINK_ID_LEN || !/^[0-9]{6}$/u.test(code)) {
    throw new ProtocolError('invalid link approval');
  }
  return Uint8Array.of(3, ...id, ...encoder.encode(code));
}

export function encodeApproveKey(key: Uint8Array): Uint8Array {
  if (key.length !== 32) {
    throw new ProtocolError('invalid link key');
  }
  return Uint8Array.of(6, ...key);
}

export function encodeApproveId(id: Uint8Array): Uint8Array {
  if (id.length !== LINK_ID_LEN) {
    throw new ProtocolError('invalid link id');
  }
  return Uint8Array.of(7, ...id);
}

export function decodeLink(payload: Uint8Array): LinkReply {
  const tag = payload[0];
  if (tag === undefined) {
    throw new ProtocolError('empty link message');
  }
  switch (tag) {
    case 1: {
      if (payload.length !== 1 + LINK_ID_LEN + 4) {
        throw new ProtocolError('bad link info length');
      }
      const verification = decoder.decode(payload.subarray(1 + LINK_ID_LEN));
      if (!/^[0-9]{4}$/u.test(verification)) {
        throw new ProtocolError('bad link verification');
      }
      return { kind: 'info', id: payload.slice(1, 1 + LINK_ID_LEN), verification };
    }
    case 2:
    case 4:
      if (payload.length !== 1) {
        throw new ProtocolError('bad link message length');
      }
      return { kind: tag === 2 ? 'notFound' : 'approved' };
    case 5: {
      if (payload.length !== 2) {
        throw new ProtocolError('bad link failure length');
      }
      const reason = FAILURES[payload[1] ?? 255];
      if (reason === undefined) {
        throw new ProtocolError('unknown link failure');
      }
      return { kind: 'failed', reason };
    }
    default:
      throw new ProtocolError(`unknown link message ${tag}`);
  }
}
