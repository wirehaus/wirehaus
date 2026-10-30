import { fromHex, toHex } from './linkaddr.js';
import { type LinkReply, encodeApprove, encodeApproveId, encodeApproveKey, encodeLookup } from './linkprotocol.js';
import { linkGenerate, linkOpen, linkVerification, loadCrypto, qrDecode, qrMatrix } from './secure.js';

export type LinkStart = { id: string; manual: string | undefined; verify: string; ttl: number; link: string | undefined };

export type LinkEnd = 'expired' | 'busy' | 'slow' | 'failed' | 'cancelled';

const POLL_MS = 1500;
const PROBE_MS = 4000;
const REQUEST_MS = 8000;
const V2_TTL = 600;

async function getJson(url: string, timeout = REQUEST_MS): Promise<{ status: number; body: Record<string, unknown> }> {
  const controller = new AbortController();
  const timer = window.setTimeout(() => controller.abort(), timeout);
  try {
    const response = await fetch(url, {
      cache: 'no-store',
      credentials: 'omit',
      referrerPolicy: 'no-referrer',
      signal: controller.signal,
    });
    const body = (await response.json().catch(() => ({}))) as Record<string, unknown>;
    return { status: response.status, body };
  } finally {
    window.clearTimeout(timer);
  }
}

function pause(ms: number): Promise<void> {
  return new Promise((resolve) => window.setTimeout(resolve, ms));
}

export type LinkLevel = 'unknown' | 'none' | 1 | 2;

export async function probeLink(base: string): Promise<LinkLevel> {
  try {
    const { status, body } = await getJson(`${base}/link/ping`, PROBE_MS);
    if (status !== 200) {
      return status === 404 ? 'none' : 'unknown';
    }
    const level = body['link'];
    return level === 1 || level === 2 ? level : 'none';
  } catch {
    return 'unknown';
  }
}

export async function probeLinkPatiently(base: string, tries = 3): Promise<LinkLevel> {
  let level: LinkLevel = 'unknown';
  for (let attempt = 0; attempt < tries && level === 'unknown'; attempt += 1) {
    level = await probeLink(base);
  }
  return level;
}

export class ComputerLink {
  private timer: number | undefined;
  private ended = false;
  private secret: Uint8Array | undefined;
  private ids: string[] = [];

  constructor(
    private readonly base: string,
    private readonly hooks: {
      started: (info: LinkStart) => void;
      manual?: (manual: string) => void;
      code: (pairingCode: string) => void;
      ended: (reason: LinkEnd) => void;
    },
    private readonly addressOf: (idHex: string) => string | undefined,
  ) {}

  async start(level: LinkLevel = 2): Promise<void> {
    if (level === 1) {
      await this.startRegistered();
      return;
    }
    try {
      await loadCrypto();
      const keys = linkGenerate();
      this.secret = keys.slice(0, 32);
      const publicKey = keys.slice(32);
      keys.fill(0);
      const id = publicKey.slice(0, 16);
      this.ids = [toHex(id)];
      this.hooks.started({
        id: toHex(id),
        manual: undefined,
        verify: linkVerification(publicKey, id),
        ttl: V2_TTL,
        link: this.addressOf(toHex(publicKey)),
      });
      this.schedule(Date.now() + V2_TTL * 1000);
      void this.registerManual(publicKey);
    } catch {
      this.finish('failed');
    }
  }

  private async registerManual(publicKey: Uint8Array): Promise<void> {
    for (let attempt = 0; attempt < 4 && !this.ended; attempt += 1) {
      try {
        const { status, body } = await getJson(`${this.base}/link/start?k=${toHex(publicKey)}`);
        if (status === 200 && typeof body['id'] === 'string' && typeof body['manual'] === 'string') {
          this.ids = [...this.ids, body['id']];
          this.hooks.manual?.(body['manual']);
          return;
        }
        if (status !== 429) {
          return;
        }
      } catch {
        await pause(3000);
        continue;
      }
      await pause(3000);
    }
  }

  private async startRegistered(): Promise<void> {
    try {
      await loadCrypto();
      const keys = linkGenerate();
      this.secret = keys.slice(0, 32);
      const publicKey = keys.slice(32);
      keys.fill(0);
      const { status, body } = await getJson(`${this.base}/link/start?k=${toHex(publicKey)}`);
      if (status === 429) {
        this.finish(body['error'] === 'slow' ? 'slow' : 'busy');
        return;
      }
      const id = typeof body['id'] === 'string' ? fromHex(body['id']) : undefined;
      if (status !== 200 || id === undefined || id.length !== 16 || typeof body['manual'] !== 'string') {
        this.finish('failed');
        return;
      }
      if (linkVerification(publicKey, id) !== body['verify']) {
        this.finish('failed');
        return;
      }
      const ttl = typeof body['ttl'] === 'number' ? body['ttl'] : 180;
      this.hooks.started({
        id: toHex(id),
        manual: body['manual'],
        verify: String(body['verify']),
        ttl,
        link: this.addressOf(toHex(id)),
      });
      this.ids = [toHex(id)];
      this.schedule(Date.now() + ttl * 1000);
    } catch {
      this.finish('failed');
    }
  }

  cancel(): void {
    this.finish('cancelled');
  }

  private schedule(deadline: number): void {
    this.timer = window.setTimeout(() => void this.poll(deadline), POLL_MS);
  }

