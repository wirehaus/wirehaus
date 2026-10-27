import assert from 'node:assert/strict';
import test from 'node:test';

import { UpdateWatcher, readManifest } from './updates.js';

const manifest = (hash: string): string => `${hash.repeat(64).slice(0, 64)}  viewer.js\n`;

function sequence(...bodies: (string | null)[]): (input: string) => Promise<{ ok: boolean; text(): Promise<string> }> {
  let index = 0;
  return async () => {
    const body = bodies[Math.min(index, bodies.length - 1)] ?? null;
    index += 1;
    return body === null ? { ok: false, text: async () => '' } : { ok: true, text: async () => body };
  };
}

test('announces a new version once, only after a baseline exists', async () => {
  let announced = 0;
  const watcher = new UpdateWatcher(() => (announced += 1), sequence(manifest('a'), manifest('a'), manifest('b'), manifest('c')));
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
  const watcher = new UpdateWatcher(() => (announced += 1), sequence(manifest('a'), '<html></html>', null, manifest('a')));
  for (let round = 0; round < 4; round += 1) {
    await watcher.check();
  }
  assert.equal(announced, 0);
});
