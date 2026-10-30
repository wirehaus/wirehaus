import { ALTGR_WINDOW_MS, AltGrFilter, type KeyChange } from './altgr.js';
import { AUDIO_CHANNEL, type AudioMessage, decodeAudio } from './audioprotocol.js';
import { agentBase } from './linkaddr.js';
import { probeLinkPatiently } from './linkflow.js';
import { LINK_CHANNEL, type LinkReply, decodeLink } from './linkprotocol.js';
import { CLIPBOARD_CHANNEL, type ClipboardMessage, decodeClipboard } from './clipboardprotocol.js';
import { FILES_CHANNEL, type FilesMessage, decodeFiles } from './filesprotocol.js';
import { HostKey } from './hostkey.js';
import { evdevCode } from './keymap.js';
import {
  CODEC_JPEG,
  CONTROL_CHANNEL,
  DESKTOP_CHANNEL,
  TERMINAL_CHANNEL,
  type TerminalMessage,
  type AuthFailure,
  type DesktopMessage,
  type InputEvent,
  type MouseButton,
  decodeControl,
  encodeResume,
  type EndReason,
  ProtocolError,
  decodeDesktop,
  decodeFrame,
  decodeTerminal,
  encodeCapabilities,
  encodeFrame,
  encodeFrameAck,
  encodeInput,
  encodeMonitorsAware,
  encodeSelectMonitor,
  encodeTotp,
  type MonitorInfo,
} from './protocol.js';
import { Handshake, type Session, generateDeviceKey, loadCrypto } from './secure.js';
import { type Identity, forgetIdentity, loadIdentity, saveIdentity } from './storage.js';
import { loadTicket, saveTicket } from './ticketstore.js';
import type { Target, TargetError } from './target.js';
import { VideoPlayer, videoSupported } from './video.js';
import { ensureAwake } from './wake.js';

export type AudioListener = (message: AudioMessage) => void;
export type ClipboardListener = (message: ClipboardMessage) => void;

export type FilesListener = (stream: number, message: FilesMessage) => void;

export type TerminalListener = (stream: number, message: TerminalMessage) => void;

export type RefusalReason = Exclude<AuthFailure, 'wrongCode'> | 'unknown' | 'failed';

export type ViewerState =
  | { kind: 'loading' }
  | { kind: 'noAgent'; error: TargetError }
  | { kind: 'needsPairing'; error?: 'invalid' }
  | { kind: 'connecting'; waking?: boolean }
  | { kind: 'needsCode'; error?: 'wrong' | 'invalid' }
  | { kind: 'connected'; width: number; height: number }
  | { kind: 'refused'; reason: RefusalReason; retryAfter: number }
  | { kind: 'reconnecting'; attempt: number }
  | { kind: 'ended'; reason: EndReason }
  | { kind: 'closed' };

type Phase = 'handshake' | 'authenticating' | 'streaming';

const BUTTONS: Record<number, MouseButton> = { 0: 'left', 1: 'middle', 2: 'right' };
const RETRY_DELAYS_MS = [1000, 2000, 4000, 8000, 15000];
const HANDSHAKE_WATCHDOG_MS = 20_000;
const HANDSHAKE_RETRIES = 4;
const AGENT_REFUSALS = 5;
const RESUME_RETRIES = 3;
const WAKE_RETRY_WINDOW_MS = 120_000;
const FRESH_TUNNEL_MS = 20 * 60_000;
const WAKE_RETRY_DELAY_MS = 1500;
const RESUME_RETRY_MS = 800;
const PIXELS_PER_NOTCH = 100;
const MAX_NOTCHES = 10;
const MAX_ZOOM = 5;

