import init, { Handshake, Session, generateDeviceKey } from './wirehaus_crypto_wasm.js';
import { WASM_SHA256 } from './wasmhash.js';
import { verifiedModule } from './wasmverify.js';

let ready: Promise<unknown> | undefined;

async function load(): Promise<unknown> {
  if (WASM_SHA256 === '') {
    return init();
  }
  return init({ module_or_path: await verifiedModule(WASM_SHA256) });
}

export function loadCrypto(): Promise<unknown> {
  ready ??= load();
  return ready;
}

export { Handshake, Session, generateDeviceKey };
