import assert from 'node:assert/strict';
import { test } from 'node:test';

import {
  MAX_CLIPBOARD_BYTES,
  clipboardFits,
  decodeClipboard,
  encodeClipboardRead,
  encodeClipboardWrite,
} from './clipboardprotocol.js';
import { ProtocolError } from './protocol.js';

test('encodes the clipboard requests with the agent layout', () => {
  assert.deepEqual([...encodeClipboardWrite('ab')], [0, 97, 98]);
  assert.deepEqual([...encodeClipboardWrite('ñ')], [0, 0xc3, 0xb1]);
  assert.deepEqual([...encodeClipboardRead()], [1]);
});

test('counts the limit in bytes, not characters', () => {
  assert.equal(clipboardFits(''), false);
  assert.equal(clipboardFits('a'.repeat(MAX_CLIPBOARD_BYTES)), true);
  assert.equal(clipboardFits('a'.repeat(MAX_CLIPBOARD_BYTES + 1)), false);
  assert.equal(clipboardFits('ñ'.repeat(MAX_CLIPBOARD_BYTES / 2)), true);
  assert.equal(clipboardFits('ñ'.repeat(MAX_CLIPBOARD_BYTES / 2 + 1)), false);
  assert.throws(() => encodeClipboardWrite(''), ProtocolError);
  assert.throws(() => encodeClipboardWrite('a'.repeat(MAX_CLIPBOARD_BYTES + 1)), ProtocolError);
});

test('decodes the messages the agent sends', () => {
  assert.deepEqual(decodeClipboard(Uint8Array.of(2, 0xc3, 0xb1)), { kind: 'content', text: 'ñ' });
  assert.deepEqual(decodeClipboard(Uint8Array.of(3)), { kind: 'empty' });
  assert.deepEqual(decodeClipboard(Uint8Array.of(4)), { kind: 'written' });
  assert.deepEqual(decodeClipboard(Uint8Array.of(5, 1)), { kind: 'failed', reason: 'tooLarge' });
  assert.deepEqual(decodeClipboard(Uint8Array.of(5, 3)), { kind: 'failed', reason: 'timeout' });
});

test('rejects malformed messages from the agent', () => {
  const bad: number[][] = [[], [2], [2, 0xff], [3, 0], [4, 1], [5], [5, 4], [5, 0, 0], [0, 97], [1], [9]];
  for (const bytes of bad) {
    assert.throws(() => decodeClipboard(Uint8Array.from(bytes)), ProtocolError, bytes.join(','));
  }
  assert.throws(() => decodeClipboard(new Uint8Array(2 + MAX_CLIPBOARD_BYTES).fill(2)), ProtocolError);
});
