import assert from 'node:assert/strict';
import test from 'node:test';

import type { FileEntry } from './filesprotocol.js';
import {
  categorize,
  isEditable,
  isPreviewable,
  joinPath,
  knownPlaces,
  looksBinary,
  sortEntries,
  uniqueName,
  validName,
} from './filetypes.js';

const file = (name: string, size = 1, modified = 0): FileEntry => ({ name, kind: 'file', size, modified });
const dir = (name: string): FileEntry => ({ name, kind: 'directory', size: 0, modified: 0 });

test('classifies entries by kind and extension', () => {
  assert.equal(categorize(dir('x')), 'folder');
  assert.equal(categorize(file('a.TXT')), 'text');
  assert.equal(categorize(file('main.rs')), 'code');
  assert.equal(categorize(file('Makefile')), 'text');
  assert.equal(categorize(file('.bashrc')), 'text');
  assert.equal(categorize(file('p.png')), 'image');
  assert.equal(categorize(file('x.zip')), 'archive');
  assert.equal(categorize(file('x.pdf')), 'document');
  assert.equal(categorize(file('blob.bin')), 'other');
  assert.equal(categorize({ name: 's', kind: 'other' }), 'other');
});

test('only small text files are editable and small images previewable', () => {
  assert.equal(isEditable(file('a.txt', 100)), true);
  assert.equal(isEditable(file('a.txt', 3 * 1024 * 1024)), false);
  assert.equal(isEditable(file('a.png', 10)), false);
  assert.equal(isEditable(dir('a.txt')), false);
  assert.equal(isPreviewable(file('a.png', 10)), true);
  assert.equal(isPreviewable(file('a.txt', 10)), false);
});

test('sorts folders first and honours the key and direction', () => {
  const items = [file('b.txt', 5, 2), dir('Zeta'), file('a10.txt', 9, 1), file('a2.txt', 1, 3), dir('alfa')];
  assert.deepEqual(sortEntries(items, 'name', false).map((e) => e.name), ['alfa', 'Zeta', 'a2.txt', 'a10.txt', 'b.txt']);
  assert.deepEqual(sortEntries(items, 'name', true).map((e) => e.name), ['Zeta', 'alfa', 'b.txt', 'a10.txt', 'a2.txt']);
  assert.deepEqual(sortEntries(items, 'size', true).map((e) => e.name).slice(2), ['a10.txt', 'b.txt', 'a2.txt']);
  assert.deepEqual(sortEntries(items, 'modified', false).map((e) => e.name).slice(2), ['a10.txt', 'b.txt', 'a2.txt']);
});

test('validates names and builds unique copies', () => {
  for (const bad of ['', '.', '..', 'a/b', 'a\\b', 'a:b', 'a\nb', 'x'.repeat(256)]) {
    assert.equal(validName(bad), false, bad);
  }
  assert.equal(validName('informe final.txt'), true);
  assert.equal(uniqueName(['a.txt'], 'b.txt'), 'b.txt');
  assert.equal(uniqueName(['a.txt', 'a 2.txt'], 'a.txt'), 'a 3.txt');
  assert.equal(uniqueName(['nueva'], 'nueva'), 'nueva 2');
  assert.equal(joinPath(['a', 'b'], 'c'), 'a/b/c');
  assert.equal(joinPath([]), '');
});

test('finds the usual home folders in either language', () => {
  const found = knownPlaces([dir('Descargas'), dir('Documents'), dir('src'), file('Desktop')]);
  assert.deepEqual(found, ['Documents', 'Descargas']);
});

test('detects binary content', () => {
  assert.equal(looksBinary(Uint8Array.of(104, 105, 0, 1)), true);
  assert.equal(looksBinary(new TextEncoder().encode('hola ñ')), false);
});
