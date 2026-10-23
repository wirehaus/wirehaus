const DECODE_TIMEOUT_MS = 2000;
const FRAME_DURATION_US = 33_000;

export function codecFromKeyframe(data: Uint8Array): string | undefined {
  for (let at = 0; at + 6 < data.length; at += 1) {
    const startCode = data[at] === 0 && data[at + 1] === 0 && data[at + 2] === 1;
    const header = data[at + 3];
    if (startCode && header !== undefined && (header & 0x1f) === 7) {
      const profile = data[at + 4];
      const constraints = data[at + 5];
      const level = data[at + 6];
      if (profile === undefined || constraints === undefined || level === undefined) {
        return undefined;
      }
      return `avc1.${hex(profile)}${hex(constraints)}${hex(level)}`;
    }
  }
  return undefined;
}

function hex(value: number): string {
  return value.toString(16).padStart(2, '0');
}

export async function videoSupported(): Promise<boolean> {
  if (typeof VideoDecoder === 'undefined') {
    return false;
  }
  try {
    const result = await VideoDecoder.isConfigSupported({ codec: 'avc1.42c01f' });
    return result.supported === true;
  } catch {
    return false;
  }
}

export class VideoPlayer {
  private decoder: VideoDecoder | null = null;
  private configured = '';
  private pending = 0;
  private waiters: (() => void)[] = [];
  private timestamp = 0;
  private failed = false;

  constructor(
    private readonly draw: (frame: VideoFrame) => void,
    private readonly onFailure: () => void,
  ) {}

  async play(width: number, height: number, keyframe: boolean, data: Uint8Array): Promise<void> {
    if (this.failed) {
      return;
    }
    if (keyframe) {
      const codec = codecFromKeyframe(data);
      if (codec === undefined) {
        this.fail();
        return;
      }
      this.configure(codec, width, height);
    }
    const decoder = this.decoder;
    if (decoder === null) {
      return;
    }
    this.pending += 1;
    try {
      decoder.decode(new EncodedVideoChunk({ type: keyframe ? 'key' : 'delta', timestamp: this.timestamp, data }));
      this.timestamp += FRAME_DURATION_US;
    } catch {
      this.pending -= 1;
      this.fail();
      return;
    }
    await this.idle();
  }

  rearm(): void {
    this.failed = false;
  }

  reset(): void {
    const decoder = this.decoder;
    this.decoder = null;
    this.configured = '';
    this.pending = 0;
    if (decoder !== null && decoder.state !== 'closed') {
      decoder.close();
    }
    this.release();
  }

  private configure(codec: string, width: number, height: number): void {
    const key = `${codec}/${width}x${height}`;
    if (this.decoder?.state === 'configured' && this.configured === key) {
      return;
    }
    this.reset();
    const decoder = new VideoDecoder({
      output: (frame) => {
        try {
          this.draw(frame);
        } finally {
          frame.close();
          this.settle();
        }
      },
      error: () => this.fail(),
    });
    decoder.configure({ codec, codedWidth: width, codedHeight: height, optimizeForLatency: true });
    this.decoder = decoder;
    this.configured = key;
  }

  private settle(): void {
    this.pending = Math.max(0, this.pending - 1);
    if (this.pending === 0) {
      this.release();
    }
  }

  private release(): void {
    for (const waiter of this.waiters.splice(0)) {
      waiter();
    }
  }

  private idle(): Promise<void> {
    if (this.pending === 0) {
      return Promise.resolve();
    }
    return new Promise<void>((resolve) => {
      const timer = setTimeout(() => {
        this.fail();
        resolve();
      }, DECODE_TIMEOUT_MS);
      this.waiters.push(() => {
        clearTimeout(timer);
        resolve();
      });
    });
  }

  private fail(): void {
    if (this.failed) {
      return;
    }
    this.failed = true;
    this.reset();
    this.onFailure();
  }
}
