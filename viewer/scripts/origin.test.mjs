import assert from 'node:assert/strict';
import { test } from 'node:test';
import {
  contentSecurityPolicy,
  headersFile,
  integrityOf,
  moduleIntegrity,
  pinWasm,
  rewriteIndex,
  securityTxt,
} from './origin.mjs';

const bytes = (text) => Buffer.from(text);
const viewer = () =>
  new Map([
    ['main.js', bytes('main')],
    ['viewer.js', bytes('viewer')],
    ['xterm.mjs', bytes('xterm')],
    ['style.css', bytes('style')],
    ['xterm.css', bytes('xtermcss')],
    ['wirehaus_crypto_wasm_bg.wasm', bytes('wasm')],
  ]);
const page = `<meta name="wirehaus-agent-address" content="own" />
<link rel="stylesheet" href="/xterm.css" />
<link rel="stylesheet" href="/style.css" />
<script type="module" src="/main.js"></script>`;

test('integrity is the base64 SHA-256 of the bytes', () => {
  assert.equal(integrityOf(bytes('abc')), 'sha256-ungWv48Bz+pBQUDeXa4iI7ADYaOWF3qctBD/YfIAFa0=');
});

test('only modules get an integrity entry, sorted and with absolute paths', () => {
  const map = JSON.parse(moduleIntegrity(viewer()));
  assert.deepEqual(Object.keys(map.integrity), ['/main.js', '/viewer.js', '/xterm.mjs']);
  assert.equal(map.integrity['/viewer.js'], integrityOf(bytes('viewer')));
});

test('the import map text is deterministic', () => {
  const reversed = new Map([...viewer().entries()].reverse());
  assert.equal(moduleIntegrity(viewer()), moduleIntegrity(reversed));
});

test('the policy admits only the hashed import map and the given connect sources', () => {
  const policy = contentSecurityPolicy('{"integrity":{}}', 'wss://*.wirehaus.dev');
  assert.match(policy, /^default-src 'none'; /);
  assert.ok(policy.includes(`'${integrityOf(bytes('{"integrity":{}}'))}'`));
  assert.ok(policy.includes("connect-src 'self' wss://*.wirehaus.dev;"));
  assert.ok(policy.includes("frame-ancestors 'none'"));
  assert.ok(!policy.includes('unsafe-inline'));
  assert.ok(!policy.includes("'unsafe-eval'"));
});

test('the headers file applies the policy to every path and types the manifest', () => {
  const text = headersFile("default-src 'none'");
  assert.ok(text.startsWith('/*\n  ! Access-Control-Allow-Origin\n  Content-Security-Policy: default-src \'none\'\n'));
  assert.ok(text.includes('  Cache-Control: no-cache\n'));
  assert.ok(text.includes('/manifest.sha256\n  Content-Type: text/plain; charset=utf-8\n'));
});

test('the rewritten page is configurable and pins every subresource', () => {
  const files = viewer();
  const map = moduleIntegrity(files);
  const result = rewriteIndex(page, files, map);
  assert.ok(result.includes('content="configurable"'));
  assert.ok(!result.includes('content="own"'));
  assert.ok(result.includes(`<script type="importmap">${map}</script>`));
  assert.ok(result.includes(`src="/main.js" integrity="${integrityOf(bytes('main'))}" crossorigin="anonymous"`));
  assert.ok(result.includes(`href="/style.css" integrity="${integrityOf(bytes('style'))}" crossorigin="anonymous"`));
  assert.ok(result.indexOf('importmap') < result.indexOf('type="module"'));
});

test('a page that does not match the expected shape is rejected', () => {
  const files = viewer();
  const map = moduleIntegrity(files);
  assert.throws(() => rewriteIndex(page.replace('content="own"', 'content="x"'), files, map));
  assert.throws(() => rewriteIndex(`${page}\n<script type="module" src="/main.js"></script>`, files, map));
  assert.throws(() => rewriteIndex(page.replace('/main.js', '/absent.js'), files, map), /absent\.js/);
});

test('security.txt names the contact and expires in the future', () => {
  const now = new Date('2026-10-26T12:00:00Z');
  const text = securityTxt(now, 180);
  assert.ok(text.includes('Contact: mailto:security@wirehaus.dev\n'));
  assert.ok(text.includes('Expires: 2027-04-24T12:00:00.000Z\n'));
  assert.ok(text.endsWith('\n'));
});

test('the wasm hash is written into the hash module and only once', () => {
  const source = "export const WASM_SHA256 = '';\n";
  const pinned = pinWasm(source, bytes('abc'));
  assert.equal(
    pinned,
    "export const WASM_SHA256 = 'ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad';\n",
  );
  assert.throws(() => pinWasm(pinned, bytes('abc')));
});
