import { evdevCode } from './keymap.js';
import {
  CODEC_JPEG,
  DESKTOP_CHANNEL,
  type DesktopMessage,
  type InputEvent,
  type MouseButton,
  decodeDesktop,
  decodeFrame,
  encodeFrame,
  encodeFrameAck,
  encodeInput,
} from './protocol.js';

export type ViewerState =
  | { kind: 'connecting' }
  | { kind: 'connected'; width: number; height: number }
  | { kind: 'closed' };

const BUTTONS: Record<number, MouseButton> = { 0: 'left', 1: 'middle', 2: 'right' };
const PIXELS_PER_NOTCH = 100;
const MAX_NOTCHES = 10;

export class Viewer {
  private readonly context: CanvasRenderingContext2D;
  private socket: WebSocket | null = null;
  private drawing: Promise<void> = Promise.resolve();
  private readonly pressedKeys = new Set<number>();
  private readonly pressedButtons = new Set<MouseButton>();
  private pendingMove: { x: number; y: number } | null = null;

  constructor(
    private readonly canvas: HTMLCanvasElement,
    private readonly report: (state: ViewerState) => void,
  ) {
    const context = canvas.getContext('2d', { alpha: false });
    if (context === null) {
      throw new Error('canvas 2d context is not available');
    }
    this.context = context;
    this.listen();
    new ResizeObserver(() => this.fit()).observe(canvas.parentElement ?? canvas);
  }

  connect(): void {
    this.socket?.close();
    this.report({ kind: 'connecting' });
    const scheme = location.protocol === 'https:' ? 'wss' : 'ws';
    const socket = new WebSocket(`${scheme}://${location.host}/ws`);
    socket.binaryType = 'arraybuffer';
    socket.addEventListener('message', (event) => this.receive(event.data));
    socket.addEventListener('close', () => {
      if (this.socket === socket) {
        this.socket = null;
        this.releaseAll();
        this.report({ kind: 'closed' });
      }
    });
    this.socket = socket;
  }

  private receive(data: unknown): void {
    if (!(data instanceof ArrayBuffer)) {
      return;
    }
    try {
      const frame = decodeFrame(new Uint8Array(data));
      if (frame.channel === DESKTOP_CHANNEL) {
        this.apply(decodeDesktop(frame.payload));
      }
    } catch {
      this.socket?.close();
    }
  }

  private apply(message: DesktopMessage): void {
    if (message.kind === 'resize') {
      this.canvas.width = message.width;
      this.canvas.height = message.height;
      this.fit();
      this.report({ kind: 'connected', width: message.width, height: message.height });
      this.canvas.focus();
      return;
    }
    if (message.kind === 'frameEnd') {
      const sequence = message.sequence;
      this.drawing = this.drawing.then(() => this.acknowledge(sequence));
      return;
    }
    if (message.codec !== CODEC_JPEG) {
      return;
    }
    const bytes = new Uint8Array(message.data.length);
    bytes.set(message.data);
    this.drawing = this.drawing
      .then(async () => {
        const bitmap = await createImageBitmap(new Blob([bytes], { type: 'image/jpeg' }));
        this.context.drawImage(bitmap, message.x, message.y);
        bitmap.close();
      })
      .catch(() => undefined);
  }

  private acknowledge(sequence: number): void {
    if (this.socket?.readyState === WebSocket.OPEN) {
      this.socket.send(encodeFrame(DESKTOP_CHANNEL, 0, encodeFrameAck(sequence)));
    }
  }

  private fit(): void {
    const area = this.canvas.parentElement;
    if (area === null || this.canvas.width === 0 || this.canvas.height === 0) {
      return;
    }
    const scale = Math.min(area.clientWidth / this.canvas.width, area.clientHeight / this.canvas.height);
    this.canvas.style.width = `${Math.floor(this.canvas.width * scale)}px`;
    this.canvas.style.height = `${Math.floor(this.canvas.height * scale)}px`;
  }

