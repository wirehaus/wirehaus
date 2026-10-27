const MANIFEST = '/manifest.sha256';
const SHAPE = /^[0-9a-f]{64}[ \t]/u;
const POLL_MS = 5 * 60 * 1000;

export type Fetcher = (input: string, init?: RequestInit) => Promise<Pick<Response, 'ok' | 'text' | 'arrayBuffer'>>;

export type Verifier = (manifest: string) => Promise<boolean>;

const RETRY_MS = 20_000;
const MAX_RETRIES = 8;

export async function readManifest(fetcher: Fetcher): Promise<string | null> {
  try {
    const response = await fetcher(MANIFEST, { cache: 'no-store', credentials: 'omit', redirect: 'error' });
    if (!response.ok) {
      return null;
    }
    const text = (await response.text()).trim();
    return SHAPE.test(text) ? text : null;
  } catch {
    return null;
  }
}

function hex(bytes: ArrayBuffer): string {
  return [...new Uint8Array(bytes)].map((byte) => byte.toString(16).padStart(2, '0')).join('');
}

export async function servedConsistently(manifest: string, fetcher: Fetcher): Promise<boolean> {
  const entries = manifest
    .split('\n')
    .map((line) => /^([0-9a-f]{64})[ \t]+([A-Za-z0-9_.-]+)$/u.exec(line.trim()))
    .filter((match): match is RegExpExecArray => match !== null)
    .map((match) => ({ hash: match[1] ?? '', name: match[2] ?? '' }));
  if (entries.length === 0) {
    return false;
  }
  try {
    for (let start = 0; start < entries.length; start += 6) {
      const batch = entries.slice(start, start + 6);
      const results = await Promise.all(
        batch.map(async ({ hash, name }) => {
          const response = await fetcher(name === 'index.html' ? '/' : `/${name}`, { cache: 'no-store', credentials: 'omit' });
          return response.ok && hex(await crypto.subtle.digest('SHA-256', await response.arrayBuffer())) === hash;
        }),
      );
      if (results.some((same) => !same)) {
        return false;
      }
    }
    return true;
  } catch {
    return false;
  }
}

export class UpdateWatcher {
  private baseline: string | null = null;
  private announced = false;
  private timer: number | undefined;
  private retries = 0;

  constructor(
    private readonly onNewVersion: () => void,
    private readonly fetcher: Fetcher = (input, init) => fetch(input, init),
    private readonly verify: Verifier = (manifest) => servedConsistently(manifest, this.fetcher),
  ) {}

  start(): void {
    void this.check();
    this.timer = window.setInterval(() => void this.check(), POLL_MS);
    document.addEventListener('visibilitychange', () => {
      if (document.visibilityState === 'visible') {
        void this.check();
      }
    });
    window.addEventListener('online', () => void this.check());
  }

  private retryLater(): void {
    if (this.retries >= MAX_RETRIES) {
      return;
    }
    this.retries += 1;
    const timer: unknown = setTimeout(() => void this.check(), RETRY_MS);
    (timer as { unref?: () => void }).unref?.();
  }

  stop(): void {
    window.clearInterval(this.timer);
  }

  async check(): Promise<boolean> {
    if (this.announced) {
      return true;
    }
    const current = await readManifest(this.fetcher);
    if (current === null) {
      return false;
    }
    if (this.baseline === null) {
      this.baseline = current;
      return false;
    }
    if (current !== this.baseline) {
      if (!(await this.verify(current))) {
        this.retryLater();
        return false;
      }
      this.announced = true;
      this.onNewVersion();
      return true;
    }
    return false;
  }
}
