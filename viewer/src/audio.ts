import type { AudioFailure, AudioMessage } from './audioprotocol.js';
import { encodeAudioStart, encodeAudioStop } from './audioprotocol.js';

const SAMPLE_RATE = 48_000;
const CHANNELS = 2;
const PACKET_MICROSECONDS = 20_000;
const TARGET_LATENCY = 0.08;
const MAX_AHEAD = 0.35;

export type Playback = { startAt: number; next: number };

export function planPlayback(now: number, nextTime: number, duration: number): Playback | null {
  if (nextTime <= now) {
    const startAt = now + TARGET_LATENCY;
    return { startAt, next: startAt + duration };
  }
  if (nextTime - now > MAX_AHEAD) {
    return null;
  }
  return { startAt: nextTime, next: nextTime + duration };
}

export function audioSupported(): boolean {
  return typeof AudioDecoder !== 'undefined' && typeof AudioContext !== 'undefined' && typeof EncodedAudioChunk !== 'undefined';
}

export class AudioPlayer {
  private context: AudioContext | null = null;
  private decoder: AudioDecoder | null = null;
  private clock = 0;
  private nextTime = 0;

  async start(): Promise<void> {
    const config: AudioDecoderConfig = { codec: 'opus', sampleRate: SAMPLE_RATE, numberOfChannels: CHANNELS };
    const support = await AudioDecoder.isConfigSupported(config);
    if (support.supported !== true) {
      throw new Error('opus is not supported');
    }
    const context = new AudioContext({ sampleRate: SAMPLE_RATE, latencyHint: 'interactive' });
    await context.resume();
    const decoder = new AudioDecoder({
      output: (data) => this.play(data),
      error: () => this.stop(),
    });
    decoder.configure(config);
    this.context = context;
    this.decoder = decoder;
    this.clock = 0;
    this.nextTime = 0;
  }

  push(packet: Uint8Array): void {
    const decoder = this.decoder;
    if (decoder === null || decoder.state !== 'configured') {
      return;
    }
    decoder.decode(new EncodedAudioChunk({ type: 'key', timestamp: this.clock, data: packet }));
    this.clock += PACKET_MICROSECONDS;
  }

  stop(): void {
    const decoder = this.decoder;
    const context = this.context;
    this.decoder = null;
    this.context = null;
    if (decoder !== null && decoder.state !== 'closed') {
      decoder.close();
    }
    if (context !== null && context.state !== 'closed') {
      void context.close();
    }
  }

  private play(data: AudioData): void {
    const context = this.context;
    if (context === null) {
      data.close();
      return;
    }
    const plan = planPlayback(context.currentTime, this.nextTime, data.numberOfFrames / data.sampleRate);
    if (plan === null) {
      data.close();
      return;
    }
    const buffer = context.createBuffer(data.numberOfChannels, data.numberOfFrames, data.sampleRate);
    for (let channel = 0; channel < data.numberOfChannels; channel += 1) {
      const samples = new Float32Array(data.numberOfFrames);
      data.copyTo(samples, { planeIndex: channel, format: 'f32-planar' });
      buffer.copyToChannel(samples, channel);
    }
    data.close();
    const source = context.createBufferSource();
    source.buffer = buffer;
    source.connect(context.destination);
    source.start(plan.startAt);
    this.nextTime = plan.next;
  }
}

export type AudioTexts = {
  listen: string;
  stop: string;
  starting: string;
  unsupported: string;
  failure: (reason: AudioFailure) => string;
};

export class AudioControl {
  private readonly player = new AudioPlayer();
  private listening = false;
  private starting = false;

  constructor(
    private readonly button: HTMLButtonElement,
    private readonly io: { send(payload: Uint8Array): void },
    private readonly texts: AudioTexts,
    private readonly say: (text: string) => void,
  ) {
    this.refresh();
    button.addEventListener('click', () => void this.toggle());
  }

  receive(message: AudioMessage): void {
    switch (message.kind) {
      case 'started':
        this.starting = false;
        this.listening = true;
        this.say('');
        break;
      case 'packet':
        if (this.listening) {
          this.player.push(message.data);
        }
        break;
      case 'stopped':
        this.end('');
        break;
      case 'failed':
        this.end(this.texts.failure(message.reason));
        break;
    }
    this.refresh();
  }

  reset(): void {
    if (this.listening || this.starting) {
      this.end('');
    }
  }

  private async toggle(): Promise<void> {
    if (this.starting) {
      return;
    }
    if (this.listening) {
      this.io.send(encodeAudioStop());
      this.end('');
      return;
    }
    if (!audioSupported()) {
      this.say(this.texts.unsupported);
      return;
    }
    this.starting = true;
    this.say(this.texts.starting);
    this.refresh();
    try {
      await this.player.start();
    } catch {
      this.starting = false;
      this.say(this.texts.unsupported);
      this.refresh();
      return;
    }
    this.io.send(encodeAudioStart());
  }

  private end(notice: string): void {
    this.player.stop();
    this.listening = false;
    this.starting = false;
    this.say(notice);
    this.refresh();
  }

  private refresh(): void {
    this.button.textContent = this.listening ? this.texts.stop : this.texts.listen;
    this.button.disabled = this.starting;
    this.button.setAttribute('aria-pressed', String(this.listening));
  }
}
