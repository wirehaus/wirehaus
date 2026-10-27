import assert from 'node:assert/strict';
import { test } from 'node:test';

import { keysForCharacter } from './softkeys.js';

test('lowercase letters are a plain key press', () => {
  assert.deepEqual(keysForCharacter('a', 'us'), [
    { code: 30, pressed: true },
    { code: 30, pressed: false },
  ]);
});

test('uppercase letters hold shift around the key', () => {
  assert.deepEqual(keysForCharacter('A', 'es'), [
    { code: 42, pressed: true },
    { code: 30, pressed: true },
    { code: 30, pressed: false },
    { code: 42, pressed: false },
  ]);
});

test('the same punctuation lands on different keys per layout', () => {
  assert.deepEqual(keysForCharacter('@', 'us')?.map((k) => k.code), [42, 3, 3, 42]);
  assert.deepEqual(keysForCharacter('@', 'es')?.map((k) => k.code), [100, 3, 3, 100]);
  assert.deepEqual(keysForCharacter('-', 'es')?.map((k) => k.code), [53, 53]);
  assert.deepEqual(keysForCharacter('-', 'us')?.map((k) => k.code), [12, 12]);
});

test('the Spanish n with tilde is the semicolon key', () => {
  assert.deepEqual(keysForCharacter('ñ', 'es')?.map((k) => k.code), [39, 39]);
  assert.equal(keysForCharacter('ñ', 'us'), undefined);
});

test('characters with no key are rejected', () => {
  assert.equal(keysForCharacter('中', 'es'), undefined);
});

test('space, enter and tab map to their keys', () => {
  assert.deepEqual(keysForCharacter(' ', 'us')?.map((k) => k.code), [57, 57]);
  assert.deepEqual(keysForCharacter('\n', 'us')?.map((k) => k.code), [28, 28]);
  assert.deepEqual(keysForCharacter('\t', 'us')?.map((k) => k.code), [15, 15]);
});

test('every character on the US punctuation row maps to a real key', () => {
  for (const character of '`~!@#$%^&*()-_=+[{]}\\|;:\'",<.>/?') {
    assert.ok(keysForCharacter(character, 'us') !== undefined, character);
  }
});
