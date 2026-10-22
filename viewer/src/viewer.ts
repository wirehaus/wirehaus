import { evdevCode } from './keymap.js';
import {
  CODEC_JPEG,
  CONTROL_CHANNEL,
  DESKTOP_CHANNEL,
  type AuthFailure,
  type DesktopMessage,
  type InputEvent,
  type MouseButton,
  decodeControl,
  decodeDesktop,
  decodeFrame,
  encodeFrame,
  encodeFrameAck,
  encodeInput,
  encodeTotp,
} from './protocol.js';
import { Handshake, type Session, generateDeviceKey, loadCrypto } from './secure.js';
import { type Identity, forgetIdentity, loadIdentity, saveIdentity } from './storage.js';

export type RefusalReason = Exclude<AuthFailure, 'wrongCode'> | 'unknown' | 'failed';

export type ViewerState =
  | { kind: 'loading' }
  | { kind: 'needsPairing'; error?: 'invalid' }
  | { kind: 'connecting' }
  | { kind: 'needsCode'; error?: 'wrong' | 'invalid' }
  | { kind: 'connected'; width: number; height: number }
  | { kind: 'refused'; reason: RefusalReason; retryAfter: number }
  | { kind: 'closed' };

type Phase = 'handshake' | 'authenticating' | 'streaming';

const BUTTONS: Record<number, MouseButton> = { 0: 'left', 1: 'middle', 2: 'right' };
const PIXELS_PER_NOTCH = 100;
const MAX_NOTCHES = 10;

export class Viewer {
  private readonly context: CanvasRenderingContext2D;
  private socket: WebSocket | null = null;
  private session: Session | null = null;
  private handshake: Handshake | null = null;
  private phase: Phase = 'handshake';
  private identity: Identity | undefined;
  private pairing = false;
  private agentToPin: Uint8Array | null = null;
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

  async start(): Promise<void> {
    this.report({ kind: 'loading' });
    try {
      await loadCrypto();
      this.identity = await loadIdentity().catch(() => undefined);
    } catch {
      this.report({ kind: 'refused', reason: 'failed', retryAfter: 0 });
      return;
    }
    const pinned = this.identity?.agents[location.origin];
    if (this.identity !== undefined && pinned !== undefined) {
      const key = this.identity.deviceKey;
      this.open(() => Handshake.reconnect(key, pinned), false);
    } else {
      this.report({ kind: 'needsPairing' });
    }
  }

  async pair(code: string, name: string): Promise<void> {
    const deviceKey = this.identity?.deviceKey ?? generateDeviceKey();
    if (this.identity === undefined) {
      this.identity = { deviceKey, agents: {} };
      await saveIdentity(this.identity).catch(() => undefined);
    }
    this.open(() => Handshake.pairing(deviceKey, code, name), true);
  }

  submitCode(code: string): void {
    if (this.phase !== 'authenticating') {
      return;
    }
    let payload: Uint8Array;
    try {
      payload = encodeTotp(code);
    } catch {
      this.report({ kind: 'needsCode', error: 'invalid' });
      return;
    }
    this.send(CONTROL_CHANNEL, payload);
  }

  async forget(): Promise<void> {
    this.close();
    await forgetIdentity().catch(() => undefined);
    this.identity = undefined;
    this.report({ kind: 'needsPairing' });
  }

  async reconnect(): Promise<void> {
    this.close();
    await this.start();
  }

  private open(make: () => Handshake, pairing: boolean): void {
    this.close();
    let handshake: Handshake;
    try {
      handshake = make();
    } catch {
      this.report(pairing ? { kind: 'needsPairing', error: 'invalid' } : { kind: 'refused', reason: 'failed', retryAfter: 0 });
      return;
    }
    this.report({ kind: 'connecting' });
    this.handshake = handshake;
    this.pairing = pairing;
    this.phase = 'handshake';
    const scheme = location.protocol === 'https:' ? 'wss' : 'ws';
    const socket = new WebSocket(`${scheme}://${location.host}/ws`);
    socket.binaryType = 'arraybuffer';
    socket.addEventListener('open', () => socket.send(owned(handshake.firstMessage)));
    socket.addEventListener('message', (event) => this.receive(socket, event.data));
    socket.addEventListener('close', () => this.closed(socket));
    this.socket = socket;
  }

  private close(): void {
    const socket = this.socket;
    this.socket = null;
    socket?.close();
    this.session?.free();
    this.session = null;
    this.handshake?.free();
    this.handshake = null;
    this.agentToPin = null;
    this.releaseAll();
  }

  private closed(socket: WebSocket): void {
    if (this.socket !== socket) {
      return;
    }
    const phase = this.phase;
    this.close();
    this.report(
      phase === 'streaming' ? { kind: 'closed' } : { kind: 'refused', reason: 'unknown', retryAfter: 0 },
    );
  }

