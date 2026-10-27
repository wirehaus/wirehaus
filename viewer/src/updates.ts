const MANIFEST = '/manifest.sha256';
const SHAPE = /^[0-9a-f]{64}[ \t]/u;
const POLL_MS = 5 * 60 * 1000;

export type Fetcher = (input: string, init?: RequestInit) => Promise<Pick<Response, 'ok' | 'text'>>;

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

export class UpdateWatcher {
  private baseline: string | null = null;
  private announced = false;
  private timer: number | undefined;

  constructor(
    private readonly onNewVersion: () => void,
    private readonly fetcher: Fetcher = (input, init) => fetch(input, init),
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
      this.announced = true;
      this.onNewVersion();
      return true;
    }
    return false;
  }
}
