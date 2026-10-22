import assert from 'node:assert/strict';
import { test } from 'node:test';

import {
  DESKTOP_CHANNEL,
  ProtocolError,
  decodeControl,
  decodeDesktop,
  decodeFrame,
  encodeCapabilities,
  encodeFrame,
  encodeFrameAck,
  encodeInput,
  encodeTotp,
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

test('decodes the frame end marker the agent sends', () => {
  assert.deepEqual(decodeDesktop(Uint8Array.of(2, 1, 2, 3, 4)), { kind: 'frameEnd', sequence: 0x01020304 });
  assert.deepEqual(decodeDesktop(Uint8Array.of(2, 0xff, 0xff, 0xff, 0xff)), {
    kind: 'frameEnd',
    sequence: 0xffffffff,
  });
  assert.throws(() => decodeDesktop(Uint8Array.of(2, 1, 2, 3)), ProtocolError);
  assert.throws(() => decodeDesktop(Uint8Array.of(2, 1, 2, 3, 4, 5)), ProtocolError);
});

test('encodes the acknowledgement layout the agent expects', () => {
  assert.deepEqual([...encodeFrameAck(0x01020304)], [4, 1, 2, 3, 4]);
  assert.deepEqual([...encodeFrameAck(0xfffffffe)], [4, 0xff, 0xff, 0xff, 0xfe]);
});

test('decodes the control messages the agent sends', () => {
  assert.deepEqual(decodeControl(Uint8Array.of(0)), { kind: 'authRequired' });
  assert.deepEqual(decodeControl(Uint8Array.of(2)), { kind: 'authOk' });
  assert.deepEqual(decodeControl(Uint8Array.of(3, 1, 1, 2, 3, 4)), {
    kind: 'authFailed',
    reason: 'locked',
    retryAfter: 0x01020304,
  });
  assert.deepEqual(decodeControl(Uint8Array.of(3, 3, 0xff, 0xff, 0xff, 0xff)), {
    kind: 'authFailed',
    reason: 'busy',
    retryAfter: 0xffffffff,
  });
});

test('rejects malformed control messages', () => {
  assert.throws(() => decodeControl(new Uint8Array(0)), ProtocolError);
  assert.throws(() => decodeControl(Uint8Array.of(9)), ProtocolError);
  assert.throws(() => decodeControl(Uint8Array.of(0, 1)), ProtocolError);
  assert.throws(() => decodeControl(Uint8Array.of(2, 0)), ProtocolError);
  assert.throws(() => decodeControl(Uint8Array.of(3, 1, 0, 0, 0)), ProtocolError);
  assert.throws(() => decodeControl(Uint8Array.of(3, 9, 0, 0, 0, 0)), ProtocolError);
});

test('encodes the second factor code with the agent layout', () => {
  assert.deepEqual([...encodeTotp('123456')], [1, 49, 50, 51, 52, 53, 54]);
  assert.deepEqual([...encodeTotp('000000')], [1, 48, 48, 48, 48, 48, 48]);
});

test('refuses codes that are not six ASCII digits', () => {
  for (const code of ['', '12345', '1234567', '12345a', ' 12345', '１２３４５６']) {
    assert.throws(() => encodeTotp(code), ProtocolError, code);
  }
});

test('decodes the video messages the agent sends', () => {
  const message = decodeDesktop(Uint8Array.of(3, 1, 2, 3, 4, 1, 9, 8));
  assert.equal(message.kind, 'video');
  if (message.kind === 'video') {
    assert.deepEqual([message.width, message.height, message.keyframe], [0x0102, 0x0304, true]);
    assert.deepEqual([...message.data], [9, 8]);
  }
  const delta = decodeDesktop(Uint8Array.of(3, 0, 8, 0, 8, 0));
  assert.equal(delta.kind === 'video' && delta.keyframe, false);
});

test('rejects malformed video messages', () => {
  assert.throws(() => decodeDesktop(Uint8Array.of(3, 0, 8, 0, 8)), ProtocolError);
  assert.throws(() => decodeDesktop(Uint8Array.of(3, 0, 8, 0, 8, 2, 1)), ProtocolError);
});

test('encodes the capabilities layout the agent expects', () => {
  assert.deepEqual([...encodeCapabilities(true)], [5, 1]);
  assert.deepEqual([...encodeCapabilities(false)], [5, 0]);
});
