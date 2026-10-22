import init, { Handshake, Session, generateDeviceKey } from './wirehaus_crypto_wasm.js';

let ready: Promise<unknown> | undefined;

export function loadCrypto(): Promise<unknown> {
  ready ??= init();
  return ready;
}

export { Handshake, Session, generateDeviceKey };