  private send(event: InputEvent): void {
    if (this.socket?.readyState === WebSocket.OPEN) {
      this.socket.send(encodeFrame(DESKTOP_CHANNEL, 0, encodeInput(event)));
    }
  }

  private position(event: MouseEvent): { x: number; y: number } | null {
    const rect = this.canvas.getBoundingClientRect();
    if (rect.width === 0 || rect.height === 0) {
      return null;
    }
    const x = Math.floor(((event.clientX - rect.left) * this.canvas.width) / rect.width);
    const y = Math.floor(((event.clientY - rect.top) * this.canvas.height) / rect.height);
    return {
      x: Math.min(Math.max(x, 0), this.canvas.width - 1),
      y: Math.min(Math.max(y, 0), this.canvas.height - 1),
    };
  }

  private listen(): void {
    const canvas = this.canvas;
    canvas.addEventListener('contextmenu', (event) => event.preventDefault());
    canvas.addEventListener('pointermove', (event) => this.queueMove(event));
    canvas.addEventListener('pointerdown', (event) => this.pointerButton(event, true));
    canvas.addEventListener('pointerup', (event) => this.pointerButton(event, false));
    canvas.addEventListener('pointercancel', () => this.releaseButtons());
    canvas.addEventListener('wheel', (event) => this.wheel(event), { passive: false });
    canvas.addEventListener('keydown', (event) => this.key(event, true));
    canvas.addEventListener('keyup', (event) => this.key(event, false));
    canvas.addEventListener('blur', () => this.releaseAll());
  }

  private queueMove(event: PointerEvent): void {
    const position = this.position(event);
    if (position === null) {
      return;
    }
    const idle = this.pendingMove === null;
    this.pendingMove = position;
    if (idle) {
      requestAnimationFrame(() => {
        if (this.pendingMove !== null) {
          this.send({ kind: 'pointer', ...this.pendingMove });
          this.pendingMove = null;
        }
      });
    }
  }

  private pointerButton(event: PointerEvent, pressed: boolean): void {
    const button = BUTTONS[event.button];
    event.preventDefault();
    if (button === undefined) {
      return;
    }
    const position = this.position(event);
    if (position !== null) {
      this.pendingMove = null;
      this.send({ kind: 'pointer', ...position });
    }
    if (pressed) {
      this.canvas.focus();
      this.canvas.setPointerCapture(event.pointerId);
      this.pressedButtons.add(button);
    } else {
      this.pressedButtons.delete(button);
    }
    this.send({ kind: 'button', button, pressed });
  }

  private wheel(event: WheelEvent): void {
    event.preventDefault();
    const horizontal = notches(event.deltaX, event.deltaMode);
    const vertical = -notches(event.deltaY, event.deltaMode);
    if (horizontal !== 0 || vertical !== 0) {
      this.send({ kind: 'wheel', horizontal, vertical });
    }
  }

  private key(event: KeyboardEvent, pressed: boolean): void {
    const code = evdevCode(event.code);
    if (code === undefined) {
      return;
    }
    event.preventDefault();
    event.stopPropagation();
    if (pressed) {
      this.pressedKeys.add(code);
    } else {
      this.pressedKeys.delete(code);
    }
    this.send({ kind: 'key', code, pressed });
  }

  private releaseButtons(): void {
    for (const button of this.pressedButtons) {
      this.send({ kind: 'button', button, pressed: false });
    }
    this.pressedButtons.clear();
  }

  private releaseAll(): void {
    this.releaseButtons();
    for (const code of this.pressedKeys) {
      this.send({ kind: 'key', code, pressed: false });
    }
    this.pressedKeys.clear();
  }
}

function notches(delta: number, mode: number): number {
  if (delta === 0) {
    return 0;
  }
  const lines = mode === WheelEvent.DOM_DELTA_PIXEL ? delta / PIXELS_PER_NOTCH : delta;
  const count = Math.min(Math.max(Math.round(Math.abs(lines)), 1), MAX_NOTCHES);
  return Math.sign(delta) * count;
}
