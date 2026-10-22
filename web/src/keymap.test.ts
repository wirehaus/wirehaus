import assert from 'node:assert/strict';
import { test } from 'node:test';

import { evdevCode, mappedCodes } from './keymap.js';

test('maps common keys to their evdev codes', () => {
  assert.equal(evdevCode('Escape'), 1);
  assert.equal(evdevCode('Digit1'), 2);
  assert.equal(evdevCode('Digit0'), 11);
  assert.equal(evdevCode('KeyQ'), 16);
  assert.equal(evdevCode('KeyA'), 30);
  assert.equal(evdevCode('KeyM'), 50);
  assert.equal(evdevCode('Enter'), 28);
  assert.equal(evdevCode('Space'), 57);
  assert.equal(evdevCode('F1'), 59);
  assert.equal(evdevCode('F10'), 68);
  assert.equal(evdevCode('F11'), 87);
  assert.equal(evdevCode('F13'), 183);
  assert.equal(evdevCode('Numpad7'), 71);
  assert.equal(evdevCode('Numpad0'), 82);
  assert.equal(evdevCode('ArrowUp'), 103);
  assert.equal(evdevCode('MetaLeft'), 125);
});

test('does not map unknown keys', () => {
  assert.equal(evdevCode('Unidentified'), undefined);
  assert.equal(evdevCode(''), undefined);
});

test('never maps two keys to the same code and stays inside the X11 range', () => {
  const codes = mappedCodes();
  assert.equal(new Set(codes).size, codes.length);
  assert.ok(codes.every((code) => code >= 1 && code <= 247));
});
