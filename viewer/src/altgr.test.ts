import assert from 'node:assert/strict';
import { test } from 'node:test';

import { ALTGR_WINDOW_MS, AltGrFilter, type KeyChange } from './altgr.js';

const CONTROL = 29;
const ALT_RIGHT = 100;
const DIGIT_2 = 3;

const down = (code: number, time: number): KeyChange => ({ code, pressed: true, time });
const up = (code: number, time: number): KeyChange => ({ code, pressed: false, time });

test('the control that Windows invents for AltGr is dropped together with its release', () => {
  const filter = new AltGrFilter();
  assert.deepEqual(filter.feed(down(CONTROL, 100)), []);
  assert.deepEqual(filter.feed(down(ALT_RIGHT, 100)), [down(ALT_RIGHT, 100)]);
  assert.deepEqual(filter.feed(down(DIGIT_2, 150)), [down(DIGIT_2, 150)]);
  assert.deepEqual(filter.feed(up(DIGIT_2, 200)), [up(DIGIT_2, 200)]);
  assert.deepEqual(filter.feed(up(CONTROL, 300)), []);
  assert.deepEqual(filter.feed(up(ALT_RIGHT, 300)), [up(ALT_RIGHT, 300)]);
  assert.equal(filter.pending(), false);
});

test('the release works in either order', () => {
  const filter = new AltGrFilter();
  filter.feed(down(CONTROL, 100));
  filter.feed(down(ALT_RIGHT, 100));
  assert.deepEqual(filter.feed(up(ALT_RIGHT, 300)), [up(ALT_RIGHT, 300)]);
  assert.deepEqual(filter.feed(up(CONTROL, 300)), []);
});

test('a real control press is held back briefly and then sent', () => {
  const filter = new AltGrFilter();
  assert.deepEqual(filter.feed(down(CONTROL, 100)), []);
  assert.equal(filter.pending(), true);
  assert.deepEqual(filter.flush(100 + ALTGR_WINDOW_MS - 1), []);
  assert.deepEqual(filter.flush(100 + ALTGR_WINDOW_MS), [down(CONTROL, 100)]);
  assert.equal(filter.pending(), false);
  assert.deepEqual(filter.feed(up(CONTROL, 400)), [up(CONTROL, 400)]);
});

test('control followed by another key sends control first, in order', () => {
  const filter = new AltGrFilter();
  filter.feed(down(CONTROL, 100));
  assert.deepEqual(filter.feed(down(30, 102)), [down(CONTROL, 100), down(30, 102)]);
  assert.deepEqual(filter.feed(up(30, 150)), [up(30, 150)]);
  assert.deepEqual(filter.feed(up(CONTROL, 160)), [up(CONTROL, 160)]);
});

test('a very short control tap is not lost', () => {
  const filter = new AltGrFilter();
  filter.feed(down(CONTROL, 100));
  assert.deepEqual(filter.feed(up(CONTROL, 101)), [down(CONTROL, 100), up(CONTROL, 101)]);
});

test('control and alt right pressed apart stay as two real keys', () => {
  const filter = new AltGrFilter();
  filter.feed(down(CONTROL, 100));
  assert.deepEqual(filter.feed(down(ALT_RIGHT, 100 + ALTGR_WINDOW_MS + 20)), [
    down(CONTROL, 100),
    down(ALT_RIGHT, 100 + ALTGR_WINDOW_MS + 20),
  ]);
  assert.deepEqual(filter.feed(up(CONTROL, 300)), [up(CONTROL, 300)]);
});

test('a swallowed release does not eat a real control press later', () => {
  const filter = new AltGrFilter();
  filter.feed(down(CONTROL, 100));
  filter.feed(down(ALT_RIGHT, 100));
  filter.feed(up(ALT_RIGHT, 200));
  assert.deepEqual(filter.feed(down(CONTROL, 5000)), []);
  assert.deepEqual(filter.flush(5000 + ALTGR_WINDOW_MS), [down(CONTROL, 5000)]);
  assert.deepEqual(filter.feed(up(CONTROL, 5200)), [up(CONTROL, 5200)]);
});

test('other keys pass straight through', () => {
  const filter = new AltGrFilter();
  assert.deepEqual(filter.feed(down(30, 1)), [down(30, 1)]);
  assert.deepEqual(filter.feed(up(30, 2)), [up(30, 2)]);
});
