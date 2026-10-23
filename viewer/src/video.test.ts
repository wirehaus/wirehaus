import assert from 'node:assert/strict';
import { test } from 'node:test';

import { VideoPlayer, codecFromKeyframe } from './video.js';

const SMALL = Uint8Array.of(
  0x00, 0x00, 0x00, 0x01, 0x67, 0x42, 0xc0, 0x14, 0x8c, 0x68, 0x20, 0x92, 0x6a, 0x02, 0x02, 0x02, 0x0f, 0x08,
  0x84, 0x6a, 0x00, 0x00, 0x00, 0x01, 0x68, 0xce, 0x3c, 0x80,
);
const HD = Uint8Array.of(
  0x00, 0x00, 0x00, 0x01, 0x67, 0x42, 0xc0, 0x28, 0x8c, 0x68, 0x07, 0x80, 0x22, 0x7e, 0x59, 0xa8,
);

test('reads the codec string from the parameter set of a real keyframe', () => {
  assert.equal(codecFromKeyframe(SMALL), 'avc1.42c014');
  assert.equal(codecFromKeyframe(HD), 'avc1.42c028');
});

test('finds the parameter set behind a short start code and after other units', () => {
  const short = Uint8Array.of(0x00, 0x00, 0x01, 0x67, 0x64, 0x00, 0x1f, 0xac);
  assert.equal(codecFromKeyframe(short), 'avc1.64001f');
  const later = Uint8Array.of(0x00, 0x00, 0x00, 0x01, 0x09, 0x10, 0x00, 0x00, 0x01, 0x67, 0x4d, 0x40, 0x2a, 0x00);
  assert.equal(codecFromKeyframe(later), 'avc1.4d402a');
});

test('returns nothing when there is no usable parameter set', () => {
  assert.equal(codecFromKeyframe(new Uint8Array(0)), undefined);
  assert.equal(codecFromKeyframe(Uint8Array.of(0, 0, 0, 1, 0x65, 0x88, 0x84, 0x00, 0x33)), undefined);
  assert.equal(codecFromKeyframe(Uint8Array.of(0, 0, 0, 1, 0x67, 0x42, 0xc0)), undefined);
  assert.equal(codecFromKeyframe(Uint8Array.of(1, 2, 3, 4, 5, 6, 7, 8, 9, 10)), undefined);
});

test('a player that failed ignores video until it is rearmed for the next connection', async () => {
  let failures = 0;
  const player = new VideoPlayer(
    () => undefined,
    () => {
      failures += 1;
    },
  );
  const noParameterSet = Uint8Array.of(0, 0, 0, 1, 0x65, 0x88, 0x84, 0x00, 0x33);
  await player.play(64, 64, true, noParameterSet);
  assert.equal(failures, 1);
  await player.play(64, 64, true, noParameterSet);
  assert.equal(failures, 1);
  player.rearm();
  await player.play(64, 64, true, noParameterSet);
  assert.equal(failures, 2);
});