export class Viewer {
  private readonly context: CanvasRenderingContext2D;
  private readonly player: VideoPlayer;
  private socket: WebSocket | null = null;
  private session: Session | null = null;
  private handshake: Handshake | null = null;
  private phase: Phase = 'handshake';
  private identity: Identity | undefined;
  private pairing = false;
  private agentToPin: Uint8Array | null = null;
  private drawing: Promise<void> = Promise.resolve();
  private readonly pressedKeys = new Set<number>();
  private readonly altGr = new AltGrFilter();
  private readonly hostKey = new HostKey();
  hostKeyEnabled = false;
  onHostKey: (() => void) | null = null;
  private readonly pressedButtons = new Set<MouseButton>();
  private pendingMove: { x: number; y: number } | null = null;
  private zoom = 1;
  private panX = 0;
  private panY = 0;
  zoomListener: ((zoomed: boolean) => void) | null = null;
  desktopEnabled = true;
  private ticketValue: Uint8Array | null = null;
  private resumeSent = false;
  private resumeRetries = 0;
  private handshakeRetries = 0;
  private agentRefusals = 0;
  private videoRetries: number[] = [];
  private retrying = false;
  private retries = 0;
  private retryTimer: number | undefined;
  private endedBy: EndReason | null = null;
  terminalListener: TerminalListener | null = null;
  filesListener: FilesListener | null = null;
  clipboardListener: ClipboardListener | null = null;
  audioListener: AudioListener | null = null;
  monitorsListener: ((active: number, items: MonitorInfo[]) => void) | null = null;
  linkListener: ((reply: LinkReply) => void) | null = null;
  forcePairing = false;
  private wakeUntil = 0;
  private epoch = 0;
  private skippedCheck = false;
  private opened = false;
  private lastOpen: { make: () => Handshake; pairing: boolean } | null = null;

  constructor(
    private readonly canvas: HTMLCanvasElement,
    private readonly report: (state: ViewerState) => void,
    private readonly target: Target | TargetError,
    private readonly api = '',
  ) {
    const context = canvas.getContext('2d', { alpha: false });
    if (context === null) {
      throw new Error('canvas 2d context is not available');
    }
    this.context = context;
    this.player = new VideoPlayer(
      (frame) => this.context.drawImage(frame, 0, 0),
      () => this.videoFailed(),
    );
    this.listen();
    new ResizeObserver(() => this.fit()).observe(canvas.parentElement ?? canvas);
  }

  recycle(): void {
    if (this.phase !== 'streaming' || this.ticketValue === null || this.socket === null) {
      this.retryNow();
      return;
    }
    this.close();
    if (!this.scheduleRetry()) {
      this.report({ kind: 'refused', reason: 'failed', retryAfter: 0 });
    }
  }

  selectMonitor(index: number): void {
    this.send(DESKTOP_CHANNEL, encodeSelectMonitor(index));
  }

  retryNow(): void {
    if (this.retryTimer !== undefined && this.retrying) {
      window.clearTimeout(this.retryTimer);
      this.retryTimer = undefined;
      void this.start();
    }
  }

  releaseHostKey(): void {
    this.hostKey.reset();
  }

  private setTicket(ticket: Uint8Array | null): void {
    this.ticketValue = ticket;
    if (ticket === null) {
      this.resumeRetries = 0;
    }
    if (typeof this.target !== 'string') {
      saveTicket(this.target.origin, ticket);
    }
  }

  private restoreTicket(): void {
    if (this.ticketValue !== null || typeof this.target === 'string') {
      return;
    }
    const restored = loadTicket(this.target.origin);
    if (restored !== null) {
      this.ticketValue = restored;
      this.retrying = true;
    }
  }

  async start(): Promise<void> {
    const epoch = ++this.epoch;
    if (typeof this.target === 'string') {
      this.report({ kind: 'noAgent', error: this.target });
      return;
    }
    this.report({ kind: 'loading' });
    try {
      await loadCrypto();
      this.identity = await loadIdentity().catch(() => undefined);
    } catch {
      this.report({ kind: 'refused', reason: 'failed', retryAfter: 0 });
      return;
    }
    const pinned = this.identity?.agents[this.target.origin];
    const forced = this.forcePairing;
    this.forcePairing = false;
    if (!forced && this.identity !== undefined && pinned !== undefined) {
      const key = this.identity.deviceKey;
      this.restoreTicket();
      await this.wake();
      if (epoch !== this.epoch) {
        return;
      }
      this.open(() => Handshake.reconnect(key, pinned), false);
    } else {
      this.report({ kind: 'needsPairing' });
    }
  }

  private recentlyConnected(authority: string): boolean {
    try {
      const at = Number(localStorage.getItem(`wh.last.${authority}`));
      return at > 0 && Date.now() - at < FRESH_TUNNEL_MS;
    } catch {
      return false;
    }
  }

  private rememberConnected(): void {
    const target = this.target;
    if (typeof target === 'string') {
      return;
    }
    try {
      localStorage.setItem(`wh.last.${target.authority}`, String(Date.now()));
    } catch {
      return;
    }
  }

