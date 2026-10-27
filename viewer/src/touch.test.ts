import assert from 'node:assert/strict';
import { test } from 'node:test';

import { TouchRecognizer } from './touch.js';

const at = (x: number, y: number) => ({ x, y });

test('a short tap is a hover and a left click', () => {
  const touch = new TouchRecognizer(() => false);
  assert.deepEqual(touch.down(1, at(10, 10), 0), []);
  assert.deepEqual(touch.up(1, at(11, 10), 90), [
    { kind: 'hover', at: at(11, 10) },
    { kind: 'tap', button: 'left', at: at(11, 10) },
  ]);
});

test('dragging one finger only moves the pointer and never clicks', () => {
  const touch = new TouchRecognizer(() => false);
  touch.down(1, at(0, 0), 0);
  assert.deepEqual(touch.move(1, at(4, 0)), []);
  assert.deepEqual(touch.move(1, at(30, 5)), [{ kind: 'hover', at: at(30, 5) }]);
  assert.deepEqual(touch.up(1, at(30, 5), 300), []);
});

test('a long press holds the button until the finger lifts', () => {
  const touch = new TouchRecognizer(() => false);
  touch.down(1, at(5, 5), 0);
  assert.deepEqual(touch.longPress(), [{ kind: 'press', at: at(5, 5) }]);
  assert.deepEqual(touch.move(1, at(50, 50)), [{ kind: 'hover', at: at(50, 50) }]);
  assert.deepEqual(touch.up(1, at(50, 50), 900), [{ kind: 'release', at: at(50, 50) }]);
  assert.deepEqual(touch.longPress(), []);
});

test('a long press after the finger moved does nothing', () => {
  const touch = new TouchRecognizer(() => false);
  touch.down(1, at(5, 5), 0);
  touch.move(1, at(60, 5));
  assert.deepEqual(touch.longPress(), []);
});

test('cancel releases a held button', () => {
  const touch = new TouchRecognizer(() => false);
  touch.down(1, at(5, 5), 0);
  touch.longPress();
  assert.deepEqual(touch.cancel(), [{ kind: 'release', at: at(5, 5) }]);
  assert.deepEqual(touch.cancel(), []);
});

test('a quick two finger tap is a right click at the middle', () => {
  const touch = new TouchRecognizer(() => false);
  touch.down(1, at(0, 0), 0);
  touch.down(2, at(100, 0), 10);
  assert.deepEqual(touch.up(1, at(0, 0), 120), [
    { kind: 'hover', at: at(50, 0) },
    { kind: 'tap', button: 'right', at: at(50, 0) },
  ]);
  assert.deepEqual(touch.up(2, at(100, 0), 130), []);
});

test('two fingers dragging up scroll the remote down', () => {
  const touch = new TouchRecognizer(() => false);
  touch.down(1, at(0, 200), 0);
  touch.down(2, at(100, 200), 5);
  const actions = [
    ...touch.move(1, at(0, 150)),
    ...touch.move(2, at(100, 150)),
  ];
  const scrolls = actions.filter((a) => a.kind === 'scroll');
  assert.ok(scrolls.length > 0);
  for (const scroll of scrolls) {
    assert.equal(scroll.kind === 'scroll' && scroll.vertical < 0, true);
  }
  assert.deepEqual(touch.up(1, at(0, 150), 400), []);
});

test('two fingers dragging while zoomed pan the view instead of scrolling', () => {
  const touch = new TouchRecognizer(() => true);
  touch.down(1, at(0, 200), 0);
  touch.down(2, at(100, 200), 5);
  const actions = [...touch.move(1, at(0, 150)), ...touch.move(2, at(100, 150))];
  assert.ok(actions.some((a) => a.kind === 'pan'));
  assert.ok(!actions.some((a) => a.kind === 'scroll'));
});

test('spreading two fingers zooms in around their centre', () => {
  const touch = new TouchRecognizer(() => false);
  touch.down(1, at(100, 100), 0);
  touch.down(2, at(200, 100), 5);
  const actions = [...touch.move(1, at(60, 100)), ...touch.move(2, at(240, 100))];
  const zooms = actions.filter((a) => a.kind === 'zoom');
  assert.ok(zooms.length > 0);
  assert.ok(zooms.every((a) => a.kind === 'zoom' && a.factor > 1));
});

test('a third finger is ignored and the gesture ends cleanly', () => {
  const touch = new TouchRecognizer(() => false);
  touch.down(1, at(0, 0), 0);
  touch.down(2, at(50, 0), 5);
  touch.down(3, at(90, 0), 6);
  assert.deepEqual(touch.move(3, at(95, 0)), []);
  touch.up(3, at(95, 0), 100);
  touch.up(2, at(50, 0), 101);
  touch.up(1, at(0, 0), 102);
  touch.down(4, at(7, 7), 500);
  assert.deepEqual(touch.up(4, at(7, 7), 560), [
    { kind: 'hover', at: at(7, 7) },
    { kind: 'tap', button: 'left', at: at(7, 7) },
  ]);
});
