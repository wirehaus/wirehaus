import { fromHex, toHex } from './linkaddr.js';
import { type LinkReply, encodeApprove, encodeLookup } from './linkprotocol.js';
import { linkGenerate, linkOpen, linkVerification, loadCrypto, qrDecode, qrMatrix } from './secure.js';

export type LinkStart = { id: string; manual: string; verify: string; ttl: number; link: string | undefined };

export type LinkEnd = 'expired' | 'busy' | 'slow' | 'failed' | 'cancelled';

const POLL_MS = 1500;
const PROBE_MS = 4000;
const REQUEST_MS = 8000;

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

export async function probeLink(base: string): Promise<boolean> {
  try {
    const { status, body } = await getJson(`${base}/link/ping`, PROBE_MS);
    return status === 200 && body['link'] === 1;
  } catch {
    return false;
  }
}

export class ComputerLink {
  private timer: number | undefined;
  private ended = false;
  private secret: Uint8Array | undefined;

  constructor(
    private readonly base: string,
    private readonly hooks: {
      started: (info: LinkStart) => void;
      code: (pairingCode: string) => void;
      ended: (reason: LinkEnd) => void;
    },
    private readonly addressOf: (idHex: string) => string | undefined,
  ) {}

  async start(): Promise<void> {
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
      const deadline = Date.now() + ttl * 1000;
      this.schedule(toHex(id), deadline);
    } catch {
      this.finish('failed');
    }
  }

  cancel(): void {
    this.finish('cancelled');
  }

  private schedule(id: string, deadline: number): void {
    this.timer = window.setTimeout(() => void this.poll(id, deadline), POLL_MS);
  }

  private async poll(id: string, deadline: number): Promise<void> {
    if (this.ended) {
      return;
    }
    if (Date.now() > deadline) {
      this.finish('expired');
      return;
    }
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
      if (body['state'] === 'gone') {
        this.finish('expired');
        return;
      }
    } catch {
      if (this.ended) {
        return;
      }
    }
    this.schedule(id, deadline);
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
  private waiting: ((reply: LinkReply) => void) | null = null;

  constructor(private readonly send: (payload: Uint8Array) => void) {}

  receive(reply: LinkReply): void {
    const waiting = this.waiting;
    this.waiting = null;
    waiting?.(reply);
  }

  lookup(key: string): Promise<LinkReply> {
    return this.ask(encodeLookup(key));
  }

  approve(id: Uint8Array, code: string): Promise<LinkReply> {
    return this.ask(encodeApprove(id, code));
  }

  private ask(payload: Uint8Array): Promise<LinkReply> {
    return new Promise((resolve, reject) => {
      this.waiting = resolve;
      this.send(payload);
      window.setTimeout(() => {
        if (this.waiting === resolve) {
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
