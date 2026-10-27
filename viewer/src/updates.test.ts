import assert from 'node:assert/strict';
import test from 'node:test';

import { createHash } from 'node:crypto';

import { UpdateWatcher, readManifest, servedConsistently } from './updates.js';

const yes = async (): Promise<boolean> => true;

const manifest = (hash: string): string => `${hash.repeat(64).slice(0, 64)}  viewer.js\n`;

type Reply = { ok: boolean; text(): Promise<string>; arrayBuffer(): Promise<ArrayBuffer> };

const empty = async (): Promise<ArrayBuffer> => new ArrayBuffer(0);

function sequence(...bodies: (string | null)[]): (input: string) => Promise<Reply> {
  let index = 0;
  return async () => {
    const body = bodies[Math.min(index, bodies.length - 1)] ?? null;
    index += 1;
    return body === null
      ? { ok: false, text: async () => '', arrayBuffer: empty }
      : { ok: true, text: async () => body, arrayBuffer: empty };
  };
}

test('announces a new version once, only after a baseline exists', async () => {
  let announced = 0;
  const watcher = new UpdateWatcher(() => (announced += 1), sequence(manifest('a'), manifest('a'), manifest('b'), manifest('c')), yes);
  assert.equal(await watcher.check(), false);
  assert.equal(await watcher.check(), false);
  assert.equal(await watcher.check(), true);
  assert.equal(await watcher.check(), true);
  assert.equal(announced, 1);
});

test('failures and pages that are not a manifest are ignored', async () => {
  assert.equal(await readManifest(sequence(null)), null);
  assert.equal(await readManifest(sequence('<html>not found</html>')), null);
  assert.equal(await readManifest(async () => Promise.reject(new Error('offline'))), null);
  let announced = 0;
  const watcher = new UpdateWatcher(() => (announced += 1), sequence(manifest('a'), '<html></html>', null, manifest('a')), yes);
  for (let round = 0; round < 4; round += 1) {
    await watcher.check();
  }
  assert.equal(announced, 0);
});

const sha = (text: string): string => createHash('sha256').update(text).digest('hex');

function files(content: Record<string, string>): (input: string) => Promise<Reply> {
  return async (input) => {
    const name = input === '/' ? 'index.html' : input.slice(1);
    const body = content[name];
    return body === undefined
      ? { ok: false, text: async () => '', arrayBuffer: empty }
      : { ok: true, text: async () => body, arrayBuffer: async () => new TextEncoder().encode(body).buffer as ArrayBuffer };
  };
}

test('a version is consistent only when every file served matches the manifest', async () => {
  const served = { 'index.html': '<html>b</html>', 'main.js': 'export const b = 1;' };
  const text = `${sha(served['index.html'])}  index.html\n${sha(served['main.js'])}  main.js\n`;
  assert.equal(await servedConsistently(text, files(served)), true);
  assert.equal(await servedConsistently(text, files({ ...served, 'main.js': 'export const a = 1;' })), false);
  assert.equal(await servedConsistently(text, files({ 'index.html': served['index.html'] })), false);
  assert.equal(await servedConsistently('', files(served)), false);
  assert.equal(await servedConsistently(text, async () => Promise.reject(new Error('offline'))), false);
});

test('a new manifest is not announced while the files served are still mixed', async () => {
  let announced = 0;
  let consistent = false;
  const watcher = new UpdateWatcher(
    () => (announced += 1),
    sequence(manifest('a'), manifest('b'), manifest('b')),
    async () => consistent,
  );
  await watcher.check();
  assert.equal(await watcher.check(), false);
  assert.equal(announced, 0);
  consistent = true;
  assert.equal(await watcher.check(), true);
  assert.equal(announced, 1);
});
