import { copyFileSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const source = fileURLToPath(new URL('../static/', import.meta.url));
const generated = fileURLToPath(new URL('../src/', import.meta.url));
const target = fileURLToPath(new URL('../dist/', import.meta.url));

mkdirSync(target, { recursive: true });
for (const name of readdirSync(source)) {
  copyFileSync(join(source, name), join(target, name));
}
for (const name of ['wirehaus_crypto_wasm.js', 'wirehaus_crypto_wasm_bg.wasm']) {
  copyFileSync(join(generated, name), join(target, name));
}

const packages = fileURLToPath(new URL('../node_modules/@xterm/', import.meta.url));
copyFileSync(join(packages, 'xterm/lib/xterm.mjs'), join(target, 'xterm.mjs'));
copyFileSync(join(packages, 'xterm/css/xterm.css'), join(target, 'xterm.css'));
copyFileSync(join(packages, 'addon-fit/lib/addon-fit.mjs'), join(target, 'addon-fit.mjs'));
for (const name of readdirSync(target)) {
  if (name.endsWith('.js')) {
    const path = join(target, name);
    const rewritten = readFileSync(path, 'utf8')
      .replaceAll("'@xterm/xterm'", "'./xterm.mjs'")
      .replaceAll("'@xterm/addon-fit'", "'./addon-fit.mjs'");
    writeFileSync(path, rewritten);
  }
}
