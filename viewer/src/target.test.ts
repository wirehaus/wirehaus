import assert from 'node:assert/strict';
import { test } from 'node:test';

import { parseAgentAddress, resolveTarget } from './target.js';

const page = { protocol: 'https:', host: 'viewer.example.com', pathname: '/', hash: '' };

test('accepts plain hosts, hosts with a port and a leading secure scheme', () => {
  assert.equal(parseAgentAddress('user.wirehaus.dev'), 'user.wirehaus.dev');
  assert.equal(parseAgentAddress('  User.Wirehaus.DEV '), 'user.wirehaus.dev');
  assert.equal(parseAgentAddress('agent.example.com:8443'), 'agent.example.com:8443');
  assert.equal(parseAgentAddress('https://agent.example.com'), 'agent.example.com');
  assert.equal(parseAgentAddress('wss://agent.example.com/'), 'agent.example.com');
  assert.equal(parseAgentAddress('127.0.0.1:7878'), '127.0.0.1:7878');
  assert.equal(parseAgentAddress('agent.example.com\n'), 'agent.example.com');
});

test('rejects anything that is not a bare host and optional port', () => {
  const bad = [
    '',
    ' ',
    'http://agent.example.com',
    'ws://agent.example.com',
    'agent.example.com/path',
    'agent.example.com/ws',
    'agent.example.com?x=1',
    'agent.example.com#x',
    'user@agent.example.com',
    'user:pass@agent.example.com',
    'agent example.com',
    'agent.example.com:',
    'agent.example.com:0',
    'agent.example.com:65536',
    'agent.example.com:80:80',
    'agent.example.com:http',
    '[::1]:7878',
    '.example.com',
    'example..com',
    'example.com.',
    '-example.com',
    'example-.com',
    'exam_ple.com',
    'exämple.com',
    `${'a'.repeat(64)}.com`,
    `${'a.'.repeat(130)}com`,
    'https://https://agent.example.com',
    'agent.example\n.com',
    '//agent.example.com',
  ];
  for (const input of bad) {
    assert.equal(parseAgentAddress(input), undefined, JSON.stringify(input));
  }
});

test('a viewer served by the agent connects to its own origin', () => {
  const target = resolveTarget({ protocol: 'http:', host: '127.0.0.1:7878', pathname: '/', hash: '#agent=evil.example.com' }, false);
  assert.deepEqual(target, {
    authority: '127.0.0.1:7878',
    origin: 'http://127.0.0.1:7878',
    socketUrl: 'ws://127.0.0.1:7878/ws',
    foreign: false,
  });
});

test('a configurable viewer takes the agent from the fragment and always uses a secure socket for remote hosts', () => {
  const target = resolveTarget({ ...page, hash: '#agent=user.wirehaus.dev' }, true);
  assert.deepEqual(target, {
    authority: 'user.wirehaus.dev',
    origin: 'https://user.wirehaus.dev',
    socketUrl: 'wss://user.wirehaus.dev/ws',
    foreign: true,
  });
  const plain = resolveTarget({ protocol: 'http:', host: 'viewer.example.com', pathname: '/', hash: '#agent=user.wirehaus.dev' }, true);
  assert.equal(typeof plain === 'object' && plain.socketUrl, 'wss://user.wirehaus.dev/ws');
});

test('only loopback addresses get an unencrypted socket', () => {
  const local = resolveTarget({ ...page, hash: '#agent=localhost:7878' }, true);
  assert.equal(typeof local === 'object' && local.socketUrl, 'ws://localhost:7878/ws');
  const lookalike = resolveTarget({ ...page, hash: '#agent=localhost.evil.example' }, true);
  assert.equal(typeof lookalike === 'object' && lookalike.socketUrl, 'wss://localhost.evil.example/ws');
  const numeric = resolveTarget({ ...page, hash: '#agent=127.0.0.1.evil.example' }, true);
  assert.equal(typeof numeric === 'object' && numeric.socketUrl, 'wss://127.0.0.1.evil.example/ws');
});

test('a configurable viewer without a usable address reports why', () => {
  assert.equal(resolveTarget(page, true), 'missing');
  assert.equal(resolveTarget({ ...page, hash: '#other=1' }, true), 'missing');
  assert.equal(resolveTarget({ ...page, hash: '#agent=' }, true), 'invalid');
  assert.equal(resolveTarget({ ...page, hash: '#agent=a.example.com&agent=b.example.com' }, true), 'invalid');
  assert.equal(resolveTarget({ ...page, hash: '#agent=http://a.example.com' }, true), 'invalid');
  assert.equal(resolveTarget({ ...page, hash: '#agent=a.example.com/x' }, true), 'invalid');
});

const app = { protocol: 'https:', host: 'app.wirehaus.dev', hash: '' };

test('a single name in the path selects that name under the parent domain', () => {
  for (const pathname of ['/cristobal', '/Cristobal/', '/mi-equipo']) {
    const target = resolveTarget({ ...app, pathname }, true);
    assert.ok(typeof target === 'object');
    assert.equal(target.authority, `${pathname.replace(/\//g, '').toLowerCase()}.wirehaus.dev`);
    assert.equal(target.socketUrl, `wss://${target.authority}/ws`);
  }
});

test('reserved or malformed names in the path are refused', () => {
  for (const pathname of ['/app', '/www', '/a/b', '/-x', '/x-', '/a.b', '/%2e', `/${'a'.repeat(64)}`, '//x']) {
    assert.equal(resolveTarget({ ...app, pathname }, true), 'invalid', pathname);
  }
  assert.equal(resolveTarget({ ...app, pathname: '/' }, true), 'missing');
});

test('an explicit agent in the fragment wins over the path', () => {
  const target = resolveTarget({ ...app, pathname: '/cristobal', hash: '#agent=other.wirehaus.dev' }, true);
  assert.ok(typeof target === 'object');
  assert.equal(target.authority, 'other.wirehaus.dev');
});

test('the path is ignored when the agent serves its own viewer', () => {
  const target = resolveTarget({ protocol: 'http:', host: '127.0.0.1:7878', pathname: '/x', hash: '' }, false);
  assert.ok(typeof target === 'object' && !target.foreign);
});

test('access names follow the same rules as the registry', async () => {
  const { validAccessName, cleanName, nameFromPath, namedByPath } = await import('./target.js');
  assert.equal(validAccessName('cristobal'), true);
  assert.equal(validAccessName('a-b-c'), true);
  assert.equal(validAccessName('ab'), false);
  assert.equal(validAccessName('123'), false);
  assert.equal(validAccessName('-ab'), false);
  assert.equal(validAccessName('a--b'), false);
  assert.equal(validAccessName('Maria'), false);
  assert.equal(validAccessName('admin'), false);
  assert.equal(validAccessName('x'.repeat(33)), false);
  assert.equal(cleanName('  Maria.wirehaus.dev/ '), 'maria');
  assert.equal(cleanName('https://app.wirehaus.dev/cristobal'), 'cristobal');
  assert.equal(nameFromPath('/Ana/'), 'ana');
  assert.equal(nameFromPath('/'), undefined);
  const page = { protocol: 'https:', host: 'app.wirehaus.dev', pathname: '/ana', hash: '' };
  assert.equal(namedByPath(page), 'ana');
  assert.equal(namedByPath({ ...page, hash: '#agent=ana.wirehaus.dev' }), undefined);
  assert.equal(namedByPath({ ...page, pathname: '/' }), undefined);
});
