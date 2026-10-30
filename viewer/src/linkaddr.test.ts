import assert from 'node:assert/strict';
import test from 'node:test';

import { agentBase, fromHex, linkAddress, parseLinkText, toHex } from './linkaddr.js';

const ORIGIN = 'https://app.wirehaus.dev';
const ID = '0123456789abcdef0123456789abcdef';

test('hex round-trips and rejects garbage', () => {
  assert.equal(toHex(Uint8Array.of(0, 255, 16)), '00ff10');
  assert.deepEqual([...(fromHex('00ff10') ?? [])], [0, 255, 16]);
  assert.equal(fromHex('abc'), undefined);
  assert.equal(fromHex('zz'), undefined);
});

test('the agent address is derived from the socket address', () => {
  assert.equal(agentBase('wss://zz-test.wirehaus.dev/ws'), 'https://zz-test.wirehaus.dev');
  assert.equal(agentBase('ws://127.0.0.1:7878/ws'), 'http://127.0.0.1:7878');
  assert.equal(agentBase('https://x.example/ws'), undefined);
  assert.equal(agentBase('nonsense'), undefined);
});

test('the address shown as a QR carries the viewer, the name and the id', () => {
  assert.equal(linkAddress(ORIGIN, 'zz-test', ID), `${ORIGIN}/zz-test#link=${ID}`);
});

test('a scanned address is accepted only for the same viewer and the same computer', () => {
  assert.deepEqual(parseLinkText(`${ORIGIN}/zz-test#link=${ID}`, ORIGIN, 'zz-test'), { id: ID });
  assert.deepEqual(parseLinkText(`${ORIGIN}/ZZ-test/#link=${ID}`, ORIGIN, 'zz-test'), { id: ID });
  assert.deepEqual(parseLinkText(`${ORIGIN}/otro#link=${ID}`, ORIGIN, 'zz-test'), { error: 'other' });
  assert.deepEqual(parseLinkText(`https://evil.example/zz-test#link=${ID}`, ORIGIN, 'zz-test'), { error: 'invalid' });
  assert.deepEqual(parseLinkText(`${ORIGIN}/zz-test#link=short`, ORIGIN, 'zz-test'), { error: 'invalid' });
  assert.deepEqual(parseLinkText('hola que tal', ORIGIN, 'zz-test'), { error: 'invalid' });
});

test('a typed code or a bare id is understood', () => {
  assert.deepEqual(parseLinkText('abcd-efg2', ORIGIN, 'x'), { manual: 'ABCDEFG2' });
  assert.deepEqual(parseLinkText(' ABCDEFG2 ', ORIGIN, 'x'), { manual: 'ABCDEFG2' });
  assert.deepEqual(parseLinkText(ID, ORIGIN, 'x'), { id: ID });
  assert.deepEqual(parseLinkText('ABCD-EFG1', ORIGIN, 'x'), { error: 'invalid' });
});

test('a public key in the address or on its own is understood as a key, not as an id', () => {
  const key = 'ab'.repeat(32);
  assert.deepEqual(parseLinkText(`${ORIGIN}/zz-test#link=${key}`, ORIGIN, 'zz-test'), { key });
  assert.deepEqual(parseLinkText(key, ORIGIN, 'x'), { key });
  assert.deepEqual(parseLinkText(`${ORIGIN}/otro#link=${key}`, ORIGIN, 'zz-test'), { error: 'other' });
});
