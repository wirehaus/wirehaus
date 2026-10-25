import assert from 'node:assert/strict';
import { test } from 'node:test';

import {
  MAX_AUDIO_PACKET,
  decodeAudio,
  encodeAudioStart,
  encodeAudioStop,
} from './audioprotocol.js';
import { ProtocolError } from './protocol.js';

test('encodes the audio requests with the agent layout', () => {
  assert.deepEqual([...encodeAudioStart()], [0]);
  assert.deepEqual([...encodeAudioStop()], [1]);
});

test('decodes the messages the agent sends', () => {
  assert.deepEqual(decodeAudio(Uint8Array.of(2)), { kind: 'started' });
  assert.deepEqual(decodeAudio(Uint8Array.of(3)), { kind: 'stopped' });
  assert.deepEqual(decodeAudio(Uint8Array.of(5, 0)), { kind: 'failed', reason: 'notAllowed' });
  assert.deepEqual(decodeAudio(Uint8Array.of(5, 1)), { kind: 'failed', reason: 'unavailable' });
  const packet = decodeAudio(Uint8Array.of(4, 9, 8, 7));
  assert.equal(packet.kind, 'packet');
  assert.deepEqual(packet.kind === 'packet' ? [...packet.data] : [], [9, 8, 7]);
});

test('limits the packet size and never accepts an empty one', () => {
  assert.throws(() => decodeAudio(Uint8Array.of(4)), ProtocolError);
  const biggest = new Uint8Array(1 + MAX_AUDIO_PACKET);
  biggest[0] = 4;
  assert.equal(decodeAudio(biggest).kind, 'packet');
  assert.throws(() => decodeAudio(new Uint8Array([4, ...new Uint8Array(MAX_AUDIO_PACKET + 1)])), ProtocolError);
});

test('rejects malformed messages', () => {
  for (const bytes of [[], [6], [0], [1], [2, 0], [3, 0], [5], [5, 2], [5, 0, 0]]) {
    assert.throws(() => decodeAudio(Uint8Array.from(bytes)), ProtocolError, `${bytes}`);
  }
});
