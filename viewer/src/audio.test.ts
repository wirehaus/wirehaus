import assert from 'node:assert/strict';
import { test } from 'node:test';

import { planPlayback } from './audio.js';

test('the first buffer starts a little ahead of now to absorb jitter', () => {
  const plan = planPlayback(10, 0, 0.02);
  assert.ok(plan !== null);
  assert.ok(plan.startAt > 10 && plan.startAt < 10.2);
  assert.ok(Math.abs(plan.next - (plan.startAt + 0.02)) < 1e-9);
});

test('consecutive buffers are scheduled back to back', () => {
  const first = planPlayback(10, 0, 0.02);
  assert.ok(first !== null);
  const second = planPlayback(10.005, first.next, 0.02);
  assert.ok(second !== null);
  assert.equal(second.startAt, first.next);
});

test('after an underrun it resynchronises instead of playing in the past', () => {
  const plan = planPlayback(20, 10, 0.02);
  assert.ok(plan !== null);
  assert.ok(plan.startAt > 20);
});

test('when too much is queued ahead the new buffer is dropped to keep latency low', () => {
  assert.equal(planPlayback(10, 10.5, 0.02), null);
  assert.notEqual(planPlayback(10, 10.2, 0.02), null);
});
