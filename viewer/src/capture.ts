import { saveBlob, timestamp } from './download.js';

export type CaptureTexts = {
  screenshot: string;
  recordStart: string;
  recordStop: (seconds: number) => string;
  saved: (name: string) => string;
  unsupported: string;
  failed: string;
  limit: string;
};

const MIME_TYPES = [
  'video/webm;codecs=vp9',
  'video/webm;codecs=vp8',
  'video/webm',
  'video/mp4;codecs=avc1',
  'video/mp4',
];
const MAX_SECONDS = 30 * 60;
const MAX_BYTES = 1 << 30;
const BITRATE = 4_000_000;
const FRAMES_PER_SECOND = 30;

export class DesktopCapture {
  private recorder: MediaRecorder | null = null;
  private chunks: Blob[] = [];
  private bytes = 0;
  private startedAt = 0;
  private ticker: number | undefined;
  private overLimit = false;

  constructor(
    private readonly canvas: HTMLCanvasElement,
    shotButton: HTMLButtonElement,
    private readonly recordButton: HTMLButtonElement,
    private readonly texts: CaptureTexts,
    private readonly say: (text: string) => void,
  ) {
    shotButton.textContent = texts.screenshot;
    recordButton.textContent = texts.recordStart;
    shotButton.addEventListener('click', () => this.screenshot());
    recordButton.addEventListener('click', () => this.toggle());
  }

  recording(): boolean {
    return this.recorder !== null;
  }

  screenshot(): void {
    if (this.canvas.width === 0 || this.canvas.height === 0) {
      return;
    }
    this.canvas.toBlob((blob) => {
      if (blob === null) {
        this.say(this.texts.failed);
        return;
      }
      const name = `wirehaus-${timestamp()}.png`;
      saveBlob(blob, name);
      this.say(this.texts.saved(name));
    }, 'image/png');
  }

  toggle(): void {
    if (this.recorder === null) {
      this.start();
    } else {
      this.stop();
    }
  }

  stop(): void {
    const recorder = this.recorder;
    if (recorder !== null && recorder.state !== 'inactive') {
      recorder.stop();
    }
  }

  private start(): void {
    if (typeof MediaRecorder === 'undefined' || typeof this.canvas.captureStream !== 'function') {
      this.say(this.texts.unsupported);
      return;
    }
    const supported = MIME_TYPES.find((type) => MediaRecorder.isTypeSupported(type));
    let recorder: MediaRecorder;
    try {
      recorder = new MediaRecorder(
        this.canvas.captureStream(FRAMES_PER_SECOND),
        supported === undefined ? { videoBitsPerSecond: BITRATE } : { mimeType: supported, videoBitsPerSecond: BITRATE },
      );
    } catch {
      this.say(this.texts.failed);
      return;
    }
    this.chunks = [];
    this.bytes = 0;
    this.overLimit = false;
    recorder.addEventListener('dataavailable', (event) => this.collect(event.data));
    recorder.addEventListener('stop', () => this.finish(recorder.mimeType || supported || 'video/webm'));
    recorder.addEventListener('error', () => {
      this.say(this.texts.failed);
      this.stop();
    });
    this.startedAt = Date.now();
    this.recorder = recorder;
    recorder.start(1000);
    this.ticker = window.setInterval(() => this.tick(), 500);
    this.tick();
  }

  private collect(chunk: Blob): void {
    if (chunk.size === 0) {
      return;
    }
    this.chunks.push(chunk);
    this.bytes += chunk.size;
    if (this.bytes >= MAX_BYTES) {
      this.overLimit = true;
      this.stop();
    }
  }

  private tick(): void {
    const seconds = Math.floor((Date.now() - this.startedAt) / 1000);
    this.recordButton.textContent = this.texts.recordStop(seconds);
    if (seconds >= MAX_SECONDS) {
      this.overLimit = true;
      this.stop();
    }
  }

  private finish(mimeType: string): void {
    window.clearInterval(this.ticker);
    this.ticker = undefined;
    this.recorder = null;
    this.recordButton.textContent = this.texts.recordStart;
    const type = mimeType.split(';')[0] ?? 'video/webm';
    const blob = new Blob(this.chunks, { type });
    this.chunks = [];
    this.bytes = 0;
    if (blob.size === 0) {
      this.say(this.texts.failed);
      return;
    }
    const name = `wirehaus-${timestamp()}.${type === 'video/mp4' ? 'mp4' : 'webm'}`;
    saveBlob(blob, name);
    this.say(this.overLimit ? this.texts.limit : this.texts.saved(name));
  }
}