  private async wake(force = false): Promise<void> {
    const target = this.target;
    if (typeof target === 'string' || !target.foreign) {
      return;
    }
    if (!force && this.recentlyConnected(target.authority)) {
      this.skippedCheck = true;
      return;
    }
    this.skippedCheck = false;
    const waited = await ensureAwake(this.api, target.authority, () =>
      this.report({ kind: 'connecting', waking: true }),
    );
    if (waited) {
      this.wakeUntil = Date.now() + WAKE_RETRY_WINDOW_MS;
    }
  }

  async pair(code: string, name: string): Promise<void> {
    if (typeof this.target === 'string') {
      return;
    }
    const epoch = ++this.epoch;
    const deviceKey = this.identity?.deviceKey ?? generateDeviceKey();
    if (this.identity === undefined) {
      this.identity = { deviceKey, agents: {} };
      await saveIdentity(this.identity).catch(() => undefined);
    }
    await this.wake();
    if (epoch !== this.epoch) {
      return;
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
    this.epoch += 1;
    this.setTicket(null);
    this.stopRetrying();
    this.close();
    await forgetIdentity().catch(() => undefined);
    this.identity = undefined;
    this.report({ kind: 'needsPairing' });
  }

  async reconnect(): Promise<void> {
    this.stopRetrying();
    this.close();
    await this.start();
  }

  private open(make: () => Handshake, pairing: boolean): void {
    const target = this.target;
    if (typeof target === 'string') {
      return;
    }
    this.close();
    this.player.rearm();
    let handshake: Handshake;
    try {
      handshake = make();
    } catch {
      this.report(pairing ? { kind: 'needsPairing', error: 'invalid' } : { kind: 'refused', reason: 'failed', retryAfter: 0 });
      return;
    }
    this.report({ kind: 'connecting', waking: Date.now() < this.wakeUntil });
    this.lastOpen = { make, pairing };
    this.opened = false;
    this.handshake = handshake;
    this.pairing = pairing;
    this.resumeSent = false;
    this.endedBy = null;
    this.phase = 'handshake';
    const socket = new WebSocket(target.socketUrl);
    socket.binaryType = 'arraybuffer';
    window.setTimeout(() => {
      if (this.socket === socket && this.phase === 'handshake') {
        socket.close();
        this.closed(socket);
      }
    }, HANDSHAKE_WATCHDOG_MS);
    socket.addEventListener('open', () => {
      this.opened = true;
      socket.send(owned(handshake.firstMessage));
    });
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
    this.player.reset();
    this.releaseAll();
  }

  private videoFailed(): void {
    const socket = this.socket;
    if (socket === null) {
      return;
    }
    const now = Date.now();
    this.videoRetries = this.videoRetries.filter((time) => now - time < 120_000);
    if (this.ticketValue !== null && this.phase === 'streaming' && this.videoRetries.length < 3) {
      this.videoRetries.push(now);
      this.close();
      if (!this.scheduleRetry()) {
        this.report({ kind: 'refused', reason: 'failed', retryAfter: 0 });
      }
      return;
    }
    this.fail(socket);
  }

  private async announceMonitors(socket: WebSocket): Promise<void> {
    const target = this.target;
    const base = typeof target === 'string' ? undefined : agentBase(target.socketUrl);
    if (base === undefined) {
      return;
    }
    const level = await probeLinkPatiently(base, 2);
    if (level === 2 && this.socket === socket && this.phase === 'streaming') {
      this.send(DESKTOP_CHANNEL, encodeMonitorsAware());
    }
  }

  private async announceCapabilities(): Promise<void> {
    this.send(DESKTOP_CHANNEL, encodeCapabilities(this.desktopEnabled && (await videoSupported())));
  }

  private closed(socket: WebSocket): void {
    if (this.socket !== socket) {
      return;
    }
    const phase = this.phase;
    const endedBy = this.endedBy;
    if (phase === 'streaming') {
      this.rememberConnected();
    }
    const never = !this.opened && phase === 'handshake';
    const unreachable = never && Date.now() < this.wakeUntil;
    const recheck = never && !unreachable && this.skippedCheck && this.lastOpen !== null;
    this.close();
    if (recheck && this.lastOpen !== null) {
      const { make, pairing } = this.lastOpen;
      this.report({ kind: 'connecting', waking: true });
      const epoch = this.epoch;
      void this.wake(true).then(() => {
        if (epoch === this.epoch) {
          this.open(make, pairing);
        }
      });
      return;
    }
    if (unreachable && this.lastOpen !== null) {
      const { make, pairing } = this.lastOpen;
      this.report({ kind: 'connecting', waking: true });
      this.retryTimer = window.setTimeout(() => {
        this.retryTimer = undefined;
        this.open(make, pairing);
      }, WAKE_RETRY_DELAY_MS);
      return;
    }
    if (endedBy !== null) {
      if (endedBy !== 'idle') {
        this.setTicket(null);
      }
      this.stopRetrying();
      this.report({ kind: 'ended', reason: endedBy });
      return;
    }
    if (this.opened && phase !== 'streaming' && this.retrying && this.ticketValue !== null) {
      this.agentRefusals += 1;
      if (this.agentRefusals >= AGENT_REFUSALS) {
        this.agentRefusals = 0;
        this.setTicket(null);
        this.stopRetrying();
        this.report({ kind: 'refused', reason: 'unknown', retryAfter: 0 });
        return;
      }
    }
    if (this.ticketValue !== null && (phase === 'streaming' || this.retrying) && this.scheduleRetry()) {
      return;
    }
    if (phase === 'handshake' && this.lastOpen !== null && this.handshakeRetries < HANDSHAKE_RETRIES) {
      const { make, pairing } = this.lastOpen;
      this.handshakeRetries += 1;
      this.report({ kind: 'connecting' });
      this.retryTimer = window.setTimeout(() => {
        this.retryTimer = undefined;
        this.open(make, pairing);
      }, 1500 * this.handshakeRetries);
      return;
    }
    this.handshakeRetries = 0;
    this.setTicket(null);
    this.stopRetrying();
    this.report(
      phase === 'streaming' ? { kind: 'closed' } : { kind: 'refused', reason: 'unknown', retryAfter: 0 },
    );
  }

  private scheduleRetry(): boolean {
    const delay = RETRY_DELAYS_MS[Math.min(this.retries, RETRY_DELAYS_MS.length - 1)] ?? 15000;
    this.retrying = true;
    this.retries += 1;
    this.report({ kind: 'reconnecting', attempt: this.retries });
    this.retryTimer = window.setTimeout(() => {
      this.retryTimer = undefined;
      void this.start();
    }, delay);
    return true;
  }

  private stopRetrying(): void {
    window.clearTimeout(this.retryTimer);
    this.retryTimer = undefined;
    this.retrying = false;
    this.retries = 0;
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

  private dispatch(socket: WebSocket, frame: { channel: number; stream: number; payload: Uint8Array }): void {
    if (this.phase === 'authenticating' && frame.channel === CONTROL_CHANNEL) {
      this.control(socket, frame.payload);
    } else if (this.phase === 'streaming' && frame.channel === CONTROL_CHANNEL) {
      this.streamControl(frame.payload);
    } else if (this.phase === 'streaming' && frame.channel === DESKTOP_CHANNEL) {
      this.apply(decodeDesktop(frame.payload));
    } else if (this.phase === 'streaming' && frame.channel === TERMINAL_CHANNEL) {
      this.terminalListener?.(frame.stream, decodeTerminal(frame.payload));
    } else if (this.phase === 'streaming' && frame.channel === FILES_CHANNEL) {
      this.filesListener?.(frame.stream, decodeFiles(frame.payload));
    } else if (this.phase === 'streaming' && frame.channel === CLIPBOARD_CHANNEL) {
      this.clipboardListener?.(decodeClipboard(frame.payload));
    } else if (this.phase === 'streaming' && frame.channel === AUDIO_CHANNEL) {
      this.audioListener?.(decodeAudio(frame.payload));
    } else if (this.phase === 'streaming' && frame.channel === LINK_CHANNEL) {
      this.linkListener?.(decodeLink(frame.payload));
    } else {
      this.fail(socket);
    }
  }

  private control(socket: WebSocket, payload: Uint8Array): void {
    const message = decodeControl(payload);
    switch (message.kind) {
      case 'authRequired':
        if (this.ticketValue !== null && !this.resumeSent && !this.pairing) {
          this.resumeSent = true;
          this.send(CONTROL_CHANNEL, encodeResume(this.ticketValue));
          return;
        }
        if (this.ticketValue !== null && this.resumeSent && !this.pairing && this.resumeRetries < RESUME_RETRIES) {
          this.resumeRetries += 1;
          this.close();
          window.clearTimeout(this.retryTimer);
          this.retrying = true;
          this.report({ kind: 'reconnecting', attempt: this.resumeRetries });
          this.retryTimer = window.setTimeout(() => {
        this.retryTimer = undefined;
        void this.start();
      }, RESUME_RETRY_MS);
          return;
        }
        this.setTicket(null);
        this.stopRetrying();
        this.report({ kind: 'needsCode' });
        return;
      case 'authOk':
        this.resumeRetries = 0;
        this.handshakeRetries = 0;
        this.agentRefusals = 0;
        this.stopRetrying();
        this.phase = 'streaming';
        this.rememberConnected();
        this.report({ kind: 'connecting' });
        void this.pinAgent();
        void this.announceCapabilities();
        void this.announceMonitors(socket);
        return;
      case 'authFailed':
        if (message.reason === 'wrongCode') {
          this.report({ kind: 'needsCode', error: 'wrong' });
          return;
        }
        this.close();
        if (message.reason === 'busy' && this.retrying && this.scheduleRetry()) {
          return;
        }
        this.setTicket(null);
        this.stopRetrying();
        this.report({ kind: 'refused', reason: message.reason, retryAfter: message.retryAfter });
        return;
      case 'ticket':
      case 'sessionEnded':
        this.fail(socket);
    }
  }

  private streamControl(payload: Uint8Array): void {
    const message = decodeControl(payload);
    if (message.kind === 'ticket') {
      this.setTicket(message.ticket);
    } else if (message.kind === 'sessionEnded') {
      this.endedBy = message.reason;
      if (message.reason !== 'idle') {
        this.setTicket(null);
      }
    } else {
      throw new ProtocolError('unexpected control message');
    }
  }

  private async pinAgent(): Promise<void> {
    const agent = this.agentToPin;
    const identity = this.identity;
    const target = this.target;
    this.agentToPin = null;
    if (agent === null || identity === undefined || typeof target === 'string') {
      return;
    }
    identity.agents[target.origin] = agent;
    await saveIdentity(identity).catch(() => undefined);
  }

  private apply(message: DesktopMessage): void {
    if (message.kind === 'monitors') {
      this.monitorsListener?.(message.active, message.items);
      return;
    }
    if (message.kind === 'resize') {
      this.player.reset();
      this.canvas.width = message.width;
      this.canvas.height = message.height;
      this.fit();
      this.report({ kind: 'connected', width: message.width, height: message.height });
      if (this.desktopEnabled) {
        this.canvas.focus();
      }
      return;
    }
    if (!this.desktopEnabled) {
      return;
    }
    if (message.kind === 'video') {
      const { width, height, keyframe, data } = message;
      this.drawing = this.drawing
        .then(() => this.player.play(width, height, keyframe, data))
        .catch(() => undefined);
      return;
    }
    if (message.kind === 'frameEnd') {
      const sequence = message.sequence;
      this.drawing = this.drawing.then(() => this.acknowledge(sequence)).catch(() => undefined);
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
    const width = Math.floor(this.canvas.width * scale * this.zoom);
    const height = Math.floor(this.canvas.height * scale * this.zoom);
    this.canvas.style.width = `${width}px`;
    this.canvas.style.height = `${height}px`;
    const limitX = Math.max(0, (width - area.clientWidth) / 2);
    const limitY = Math.max(0, (height - area.clientHeight) / 2);
    this.panX = Math.min(Math.max(this.panX, -limitX), limitX);
    this.panY = Math.min(Math.max(this.panY, -limitY), limitY);
    this.canvas.style.transform = this.zoom === 1 ? '' : `translate(${this.panX}px, ${this.panY}px)`;
  }

  isZoomed(): boolean {
    return this.zoom > 1;
  }

  zoomBy(factor: number, clientX: number, clientY: number): void {
    const area = this.canvas.parentElement;
    if (area === null) {
      return;
    }
    const next = Math.min(Math.max(this.zoom * factor, 1), MAX_ZOOM);
    const box = area.getBoundingClientRect();
    const offsetX = clientX - (box.left + box.width / 2 + this.panX);
    const offsetY = clientY - (box.top + box.height / 2 + this.panY);
    const ratio = next / this.zoom;
    const was = this.zoom > 1;
    this.panX += offsetX * (1 - ratio);
    this.panY += offsetY * (1 - ratio);
    this.zoom = next;
    if (this.zoom === 1) {
      this.panX = 0;
      this.panY = 0;
    }
    this.fit();
    if (was !== this.zoom > 1) {
      this.zoomListener?.(this.zoom > 1);
    }
  }

  panBy(dx: number, dy: number): void {
    if (this.zoom === 1) {
      return;
    }
    this.panX += dx;
    this.panY += dy;
    this.fit();
  }

  resetZoom(): void {
    const was = this.zoom > 1;
    this.zoom = 1;
    this.panX = 0;
    this.panY = 0;
    this.fit();
    if (was) {
      this.zoomListener?.(false);
    }
  }

  touchHover(clientX: number, clientY: number): void {
    const position = this.position({ clientX, clientY });
    if (position !== null) {
      this.pendingMove = null;
      this.sendInput({ kind: 'pointer', ...position });
    }
  }

  touchButton(button: MouseButton, pressed: boolean, clientX: number, clientY: number): void {
    this.touchHover(clientX, clientY);
    if (pressed) {
      this.pressedButtons.add(button);
    } else {
      this.pressedButtons.delete(button);
    }
    this.sendInput({ kind: 'button', button, pressed });
  }

  touchScroll(horizontal: number, vertical: number): void {
    this.sendInput({ kind: 'wheel', horizontal, vertical });
  }

  sendKeyChanges(changes: readonly { code: number; pressed: boolean }[]): void {
    this.sendKeys(changes.map((change) => ({ ...change, time: 0 })));
  }

  sendTerminal(stream: number, payload: Uint8Array): void {
    this.send(TERMINAL_CHANNEL, payload, stream);
  }

  sendFiles(stream: number, payload: Uint8Array): void {
    this.send(FILES_CHANNEL, payload, stream);
  }

  sendAudio(payload: Uint8Array): void {
    this.send(AUDIO_CHANNEL, payload);
  }

  sendLink(payload: Uint8Array): void {
    this.send(LINK_CHANNEL, payload);
  }

  sendClipboard(stream: number, payload: Uint8Array): void {
    this.send(CLIPBOARD_CHANNEL, payload, stream);
  }

  bufferedBytes(): number {
    return this.socket?.bufferedAmount ?? 0;
  }

  private send(channel: number, payload: Uint8Array, stream = 0): void {
    const socket = this.socket;
    const session = this.session;
    if (socket?.readyState !== WebSocket.OPEN || session === null) {
      return;
    }
    const bytes = encodeFrame(channel, stream, payload);
    for (const record of session.seal(bytes) as Uint8Array[]) {
      socket.send(owned(record));
    }
  }

  private sendInput(event: InputEvent): void {
    if (this.phase === 'streaming') {
      this.send(DESKTOP_CHANNEL, encodeInput(event));
    }
  }

  private position(event: { clientX: number; clientY: number }): { x: number; y: number } | null {
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
    canvas.addEventListener('pointercancel', (event) => {
      if (event.pointerType !== 'touch') {
        this.releaseButtons();
      }
    });
    canvas.addEventListener('wheel', (event) => this.wheel(event), { passive: false });
    canvas.addEventListener('keydown', (event) => this.key(event, true));
    canvas.addEventListener('keyup', (event) => this.key(event, false));
    canvas.addEventListener('blur', () => this.releaseAll());
  }

  private queueMove(event: PointerEvent): void {
    if (event.pointerType === 'touch') {
      return;
    }
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
    if (event.pointerType === 'touch') {
      return;
    }
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
    if (this.hostKeyEnabled) {
      const verdict = this.hostKey.feed(event.code, pressed);
      if (verdict === 'swallow') {
        return;
      }
      if (verdict === 'exit') {
        this.releaseAll();
        this.onHostKey?.();
        return;
      }
      if (verdict === 'modifier-then-pass') {
        const modifier = evdevCode('ControlRight');
        if (modifier !== undefined) {
          this.sendKeys(this.altGr.feed({ code: modifier, pressed: true, time: event.timeStamp }));
        }
      }
    }
    this.sendKeys(this.altGr.feed({ code, pressed, time: event.timeStamp }));
    if (this.altGr.pending()) {
      window.setTimeout(() => this.sendKeys(this.altGr.flush(performance.now())), ALTGR_WINDOW_MS);
    }
  }

  private sendKeys(changes: readonly KeyChange[]): void {
    for (const { code, pressed } of changes) {
      if (pressed) {
        this.pressedKeys.add(code);
      } else {
        this.pressedKeys.delete(code);
      }
      this.sendInput({ kind: 'key', code, pressed });
    }
  }

  private releaseButtons(): void {
    for (const button of this.pressedButtons) {
      this.sendInput({ kind: 'button', button, pressed: false });
    }
    this.pressedButtons.clear();
  }

  private releaseAll(): void {
    this.hostKey.reset();
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
