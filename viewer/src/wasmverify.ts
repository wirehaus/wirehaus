function hex(bytes: ArrayBuffer): string {
  return [...new Uint8Array(bytes)].map((byte) => byte.toString(16).padStart(2, '0')).join('');
}

export async function verifiedModule(expected: string, fetcher: typeof fetch = fetch): Promise<ArrayBuffer> {
  const response = await fetcher(new URL('wirehaus_crypto_wasm_bg.wasm', import.meta.url));
  if (!response.ok) {
    throw new Error('crypto module unavailable');
  }
  const bytes = await response.arrayBuffer();
  if (hex(await crypto.subtle.digest('SHA-256', bytes)) !== expected) {
    throw new Error('crypto module does not match the pinned hash');
  }
  return bytes;
}
