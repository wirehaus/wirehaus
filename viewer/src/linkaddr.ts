const HEX = /^[0-9a-f]+$/u;
const MANUAL = /^[A-Za-z2-7]{4}-?[A-Za-z2-7]{4}$/u;

export type LinkTarget = { id?: string; manual?: string; error?: 'other' | 'invalid' };

export function toHex(bytes: Uint8Array): string {
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, '0')).join('');
}

export function fromHex(text: string): Uint8Array | undefined {
  if (text.length % 2 !== 0 || !HEX.test(text)) {
    return undefined;
  }
  const out = new Uint8Array(text.length / 2);
  for (let index = 0; index < out.length; index += 1) {
    out[index] = Number.parseInt(text.slice(index * 2, index * 2 + 2), 16);
  }
  return out;
}

export function agentBase(socketUrl: string): string | undefined {
  try {
    const url = new URL(socketUrl);
    if (url.protocol !== 'wss:' && url.protocol !== 'ws:') {
      return undefined;
    }
    return `${url.protocol === 'wss:' ? 'https:' : 'http:'}//${url.host}`;
  } catch {
    return undefined;
  }
}

export function linkAddress(origin: string, name: string, idHex: string): string {
  return `${origin}/${name}#link=${idHex}`;
}

export function parseLinkText(text: string, origin: string, name: string): LinkTarget {
  const trimmed = text.trim();
  if (MANUAL.test(trimmed)) {
    return { manual: trimmed.replace('-', '').toUpperCase() };
  }
  if (/^[0-9a-f]{32}$/u.test(trimmed)) {
    return { id: trimmed };
  }
  let url: URL;
  try {
    url = new URL(trimmed);
  } catch {
    return { error: 'invalid' };
  }
  const id = new URLSearchParams(url.hash.replace(/^#/u, '')).get('link') ?? '';
  if (url.origin !== origin || !/^[0-9a-f]{32}$/u.test(id)) {
    return { error: 'invalid' };
  }
  const first = url.pathname.split('/').filter((part) => part !== '')[0]?.toLowerCase();
  return first === name.toLowerCase() ? { id } : { error: 'other' };
}
