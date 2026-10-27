import { createHash } from 'node:crypto';

export const DEFAULT_CONNECT = 'wss://*.wirehaus.dev https://*.wirehaus.dev';
export const DEFAULT_API = 'https://api.wirehaus.dev';

const MODULE_EXTENSIONS = ['.js', '.mjs'];

export function integrityOf(bytes) {
  return `sha256-${createHash('sha256').update(bytes).digest('base64')}`;
}

export function pinWasm(moduleSource, wasmBytes) {
  const hash = createHash('sha256').update(wasmBytes).digest('hex');
  return replaceExactly(moduleSource, /export const WASM_SHA256 = '';/g, `export const WASM_SHA256 = '${hash}';`, 1);
}

export function moduleIntegrity(files) {
  const entries = [...files.entries()]
    .filter(([name]) => MODULE_EXTENSIONS.some((extension) => name.endsWith(extension)))
    .sort(([a], [b]) => (a < b ? -1 : 1))
    .map(([name, bytes]) => [`/${name}`, integrityOf(bytes)]);
  return JSON.stringify({ integrity: Object.fromEntries(entries) });
}

export function contentSecurityPolicy(importMapText, connect) {
  const hash = integrityOf(Buffer.from(importMapText));
  return [
    "default-src 'none'",
    `script-src 'self' 'wasm-unsafe-eval' '${hash}'`,
    "style-src 'self'",
    "img-src 'self' data:",
    "manifest-src 'self'",
    "worker-src 'self'",
    `connect-src 'self' ${connect}`,
    "base-uri 'none'",
    "form-action 'none'",
    "frame-ancestors 'none'",
  ].join('; ');
}

export function headersFile(policy) {
  return [
    '/*',
    '  ! Access-Control-Allow-Origin',
    `  Content-Security-Policy: ${policy}`,
    '  Cache-Control: no-cache',
    '  X-Content-Type-Options: nosniff',
    '  Referrer-Policy: no-referrer',
    '  Cross-Origin-Opener-Policy: same-origin',
    '  Cross-Origin-Resource-Policy: same-origin',
    '  Permissions-Policy: camera=(self), microphone=(), geolocation=(), payment=(), usb=()',
    '/manifest.sha256',
    '  Content-Type: text/plain; charset=utf-8',
    '',
  ].join('\n');
}

export function securityTxt(now, validDays) {
  const expires = new Date(now.getTime() + validDays * 86_400_000).toISOString();
  return [
    'Contact: mailto:security@wirehaus.dev',
    `Expires: ${expires}`,
    'Preferred-Languages: es, en',
    'Canonical: https://app.wirehaus.dev/.well-known/security.txt',
    '',
  ].join('\n');
}

function replaceExactly(html, pattern, replacement, expected) {
  const matches = html.match(pattern) ?? [];
  if (matches.length !== expected) {
    throw new Error(`expected ${expected} match(es) of ${pattern} in index.html, found ${matches.length}`);
  }
  return html.replace(pattern, replacement);
}

export function rewriteIndex(html, files, importMapText, api = '') {
  const integrity = (path) => {
    const bytes = files.get(path.slice(1));
    if (bytes === undefined) {
      throw new Error(`index.html references ${path}, which is not in the viewer`);
    }
    return integrityOf(bytes);
  };
  let page = replaceExactly(
    html,
    /content="own" \/>/g,
    api === '' ? 'content="configurable" />' : `content="configurable" />\n    <meta name="wirehaus-api" content="${api}" />`,
    1,
  );
  page = replaceExactly(
    page,
    /<link rel="stylesheet" href="(\/[^"]+)" \/>/g,
    (_, path) => `<link rel="stylesheet" href="${path}" integrity="${integrity(path)}" crossorigin="anonymous" />`,
    2,
  );
  return replaceExactly(
    page,
    /<script type="module" src="(\/[^"]+)"><\/script>/g,
    (_, path) =>
      `<script type="importmap">${importMapText}</script>\n    <script type="module" src="${path}" integrity="${integrity(path)}" crossorigin="anonymous"></script>`,
    1,
  );
}

export function redirectsFile() {
  return '/* /index.html 200\n';
}
