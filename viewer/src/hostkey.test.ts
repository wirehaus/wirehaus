import assert from 'node:assert/strict';
import test from 'node:test';

import { HostKey } from './hostkey.js';

test('a lone tap of the right control key leaves the capture and is not forwarded', () => {
  const host = new HostKey();
  assert.equal(host.feed('ControlRight', true), 'swallow');
  assert.equal(host.feed('ControlRight', true), 'swallow');
  assert.equal(host.feed('ControlRight', false), 'exit');
});

test('right control used as a modifier is forwarded together with the other key', () => {
  const host = new HostKey();
  assert.equal(host.feed('ControlRight', true), 'swallow');
  assert.equal(host.feed('KeyC', true), 'modifier-then-pass');
  assert.equal(host.feed('KeyC', false), 'pass');
  assert.equal(host.feed('ControlRight', true), 'pass');
  assert.equal(host.feed('ControlRight', false), 'pass');
  assert.equal(host.feed('KeyV', true), 'pass');
});

test('other keys pass and a reset forgets a held host key', () => {
  const host = new HostKey();
  assert.equal(host.feed('Escape', true), 'pass');
  assert.equal(host.feed('AltLeft', true), 'pass');
  assert.equal(host.feed('F4', true), 'pass');
  host.feed('ControlRight', true);
  host.reset();
  assert.equal(host.feed('KeyA', true), 'pass');
  assert.equal(host.feed('ControlRight', false), 'pass');
});
