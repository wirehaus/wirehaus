import { copyFileSync, mkdirSync, readdirSync } from 'node:fs';
import { join } from 'node:path';

const source = new URL('../static/', import.meta.url).pathname;
const generated = new URL('../src/', import.meta.url).pathname;
const target = new URL('../dist/', import.meta.url).pathname;

mkdirSync(target, { recursive: true });
for (const name of readdirSync(source)) {
  copyFileSync(join(source, name), join(target, name));
}
for (const name of ['wirehaus_crypto_wasm.js', 'wirehaus_crypto_wasm_bg.wasm']) {
  copyFileSync(join(generated, name), join(target, name));
}
