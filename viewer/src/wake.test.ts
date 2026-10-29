import assert from 'node:assert/strict';
import { test } from 'node:test';

import { agentName, ensureAwake, type WakeDeps } from './wake.js';

type Script = { states: string[]; reachableAfter: number };

function rig(script: Script) {
  let clock = 0;
  const calls: string[] = [];
  let pings = 0;
  const deps: WakeDeps = {
    now: () => clock,
    sleep: async (ms) => {
      clock += ms;
    },
    fetch: async (input) => {
      const url = String(input);
      calls.push(url);
      if (url.endsWith('/v1/wake')) {
        const state = script.states.shift() ?? 'awake';
        return new Response(JSON.stringify({ state }), { status: 200 });
      }
      pings += 1;
      return new Response('{}', { status: pings > script.reachableAfter ? 200 : 530 });
    },
  };
  return { deps, calls, clock: () => clock };
}

test('an awake computer is connected to at once, with a single cheap request', async () => {
  const { deps, calls } = rig({ states: ['awake'], reachableAfter: 0 });
  let shown = 0;
  const waited = await ensureAwake('https://api.example', 'ana.wirehaus.dev', () => (shown += 1), deps);
  assert.equal(waited, false);
  assert.equal(shown, 0);
  assert.deepEqual(calls, ['https://api.example/v1/wake']);
});

test('a sleeping computer is woken, the wait is shown and the tunnel is probed before connecting', async () => {
  const { deps, calls } = rig({ states: ['waking', 'waking', 'awake'], reachableAfter: 2 });
  let shown = 0;
  const waited = await ensureAwake('https://api.example', 'ana.wirehaus.dev', () => (shown += 1), deps);
  assert.equal(waited, true);
  assert.equal(shown, 1);
  assert.equal(calls.filter((c) => c.endsWith('/v1/wake')).length, 3);
  assert.equal(calls.filter((c) => c.endsWith('/link/ping')).length, 4);
});

test('an unknown name, a missing api or a host without a name does nothing', async () => {
  const { deps, calls } = rig({ states: ['unknown'], reachableAfter: 0 });
  await ensureAwake('https://api.example', 'ana.wirehaus.dev', () => undefined, deps);
  await ensureAwake('', 'ana.wirehaus.dev', () => undefined, deps);
  await ensureAwake('https://api.example', '127.0.0.1:7878', () => undefined, deps);
  await ensureAwake('https://api.example', 'localhost', () => undefined, deps);
  assert.equal(calls.length, 1);
});

test('a failing registry never blocks the connection', async () => {
  const deps: WakeDeps = {
    now: () => 0,
    sleep: async () => undefined,
    fetch: async () => {
      throw new Error('offline');
    },
  };
  await ensureAwake('https://api.example', 'ana.wirehaus.dev', () => undefined, deps);
});

test('the first probe waits a few seconds after the registry says the tunnel is up', async () => {
  const { deps, calls, clock } = rig({ states: ['waking', 'awake'], reachableAfter: 0 });
  let firstProbeAt = -1;
  const original = deps.fetch;
  deps.fetch = async (input, init) => {
    if (String(input).endsWith('/link/ping') && firstProbeAt < 0) {
      firstProbeAt = clock();
    }
    return original(input, init);
  };
  await ensureAwake('https://api.example', 'ana.wirehaus.dev', () => undefined, deps);
  assert.ok(firstProbeAt >= 2_000 + 5_000);
  assert.ok(calls.length > 0);
});

test('waiting is bounded when the computer never comes back', async () => {
  const { deps, clock } = rig({ states: Array<string>(1000).fill('waking'), reachableAfter: 0 });
  await ensureAwake('https://api.example', 'ana.wirehaus.dev', () => undefined, deps);
  assert.ok(clock() <= 335_000);
});

test('a flaky tunnel needs two good probes in a row before the connection is attempted', async () => {
  let n = 0;
  const deps: WakeDeps = {
    now: () => n * 1500,
    sleep: async () => {
      n += 1;
    },
    fetch: async (input) => {
      if (String(input).endsWith('/v1/wake')) {
        return new Response(JSON.stringify({ state: n === 0 ? 'waking' : 'awake' }), { status: 200 });
      }
      return new Response('{}', { status: [3, 5, 6].includes(n) ? 200 : 530 });
    },
  };
  await ensureAwake('https://api.example', 'ana.wirehaus.dev', () => undefined, deps);
  assert.equal(n, 6);
});

test('the name is the first label of a dotted host', () => {
  assert.equal(agentName('ana.wirehaus.dev'), 'ana');
  assert.equal(agentName('a-b.example.com'), 'a-b');
  assert.equal(agentName('ana.wirehaus.dev:8443'), undefined);
  assert.equal(agentName('localhost'), undefined);
  assert.equal(agentName('127.0.0.1:7878'), undefined);
});