  private fail(socket: WebSocket): void {
    if (this.socket === socket) {
      this.close();
      this.report({ kind: 'refused', reason: 'failed', retryAfter: 0 });
    }
  }

  private receive(socket: WebSocket, data: unknown): void {
    if (this.socket !== socket || !(data instanceof ArrayBuffer)) {
      return;
    }
    try {
      const bytes = new Uint8Array(data);
      if (this.session === null) {
        this.finishHandshake(socket, bytes);
        return;
      }
      const message = this.session.open(bytes);
      if (message !== undefined) {
        this.dispatch(socket, decodeFrame(message));
      }
    } catch {
      this.fail(socket);
    }
  }

  private finishHandshake(socket: WebSocket, reply: Uint8Array): void {
    const handshake = this.handshake;
    if (handshake === null) {
      return;
    }
    const outcome = handshake.complete(reply);
    const last = outcome.lastMessage;
    if (last !== undefined) {
      socket.send(owned(last));
    }
    const session = outcome.takeSession();
    outcome.free();
    this.handshake = null;
    this.session = session;
    this.agentToPin = this.pairing ? session.remotePublicKey : null;
    this.phase = 'authenticating';
  }

  private dispatch(socket: WebSocket, frame: { channel: number; payload: Uint8Array }): void {
    if (this.phase === 'authenticating' && frame.channel === CONTROL_CHANNEL) {
      this.control(socket, frame.payload);
    } else if (this.phase === 'streaming' && frame.channel === DESKTOP_CHANNEL) {
      this.apply(decodeDesktop(frame.payload));
    } else {
      this.fail(socket);
    }
  }

  private control(socket: WebSocket, payload: Uint8Array): void {
    const message = decodeControl(payload);
    switch (message.kind) {
      case 'authRequired':
        this.report({ kind: 'needsCode' });
        return;
      case 'authOk':
        this.phase = 'streaming';
        this.report({ kind: 'connecting' });
        void this.pinAgent();
        return;
      case 'authFailed':
        if (message.reason === 'wrongCode') {
          this.report({ kind: 'needsCode', error: 'wrong' });
          return;
        }
        this.close();
        this.report({ kind: 'refused', reason: message.reason, retryAfter: message.retryAfter });
        void socket;
    }
  }

  private async pinAgent(): Promise<void> {
    const agent = this.agentToPin;
    const identity = this.identity;
    this.agentToPin = null;
    if (agent === null || identity === undefined) {
      return;
    }
    identity.agents[location.origin] = agent;
    await saveIdentity(identity).catch(() => undefined);
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
    this.send(DESKTOP_CHANNEL, encodeFrameAck(sequence));
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

  private send(channel: number, payload: Uint8Array): void {
    const socket = this.socket;
    const session = this.session;
    if (socket?.readyState !== WebSocket.OPEN || session === null) {
      return;
    }
    const bytes = encodeFrame(channel, 0, payload);
    for (const record of session.seal(bytes) as Uint8Array[]) {
      socket.send(owned(record));
    }
  }

  private sendInput(event: InputEvent): void {
    if (this.phase === 'streaming') {
      this.send(DESKTOP_CHANNEL, encodeInput(event));
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
          this.sendInput({ kind: 'pointer', ...this.pendingMove });
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
      this.sendInput({ kind: 'pointer', ...position });
    }
    if (pressed) {
      this.canvas.focus();
      this.canvas.setPointerCapture(event.pointerId);
      this.pressedButtons.add(button);
    } else {
      this.pressedButtons.delete(button);
    }
    this.sendInput({ kind: 'button', button, pressed });
  }

  private wheel(event: WheelEvent): void {
    event.preventDefault();
    const horizontal = notches(event.deltaX, event.deltaMode);
    const vertical = -notches(event.deltaY, event.deltaMode);
    if (horizontal !== 0 || vertical !== 0) {
      this.sendInput({ kind: 'wheel', horizontal, vertical });
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
    this.sendInput({ kind: 'key', code, pressed });
  }

  private releaseButtons(): void {
    for (const button of this.pressedButtons) {
      this.sendInput({ kind: 'button', button, pressed: false });
    }
    this.pressedButtons.clear();
  }

  private releaseAll(): void {
    this.releaseButtons();
    for (const code of this.pressedKeys) {
      this.sendInput({ kind: 'key', code, pressed: false });
    }
    this.pressedKeys.clear();
  }
}

function owned(bytes: Uint8Array): Uint8Array<ArrayBuffer> {
  const copy = new Uint8Array(bytes.length);
  copy.set(bytes);
  return copy;
}

function notches(delta: number, mode: number): number {
  if (delta === 0) {
    return 0;
  }
  const lines = mode === WheelEvent.DOM_DELTA_PIXEL ? delta / PIXELS_PER_NOTCH : delta;
  const count = Math.min(Math.max(Math.round(Math.abs(lines)), 1), MAX_NOTCHES);
  return Math.sign(delta) * count;
}
