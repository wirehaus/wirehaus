import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { cpSync, existsSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import {
  DEFAULT_CONNECT,
  contentSecurityPolicy,
  headersFile,
  moduleIntegrity,
  redirectsFile,
  pinWasm,
  rewriteIndex,
  securityTxt,
} from './origin.mjs';

const SECURITY_TXT_VALID_DAYS = 180;

const root = fileURLToPath(new URL('../', import.meta.url));
const source = join(root, 'dist');

const arg = (name, fallback) => {
  const index = process.argv.indexOf(name);
  return index === -1 ? fallback : process.argv[index + 1];
};
const connect = arg('--connect', DEFAULT_CONNECT);
const target = arg('--out', join(root, 'dist-origin'));

if (!existsSync(join(source, 'index.html'))) {
  console.error('viewer/dist is missing: run npm run build first');
  process.exit(1);
}

rmSync(target, { recursive: true, force: true });
cpSync(source, target, { recursive: true });
rmSync(join(target, 'manifest.sha256'));

const hashModule = join(target, 'wasmhash.js');
writeFileSync(
  hashModule,
  pinWasm(readFileSync(hashModule, 'utf8'), readFileSync(join(target, 'wirehaus_crypto_wasm_bg.wasm'))),
);

const files = new Map(
  readdirSync(target).map((name) => [name, readFileSync(join(target, name))]),
);
const importMapText = moduleIntegrity(files);
const page = rewriteIndex(files.get('index.html').toString('utf8'), files, importMapText);
writeFileSync(join(target, 'index.html'), page);
writeFileSync(join(target, '_headers'), headersFile(contentSecurityPolicy(importMapText, connect)));
writeFileSync(join(target, '_redirects'), redirectsFile());
mkdirSync(join(target, '.well-known'));
writeFileSync(join(target, '.well-known/security.txt'), securityTxt(new Date(), SECURITY_TXT_VALID_DAYS));

execFileSync(
  'cargo',
  [
    'run',
    '--quiet',
    '--manifest-path',
    join(root, '../Cargo.toml'),
    '-p',
    'wirehaus-agent',
    '--locked',
    '--',
    'viewer',
    'manifest',
    '--viewer-dir',
    target,
  ],
  { stdio: 'inherit' },
);

const manifest = readFileSync(join(target, 'manifest.sha256'));
console.log(`origin bundle ${createHash('sha256').update(manifest).digest('hex')}`);
