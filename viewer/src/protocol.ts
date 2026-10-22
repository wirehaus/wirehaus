export const HEADER_LEN = 9;
export const MAX_PAYLOAD_LEN = 1 << 20;
export const DESKTOP_CHANNEL = 0;
export const CONTROL_CHANNEL = 6;
export const CODE_LEN = 6;

const RESIZE = 0;
const TILE = 1;
const FRAME_END = 2;
const TILE_HEADER_LEN = 9;

export const CODEC_JPEG = 0;

export type Frame = {
  channel: number;
  stream: number;
  payload: Uint8Array;
};

export type DesktopMessage =
  | { kind: 'resize'; width: number; height: number }
  | {
      kind: 'tile';
      x: number;
      y: number;
      width: number;
      height: number;
      codec: number;
      data: Uint8Array;
    }
  | { kind: 'frameEnd'; sequence: number };

export type MouseButton = 'left' | 'middle' | 'right';

export type InputEvent =
  | { kind: 'pointer'; x: number; y: number }
  | { kind: 'button'; button: MouseButton; pressed: boolean }
  | { kind: 'wheel'; horizontal: number; vertical: number }
  | { kind: 'key'; code: number; pressed: boolean };

export type AuthFailure = 'wrongCode' | 'locked' | 'suspended' | 'busy';

export type ControlMessage =
  | { kind: 'authRequired' }
  | { kind: 'authOk' }
  | { kind: 'authFailed'; reason: AuthFailure; retryAfter: number };

export class ProtocolError extends Error {}

export function encodeFrame(
  channel: number,
  stream: number,
  payload: Uint8Array,
): Uint8Array<ArrayBuffer> {
  if (payload.length > MAX_PAYLOAD_LEN) {
    throw new ProtocolError('payload too large');
  }
  const out = new Uint8Array(HEADER_LEN + payload.length);
  const view = new DataView(out.buffer);
  view.setUint8(0, channel);
  view.setUint32(1, stream);
  view.setUint32(5, payload.length);
  out.set(payload, HEADER_LEN);
  return out;
}

export function decodeFrame(bytes: Uint8Array): Frame {
  if (bytes.length < HEADER_LEN) {
    throw new ProtocolError('frame shorter than its header');
  }
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const declared = view.getUint32(5);
  if (declared > MAX_PAYLOAD_LEN) {
    throw new ProtocolError('payload too large');
  }
  if (bytes.length - HEADER_LEN !== declared) {
    throw new ProtocolError('payload length does not match the header');
  }
  return {
    channel: view.getUint8(0),
    stream: view.getUint32(1),
    payload: bytes.subarray(HEADER_LEN),
  };
}

export function decodeDesktop(payload: Uint8Array): DesktopMessage {
  const tag = payload[0];
  if (tag === undefined) {
    throw new ProtocolError('empty desktop message');
  }
  const view = new DataView(payload.buffer, payload.byteOffset, payload.byteLength);
  if (tag === RESIZE) {
    if (payload.length !== 5) {
      throw new ProtocolError('bad resize length');
    }
    return { kind: 'resize', width: view.getUint16(1), height: view.getUint16(3) };
  }
  if (tag === TILE) {
    if (payload.length < 1 + TILE_HEADER_LEN) {
      throw new ProtocolError('bad tile length');
    }
    return {
      kind: 'tile',
      x: view.getUint16(1),
      y: view.getUint16(3),
      width: view.getUint16(5),
      height: view.getUint16(7),
      codec: view.getUint8(9),
      data: payload.subarray(1 + TILE_HEADER_LEN),
    };
  }
  if (tag === FRAME_END) {
    if (payload.length !== 5) {
      throw new ProtocolError('bad frame end length');
    }
    return { kind: 'frameEnd', sequence: view.getUint32(1) };
  }
  throw new ProtocolError(`unknown desktop message ${tag}`);
}

export function encodeInput(event: InputEvent): Uint8Array {
  switch (event.kind) {
    case 'pointer': {
      const out = new Uint8Array(5);
      const view = new DataView(out.buffer);
      view.setUint8(0, 0);
      view.setUint16(1, event.x);
      view.setUint16(3, event.y);
      return out;
    }
    case 'button':
      return Uint8Array.of(1, buttonTag(event.button), event.pressed ? 1 : 0);
    case 'wheel': {
      const out = new Uint8Array(5);
      const view = new DataView(out.buffer);
      view.setUint8(0, 2);
      view.setInt16(1, event.horizontal);
      view.setInt16(3, event.vertical);
      return out;
    }
    case 'key': {
      const out = new Uint8Array(4);
      const view = new DataView(out.buffer);
      view.setUint8(0, 3);
      view.setUint16(1, event.code);
      view.setUint8(3, event.pressed ? 1 : 0);
      return out;
    }
  }
}

export function encodeFrameAck(sequence: number): Uint8Array {
  const out = new Uint8Array(5);
  const view = new DataView(out.buffer);
  view.setUint8(0, 4);
  view.setUint32(1, sequence);
  return out;
}

function buttonTag(button: MouseButton): number {
  switch (button) {
    case 'left':
      return 0;
    case 'middle':
      return 1;
    case 'right':
      return 2;
  }
}

const FAILURES: AuthFailure[] = ['wrongCode', 'locked', 'suspended', 'busy'];

export function decodeControl(payload: Uint8Array): ControlMessage {
  const tag = payload[0];
  if (tag === undefined) {
    throw new ProtocolError('empty control message');
  }
  const view = new DataView(payload.buffer, payload.byteOffset, payload.byteLength);
  switch (tag) {
    case 0:
      expectLength(payload, 1);
      return { kind: 'authRequired' };
    case 2:
      expectLength(payload, 1);
      return { kind: 'authOk' };
    case 3: {
      expectLength(payload, 6);
      const reason = FAILURES[view.getUint8(1)];
      if (reason === undefined) {
        throw new ProtocolError('unknown failure reason');
      }
      return { kind: 'authFailed', reason, retryAfter: view.getUint32(2) };
    }
    default:
      throw new ProtocolError(`unknown control message ${tag}`);
  }
}

export function encodeTotp(code: string): Uint8Array {
  if (!/^[0-9]{6}$/.test(code)) {
    throw new ProtocolError('the code must be six digits');
  }
  const out = new Uint8Array(1 + CODE_LEN);
  out[0] = 1;
  for (let index = 0; index < CODE_LEN; index += 1) {
    out[1 + index] = code.charCodeAt(index);
  }
  return out;
}

function expectLength(payload: Uint8Array, length: number): void {
  if (payload.length !== length) {
    throw new ProtocolError('bad control message length');
  }
}
