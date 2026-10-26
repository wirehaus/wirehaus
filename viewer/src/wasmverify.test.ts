import assert from 'node:assert/strict';
import { test } from 'node:test';

import { verifiedModule } from './wasmverify.js';

const sha256Abc = 'ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad';
const serving = (body: string, ok = true) => (async () => new Response(body, { status: ok ? 200 : 404 })) as typeof fetch;

test('returns the bytes when the hash matches', async () => {
  const bytes = await verifiedModule(sha256Abc, serving('abc'));
  assert.equal(new TextDecoder().decode(bytes), 'abc');
});

test('refuses bytes that do not match the pinned hash', async () => {
  await assert.rejects(verifiedModule(sha256Abc, serving('abd')), /pinned hash/);
});

test('refuses a failed download', async () => {
  await assert.rejects(verifiedModule(sha256Abc, serving('', false)), /unavailable/);
});
