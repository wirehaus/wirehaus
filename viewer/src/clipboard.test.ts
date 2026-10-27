import assert from 'node:assert/strict';
import test from 'node:test';

import { addToHistory, previewOf } from './clipboard.js';

test('history keeps the newest first, drops duplicates and blank text', () => {
  let entries = addToHistory([], 'uno', 1);
  entries = addToHistory(entries, 'dos', 2);
  entries = addToHistory(entries, 'dos', 3);
  entries = addToHistory(entries, '   ', 4);
  entries = addToHistory(entries, 'uno', 5);
  assert.deepEqual(entries.map((entry) => entry.text), ['uno', 'dos']);
  assert.equal(entries[0]?.at, 5);
});

test('history is bounded by count and by total size', () => {
  let entries = addToHistory([], 'x', 0);
  for (let index = 0; index < 100; index += 1) {
    entries = addToHistory(entries, `texto ${index}`, index);
  }
  assert.equal(entries.length, 30);
  assert.equal(entries[0]?.text, 'texto 99');
  const big = addToHistory([], 'a'.repeat(200 * 1024), 1);
  const both = addToHistory(big, 'b'.repeat(200 * 1024), 2);
  assert.deepEqual(both.map((entry) => entry.text[0]), ['b']);
});

test('previews show the first non-empty line, flattened and shortened', () => {
  assert.equal(previewOf('\n\n  hola   mundo  \nsegunda'), 'hola mundo');
  assert.equal(previewOf('x'.repeat(200)).length, 91);
  assert.equal(previewOf(''), '');
});
