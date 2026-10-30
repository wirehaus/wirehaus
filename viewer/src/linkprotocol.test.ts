import assert from 'node:assert/strict';
import test from 'node:test';

import { decodeLink, encodeApprove, encodeApproveId, encodeApproveKey, encodeLookup } from './linkprotocol.js';

const bytes = (...values: number[]): Uint8Array => Uint8Array.from(values);

test('a lookup carries the typed key as text', () => {
  const message = encodeLookup('ABCD-EFGH');
  assert.equal(message[0], 0);
  assert.equal(new TextDecoder().decode(message.subarray(1)), 'ABCD-EFGH');
});

test('lookups reject anything that is not a short key', () => {
  assert.throws(() => encodeLookup('ab'));
  assert.throws(() => encodeLookup('a b c d'));
  assert.throws(() => encodeLookup('x'.repeat(41)));
});

test('an approval carries the id and exactly six digits', () => {
  const id = new Uint8Array(16).fill(7);
  const message = encodeApprove(id, '123456');
  assert.equal(message[0], 3);
  assert.deepEqual([...message.subarray(1, 17)], [...id]);
  assert.equal(new TextDecoder().decode(message.subarray(17)), '123456');
  assert.throws(() => encodeApprove(id, '12345'));
  assert.throws(() => encodeApprove(id, '12345a'));
  assert.throws(() => encodeApprove(new Uint8Array(3), '123456'));
});

test('replies decode to their kinds', () => {
  const info = bytes(1, ...new Array<number>(16).fill(9), ...new TextEncoder().encode('0042'));
  const decoded = decodeLink(info);
  assert.equal(decoded.kind, 'info');
  if (decoded.kind === 'info') {
    assert.equal(decoded.verification, '0042');
    assert.equal(decoded.id.length, 16);
  }
  assert.deepEqual(decodeLink(bytes(2)), { kind: 'notFound' });
  assert.deepEqual(decodeLink(bytes(4)), { kind: 'approved' });
  assert.deepEqual(decodeLink(bytes(5, 0)), { kind: 'failed', reason: 'wrongCode' });
  assert.deepEqual(decodeLink(bytes(5, 3)), { kind: 'failed', reason: 'unavailable' });
});

test('malformed replies are rejected', () => {
  assert.throws(() => decodeLink(bytes()));
  assert.throws(() => decodeLink(bytes(99)));
  assert.throws(() => decodeLink(bytes(2, 0)));
  assert.throws(() => decodeLink(bytes(5)));
  assert.throws(() => decodeLink(bytes(5, 77)));
  assert.throws(() => decodeLink(bytes(1, 0, 0)));
  assert.throws(() => decodeLink(bytes(1, ...new Array<number>(16).fill(0), ...new TextEncoder().encode('12ab'))));
});

test('approving by key or by id carries only that value and is checked for size', () => {
  const key = new Uint8Array(32).fill(7);
  assert.deepEqual([...encodeApproveKey(key)], [6, ...key]);
  const id = new Uint8Array(16).fill(9);
  assert.deepEqual([...encodeApproveId(id)], [7, ...id]);
  assert.throws(() => encodeApproveKey(new Uint8Array(31)));
  assert.throws(() => encodeApproveId(new Uint8Array(17)));
});
