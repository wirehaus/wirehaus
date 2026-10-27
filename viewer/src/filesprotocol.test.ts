import assert from 'node:assert/strict';
import { test } from 'node:test';

import {
  MAX_FILE_CHUNK,
  decodeFiles,
  encodeAck,
  encodeCancel,
  encodeData,
  encodeDownload,
  encodeList,
  encodeListRoots,
  encodeMkdir,
  encodeRemove,
  encodeRename,
  encodeUpload,
} from './filesprotocol.js';
import { ProtocolError } from './protocol.js';

test('encodes the file requests with the agent layout', () => {
  assert.deepEqual([...encodeListRoots()], [0]);
  assert.deepEqual([...encodeList(2, 'ab')], [1, 2, 0, 2, 97, 98]);
  assert.deepEqual([...encodeDownload(0, '')], [2, 0, 0, 0]);
  assert.deepEqual([...encodeMkdir(1, 'x')], [4, 1, 0, 1, 120]);
  assert.deepEqual([...encodeRemove(1, 'x')], [5, 1, 0, 1, 120]);
  assert.deepEqual([...encodeRename(1, 'a', 'bc', true)], [14, 1, 0, 1, 97, 0, 2, 98, 99, 1]);
  assert.deepEqual([...encodeRename(0, '', 'ñ', false)], [14, 0, 0, 0, 0, 2, 195, 177, 0]);
  assert.deepEqual([...encodeCancel()], [6]);
  assert.deepEqual([...encodeUpload(0, '', 258)], [3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 2]);
  assert.deepEqual([...encodeAck(65536)], [13, 0, 0, 0, 0, 0, 1, 0, 0]);
  assert.deepEqual([...encodeData(Uint8Array.of(9))], [10, 9]);
});

test('encodes non-ASCII paths as UTF-8 with the byte length', () => {
  const bytes = encodeList(0, 'ñ');
  assert.deepEqual([...bytes], [1, 0, 0, 2, 0xc3, 0xb1]);
});

test('refuses requests the agent would reject', () => {
  assert.throws(() => encodeData(new Uint8Array(0)), ProtocolError);
  assert.throws(() => encodeData(new Uint8Array(MAX_FILE_CHUNK + 1)), ProtocolError);
  assert.doesNotThrow(() => encodeData(new Uint8Array(MAX_FILE_CHUNK)));
  assert.throws(() => encodeList(0, 'a'.repeat(4097)), ProtocolError);
  assert.throws(() => encodeUpload(0, 'a', -1), ProtocolError);
  assert.throws(() => encodeUpload(0, 'a', 1.5), ProtocolError);
  assert.throws(() => encodeList(256, 'a'), ProtocolError);
});

test('decodes the messages the agent sends', () => {
  assert.deepEqual(decodeFiles(Uint8Array.of(7, 2, 0, 1, 97, 0, 2, 98, 99)), {
    kind: 'roots',
    names: ['a', 'bc'],
    features: 0,
  });
  assert.deepEqual(decodeFiles(Uint8Array.of(7, 1, 0, 1, 97, 1)), {
    kind: 'roots',
    names: ['a'],
    features: 1,
  });
  const listing = Uint8Array.of(8, 1, 0, 1, 0, 1, 120, 1, 0, 0, 0, 0, 0, 0, 0, 5, 0, 0, 0, 0, 0, 0, 0, 9);
  assert.deepEqual(decodeFiles(listing), {
    kind: 'listing',
    truncated: true,
    entries: [{ name: 'x', kind: 'directory', size: 5, modified: 9 }],
  });
  assert.deepEqual(decodeFiles(Uint8Array.of(9, 0, 0, 0, 0, 0, 0, 1, 0)), { kind: 'ready', size: 256 });
  assert.deepEqual(decodeFiles(Uint8Array.of(10, 1, 2)), { kind: 'data', data: Uint8Array.of(1, 2) });
  assert.deepEqual(decodeFiles(Uint8Array.of(11)), { kind: 'done' });
  assert.deepEqual(decodeFiles(Uint8Array.of(12, 3)), { kind: 'failed', reason: 'notFound' });
  assert.deepEqual(decodeFiles(Uint8Array.of(13, 0, 0, 0, 0, 0, 0, 0, 7)), { kind: 'ack', bytes: 7 });
});

test('rejects malformed messages from the agent', () => {
  const bad: number[][] = [
    [],
    [14],
    [12, 11],
    [11, 0],
    [9, 0, 0],
    [10],
    [8, 2, 0, 0],
    [7, 17],
    [7, 1, 0, 1, 0xff],
    [8, 0, 0, 1, 0, 1, 120, 7, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [9, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff],
  ];
  for (const bytes of bad) {
    assert.throws(() => decodeFiles(Uint8Array.from(bytes)), ProtocolError, bytes.join(','));
  }
  assert.throws(() => decodeFiles(new Uint8Array(2 + MAX_FILE_CHUNK).fill(10)), ProtocolError);
});
