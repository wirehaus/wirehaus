import assert from 'node:assert/strict';
import test from 'node:test';

import { deviceLabel } from './devicename.js';

test('common phones and computers get a readable name', () => {
  assert.equal(
    deviceLabel('Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Mobile/15E148 Safari/604.1'),
    'iPhone - Safari',
  );
  assert.equal(
    deviceLabel('Mozilla/5.0 (Linux; Android 15; Pixel 9) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Mobile Safari/537.36'),
    'Android - Chrome',
  );
  assert.equal(
    deviceLabel('Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36 Edg/130.0.0.0'),
    'Windows - Edge',
  );
  assert.equal(
    deviceLabel('Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.5 Safari/605.1.15'),
    'Mac - Safari',
  );
  assert.equal(deviceLabel('Mozilla/5.0 (X11; Linux x86_64; rv:130.0) Gecko/20100101 Firefox/130.0'), 'Linux - Firefox');
});

test('an iPad that presents itself as a Mac is recognised by its touch points', () => {
  const agent = 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.5 Safari/605.1.15';
  assert.equal(deviceLabel(agent, 5), 'iPad - Safari');
  assert.equal(deviceLabel(agent, 0), 'Mac - Safari');
});

test('an unknown agent gives an empty name', () => {
  assert.equal(deviceLabel(''), '');
  assert.equal(deviceLabel('curl/8.0'), '');
});
