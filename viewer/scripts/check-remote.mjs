import { createHash } from 'node:crypto';

const [base, expected] = process.argv.slice(2);
if (base === undefined) {
  console.error('usage: node scripts/check-remote.mjs <base-url> [expected-bundle-hash]');
  process.exit(2);
}

const sha256 = (bytes) => createHash('sha256').update(bytes).digest('hex');
const get = async (name) => {
  const response = await fetch(new URL(name, base));
  if (!response.ok) {
    throw new Error(`${name}: HTTP ${response.status}`);
  }
  return new Uint8Array(await response.arrayBuffer());
};

const manifest = await get('manifest.sha256');
const bundle = sha256(manifest);
console.log(`bundle ${bundle}`);
if (expected !== undefined && expected !== bundle) {
  console.error(`the served viewer has hash ${bundle} but ${expected} was expected`);
  process.exit(1);
}

const lines = new TextDecoder().decode(manifest).trimEnd().split('\n');
let failures = 0;
for (const line of lines) {
  const [hash, name] = line.split('  ');
  const actual = sha256(await get(name));
  if (actual !== hash) {
    console.error(`${name}: served content does not match the manifest`);
    failures += 1;
  }
}
if (failures > 0) {
  process.exit(1);
}
console.log(`${lines.length} files match the manifest`);