  private async poll(deadline: number): Promise<void> {
    if (this.ended) {
      return;
    }
    if (Date.now() > deadline) {
      this.finish('expired');
      return;
    }
    for (const id of [...this.ids]) {
      try {
        const { body } = await getJson(`${this.base}/link/poll?i=${id}`);
        if (this.ended) {
          return;
        }
        if (body['state'] === 'ready' && typeof body['sealed'] === 'string' && this.secret !== undefined) {
          const sealed = fromHex(body['sealed']);
          if (sealed === undefined) {
            this.finish('failed');
            return;
          }
          const code = linkOpen(this.secret, sealed);
          this.ended = true;
          this.wipe();
          this.hooks.code(code);
          return;
        }
        if (body['state'] === 'gone' && this.ids.length === 1) {
          this.finish('expired');
          return;
        }
      } catch {
        if (this.ended) {
          return;
        }
      }
    }
    this.schedule(deadline);
  }

  private wipe(): void {
    this.secret?.fill(0);
    this.secret = undefined;
    window.clearTimeout(this.timer);
  }

  private finish(reason: LinkEnd): void {
    if (this.ended) {
      return;
    }
    this.ended = true;
    this.wipe();
    this.hooks.ended(reason);
  }
}

export class LinkApprover {
  private waiting: { accepts: (reply: LinkReply) => boolean; resolve: (reply: LinkReply) => void } | null = null;

  constructor(private readonly send: (payload: Uint8Array) => void) {}

  receive(reply: LinkReply): void {
    const waiting = this.waiting;
    if (waiting === null || !waiting.accepts(reply)) {
      return;
    }
    this.waiting = null;
    waiting.resolve(reply);
  }

  lookup(key: string): Promise<LinkReply> {
    return this.ask(encodeLookup(key), (reply) => reply.kind === 'info' || reply.kind === 'notFound' || reply.kind === 'failed');
  }

  approve(id: Uint8Array, code: string): Promise<LinkReply> {
    return this.ask(encodeApprove(id, code), (reply) => reply.kind === 'approved' || reply.kind === 'failed');
  }

  approveKey(key: Uint8Array): Promise<LinkReply> {
    return this.ask(encodeApproveKey(key), (reply) => reply.kind === 'approved' || reply.kind === 'failed');
  }

  approveId(id: Uint8Array): Promise<LinkReply> {
    return this.ask(encodeApproveId(id), (reply) => reply.kind === 'approved' || reply.kind === 'failed');
  }

  private ask(payload: Uint8Array, accepts: (reply: LinkReply) => boolean): Promise<LinkReply> {
    return new Promise((resolve, reject) => {
      const entry = { accepts, resolve };
      this.waiting = entry;
      this.send(payload);
      window.setTimeout(() => {
        if (this.waiting === entry) {
          this.waiting = null;
          reject(new Error('timeout'));
        }
      }, REQUEST_MS);
    });
  }
}

export function drawQr(canvas: HTMLCanvasElement, text: string): boolean {
  let data: Uint8Array;
  try {
    data = qrMatrix(text);
  } catch {
    return false;
  }
  const side = data[0] ?? 0;
  const quiet = 4;
  const scale = Math.max(2, Math.floor(320 / (side + 2 * quiet)));
  const size = (side + 2 * quiet) * scale;
  canvas.width = size;
  canvas.height = size;
  const context = canvas.getContext('2d');
  if (context === null) {
    return false;
  }
  context.fillStyle = '#ffffff';
  context.fillRect(0, 0, size, size);
  context.fillStyle = '#000000';
  for (let y = 0; y < side; y += 1) {
    for (let x = 0; x < side; x += 1) {
      if (data[1 + y * side + x] === 1) {
        context.fillRect((x + quiet) * scale, (y + quiet) * scale, scale, scale);
      }
    }
  }
  return true;
}

export type ScanError = 'denied' | 'unavailable';

export async function startScanner(
  video: HTMLVideoElement,
  found: (text: string) => void,
): Promise<{ stop: () => void } | ScanError> {
  if (navigator.mediaDevices?.getUserMedia === undefined) {
    return 'unavailable';
  }
  let stream: MediaStream;
  try {
    stream = await navigator.mediaDevices.getUserMedia({
      audio: false,
      video: { facingMode: { ideal: 'environment' }, width: { ideal: 1280 } },
    });
  } catch (error) {
    return error instanceof DOMException && (error.name === 'NotAllowedError' || error.name === 'SecurityError')
      ? 'denied'
      : 'unavailable';
  }
  await loadCrypto();
  video.srcObject = stream;
  video.setAttribute('playsinline', 'true');
  await video.play().catch(() => undefined);
  const canvas = document.createElement('canvas');
  const timer = window.setInterval(() => {
    const width = video.videoWidth;
    const height = video.videoHeight;
    if (width === 0 || height === 0) {
      return;
    }
    const scale = Math.min(1, 640 / width);
    canvas.width = Math.round(width * scale);
    canvas.height = Math.round(height * scale);
    const context = canvas.getContext('2d', { willReadFrequently: true });
    if (context === null) {
      return;
    }
    context.drawImage(video, 0, 0, canvas.width, canvas.height);
    const pixels = context.getImageData(0, 0, canvas.width, canvas.height).data;
    const luma = new Uint8Array(canvas.width * canvas.height);
    for (let index = 0; index < luma.length; index += 1) {
      const offset = index * 4;
      luma[index] = ((pixels[offset] ?? 0) * 77 + (pixels[offset + 1] ?? 0) * 150 + (pixels[offset + 2] ?? 0) * 29) >> 8;
    }
    const text = qrDecode(canvas.width, canvas.height, luma);
    if (text !== undefined && text !== '') {
      found(text);
    }
  }, 200);
  return {
    stop: () => {
      window.clearInterval(timer);
      for (const track of stream.getTracks()) {
        track.stop();
      }
      video.srcObject = null;
    },
  };
}
