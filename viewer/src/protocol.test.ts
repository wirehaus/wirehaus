import assert from 'node:assert/strict';
import { test } from 'node:test';

import {
  DESKTOP_CHANNEL,
  ProtocolError,
  decodeControl,
  decodeDesktop,
  decodeFrame,
  decodeTerminal,
  encodeCapabilities,
  encodeFrame,
  encodeFrameAck,
  encodeInput,
  encodeResume,
  encodeSelectMonitor,
  encodeTerminalClose,
  encodeTerminalInput,
  encodeTerminalOpen,
  encodeTerminalResize,
  encodeTotp,
  inputChunks,
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
  assert.deepEqual(decodeControl(Uint8Array.of(3, 4, 0, 0, 0, 0)), {
    kind: 'authFailed',
    reason: 'unavailable',
    retryAfter: 0,
  });
});

test('rejects malformed control messages', () => {
  assert.throws(() => decodeControl(new Uint8Array(0)), ProtocolError);
  assert.throws(() => decodeControl(Uint8Array.of(9)), ProtocolError);
  assert.throws(() => decodeControl(Uint8Array.of(0, 1)), ProtocolError);
  assert.throws(() => decodeControl(Uint8Array.of(2, 0)), ProtocolError);
  assert.throws(() => decodeControl(Uint8Array.of(3, 1, 0, 0, 0)), ProtocolError);
  assert.throws(() => decodeControl(Uint8Array.of(3, 9, 0, 0, 0, 0)), ProtocolError);
  assert.throws(() => decodeControl(Uint8Array.of(3, 5, 0, 0, 0, 0)), ProtocolError);
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

test('encodes the terminal messages with the agent layout', () => {
  assert.deepEqual([...encodeTerminalOpen(0x0102, 0x0034)], [0, 1, 2, 0, 0x34]);
  assert.deepEqual([...encodeTerminalResize(80, 24)], [2, 0, 80, 0, 24]);
  assert.deepEqual([...encodeTerminalClose()], [3]);
  assert.deepEqual([...encodeTerminalInput(Uint8Array.of(65, 66))], [1, 65, 66]);
});

test('refuses terminal sizes and chunks the agent would reject', () => {
  for (const [columns, rows] of [[0, 24], [80, 0], [501, 24], [80, 501]]) {
    assert.throws(() => encodeTerminalOpen(columns as number, rows as number), ProtocolError);
  }
  assert.throws(() => encodeTerminalInput(new Uint8Array(0)), ProtocolError);
  assert.throws(() => encodeTerminalInput(new Uint8Array(3073)), ProtocolError);
  assert.doesNotThrow(() => encodeTerminalInput(new Uint8Array(3072)));
});

test('splits long input into chunks the agent accepts', () => {
  const chunks = [...inputChunks(new Uint8Array(7000))];
  assert.deepEqual(chunks.map((chunk) => chunk.length), [3072, 3072, 856]);
  assert.deepEqual([...inputChunks(new Uint8Array(0))], []);
});

test('decodes the terminal messages the agent sends', () => {
  const output = decodeTerminal(Uint8Array.of(4, 104, 105));
  assert.equal(output.kind, 'output');
  if (output.kind === 'output') assert.deepEqual([...output.data], [104, 105]);
  assert.deepEqual(decodeTerminal(Uint8Array.of(5, 0xff, 0xff, 0xff, 0xff)), { kind: 'exited', code: -1 });
  assert.deepEqual(decodeTerminal(Uint8Array.of(5, 0, 0, 0, 7)), { kind: 'exited', code: 7 });
  assert.deepEqual(decodeTerminal(Uint8Array.of(6, 1)), { kind: 'refused', reason: 'tooManySessions' });
});

test('rejects malformed terminal messages', () => {
  assert.throws(() => decodeTerminal(new Uint8Array(0)), ProtocolError);
  assert.throws(() => decodeTerminal(Uint8Array.of(4)), ProtocolError);
  assert.throws(() => decodeTerminal(Uint8Array.of(5, 0, 0, 1)), ProtocolError);
  assert.throws(() => decodeTerminal(Uint8Array.of(6, 9)), ProtocolError);
  assert.throws(() => decodeTerminal(Uint8Array.of(9)), ProtocolError);
});

test('encodes a resume request with the agent layout', () => {
  const ticket = Uint8Array.from({ length: 32 }, (_, index) => index);
  const bytes = encodeResume(ticket);
  assert.equal(bytes.length, 33);
  assert.equal(bytes[0], 4);
  assert.deepEqual([...bytes.subarray(1)], [...ticket]);
  assert.throws(() => encodeResume(new Uint8Array(31)), ProtocolError);
  assert.throws(() => encodeResume(new Uint8Array(33)), ProtocolError);
});

test('decodes the ticket and the end of the session', () => {
  const body = Uint8Array.from({ length: 32 }, (_, index) => 255 - index);
  const decoded = decodeControl(Uint8Array.from([5, ...body]));
  assert.equal(decoded.kind, 'ticket');
  assert.deepEqual(decoded.kind === 'ticket' ? [...decoded.ticket] : [], [...body]);
  assert.deepEqual(decodeControl(Uint8Array.of(6, 0)), { kind: 'sessionEnded', reason: 'idle' });
  assert.deepEqual(decodeControl(Uint8Array.of(6, 1)), { kind: 'sessionEnded', reason: 'revoked' });
  assert.deepEqual(decodeControl(Uint8Array.of(6, 2)), { kind: 'sessionEnded', reason: 'locked' });
});

test('rejects malformed ticket and end messages', () => {
  for (const bytes of [[5], [5, 1, 2], new Array(34).fill(5), [6], [6, 3], [6, 0, 0]]) {
    assert.throws(() => decodeControl(Uint8Array.from(bytes)), ProtocolError, bytes.join(','));
  }
});

test('a monitor list is decoded with signed positions and flags, and a bad length is refused', () => {
  const body = new Uint8Array(3 + 13);
  const view = new DataView(body.buffer);
  body[0] = 4;
  body[1] = 0;
  body[2] = 1;
  view.setInt32(3, -1920);
  view.setInt32(7, 10);
  view.setUint16(11, 1920);
  view.setUint16(13, 1080);
  body[15] = 3;
  assert.deepEqual(decodeDesktop(body), {
    kind: 'monitors',
    active: 0,
    items: [{ x: -1920, y: 10, width: 1920, height: 1080, primary: true, all: true }],
  });
  assert.throws(() => decodeDesktop(body.subarray(0, 10)));
  assert.throws(() => decodeDesktop(Uint8Array.of(4, 0, 17)));
  assert.deepEqual([...encodeSelectMonitor(3)], [6, 3]);
});
