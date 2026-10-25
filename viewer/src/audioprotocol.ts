import { ProtocolError } from './protocol.js';

export const AUDIO_CHANNEL = 4;
export const MAX_AUDIO_PACKET = 1500;

export type AudioFailure = 'notAllowed' | 'unavailable';

export type AudioMessage =
  | { kind: 'started' }
  | { kind: 'stopped' }
  | { kind: 'packet'; data: Uint8Array }
  | { kind: 'failed'; reason: AudioFailure };

const FAILURES: AudioFailure[] = ['notAllowed', 'unavailable'];

export const encodeAudioStart = (): Uint8Array => Uint8Array.of(0);

export const encodeAudioStop = (): Uint8Array => Uint8Array.of(1);

export function decodeAudio(payload: Uint8Array): AudioMessage {
  const tag = payload[0];
  if (tag === undefined) {
    throw new ProtocolError('empty audio message');
  }
  switch (tag) {
    case 2:
    case 3:
      if (payload.length !== 1) {
        throw new ProtocolError('bad audio message length');
      }
      return { kind: tag === 2 ? 'started' : 'stopped' };
    case 4:
      if (payload.length < 2 || payload.length > 1 + MAX_AUDIO_PACKET) {
        throw new ProtocolError('bad audio packet length');
      }
      return { kind: 'packet', data: payload.subarray(1) };
    case 5: {
      if (payload.length !== 2) {
        throw new ProtocolError('bad audio failure length');
      }
      const reason = FAILURES[payload[1] ?? 255];
      if (reason === undefined) {
        throw new ProtocolError('unknown audio failure');
      }
      return { kind: 'failed', reason };
    }
    default:
      throw new ProtocolError(`unknown audio message ${tag}`);
  }
}
