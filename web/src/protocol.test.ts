import assert from 'node:assert/strict';
import { test } from 'node:test';

import {
  DESKTOP_CHANNEL,
  ProtocolError,
  decodeDesktop,
  decodeFrame,
  encodeFrame,
  encodeInput,
} from './protocol.js';

test('encodes the frame layout the agent expects', () => {
  const bytes = encodeFrame(2, 0x01020304, Uint8Array.of(0xaa));
  assert.deepEqual([...bytes], [2, 1, 2, 3, 4, 0, 0, 0, 1, 0xaa]);
});

test('round trips a frame', () => {
  const frame = decodeFrame(encodeFrame(DESKTOP_CHANNEL, 7, Uint8Array.of(1, 2, 3)));
  assert.equal(frame.channel, DESKTOP_CHANNEL);
  assert.equal(frame.stream, 7);
  assert.deepEqual([...frame.payload], [1, 2, 3]);
});

test('rejects short, truncated and oversized frames', () => {
  assert.throws(() => decodeFrame(new Uint8Array(8)), ProtocolError);
  const truncated = encodeFrame(0, 0, new Uint8Array(32));
  assert.throws(() => decodeFrame(truncated.subarray(0, truncated.length - 5)), ProtocolError);
  const trailing = new Uint8Array([...encodeFrame(0, 0, Uint8Array.of(9)), 0]);
  assert.throws(() => decodeFrame(trailing), ProtocolError);
  const huge = new Uint8Array(9);
  new DataView(huge.buffer).setUint32(5, 0xffffffff);
  assert.throws(() => decodeFrame(huge), ProtocolError);
  assert.throws(() => encodeFrame(0, 0, new Uint8Array((1 << 20) + 1)), ProtocolError);
});

test('encodes input events with the agent layout', () => {
  assert.deepEqual([...encodeInput({ kind: 'key', code: 0x0102, pressed: true })], [3, 1, 2, 1]);
  assert.deepEqual([...encodeInput({ kind: 'pointer', x: 0x0102, y: 0x0304 })], [0, 1, 2, 3, 4]);
  assert.deepEqual([...encodeInput({ kind: 'button', button: 'right', pressed: true })], [1, 2, 1]);
  assert.deepEqual([...encodeInput({ kind: 'button', button: 'left', pressed: false })], [1, 0, 0]);
  assert.deepEqual([...encodeInput({ kind: 'wheel', horizontal: -3, vertical: 120 })], [2, 0xff, 0xfd, 0, 120]);
});

test('decodes the resize and tile messages the agent sends', () => {
  assert.deepEqual(decodeDesktop(Uint8Array.of(0, 1, 2, 3, 4)), {
    kind: 'resize',
    width: 0x0102,
    height: 0x0304,
  });
  const tile = decodeDesktop(Uint8Array.of(1, 0, 64, 0, 128, 1, 0, 0, 64, 0, 0xff, 0xd8, 0xff));
  assert.equal(tile.kind, 'tile');
  if (tile.kind === 'tile') {
    assert.deepEqual([tile.x, tile.y, tile.width, tile.height, tile.codec], [64, 128, 256, 64, 0]);
    assert.deepEqual([...tile.data], [0xff, 0xd8, 0xff]);
  }
});

test('rejects malformed desktop messages', () => {
  assert.throws(() => decodeDesktop(new Uint8Array(0)), ProtocolError);
  assert.throws(() => decodeDesktop(Uint8Array.of(9)), ProtocolError);
  assert.throws(() => decodeDesktop(Uint8Array.of(0, 1, 2, 3)), ProtocolError);
  assert.throws(() => decodeDesktop(Uint8Array.of(0, 1, 2, 3, 4, 5)), ProtocolError);
  assert.throws(() => decodeDesktop(Uint8Array.of(1, 0, 0, 0, 0, 0, 1, 0, 1)), ProtocolError);
});
