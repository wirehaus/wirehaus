export type Point = { x: number; y: number };

export type TouchAction =
  | { kind: 'hover'; at: Point }
  | { kind: 'tap'; button: 'left' | 'right'; at: Point }
  | { kind: 'press'; at: Point }
  | { kind: 'release'; at: Point }
  | { kind: 'scroll'; horizontal: number; vertical: number }
  | { kind: 'zoom'; factor: number; at: Point }
  | { kind: 'pan'; dx: number; dy: number };

export const LONG_PRESS_MS = 450;
export const TAP_MAX_MS = 350;
const SLOP = 10;
const PINCH_SLOP = 16;
const SCROLL_STEP = 36;

type Mode = 'idle' | 'one' | 'hold' | 'two' | 'ignore';
type Pair = { centroid: Point; distance: number };

function pairOf(points: Point[]): Pair {
  const [a, b] = points as [Point, Point];
  return {
    centroid: { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 },
    distance: Math.hypot(a.x - b.x, a.y - b.y),
  };
}

function away(from: Point, to: Point, limit: number): boolean {
  return Math.hypot(to.x - from.x, to.y - from.y) > limit;
}

export class TouchRecognizer {
  private readonly points = new Map<number, Point>();
  private mode: Mode = 'idle';
  private origin: Point = { x: 0, y: 0 };
  private began = 0;
  private moved = false;
  private first: Pair = { centroid: { x: 0, y: 0 }, distance: 0 };
  private last: Pair = { centroid: { x: 0, y: 0 }, distance: 0 };
  private twoKind: 'undecided' | 'pinch' | 'drag' = 'undecided';
  private twoMoved = false;
  private accumulated: Point = { x: 0, y: 0 };

  constructor(private readonly zoomed: () => boolean) {}

  down(id: number, at: Point, time: number): TouchAction[] {
    this.points.set(id, at);
    if (this.mode === 'idle') {
      this.mode = 'one';
      this.origin = at;
      this.began = time;
      this.moved = false;
      return [];
    }
    if (this.mode === 'one' && this.points.size === 2) {
      this.mode = 'two';
      this.first = pairOf([...this.points.values()]);
      this.last = this.first;
      this.began = time;
      this.twoKind = 'undecided';
      this.twoMoved = false;
      this.accumulated = { x: 0, y: 0 };
      return [];
    }
    if (this.mode !== 'hold') {
      this.mode = 'ignore';
    }
    return [];
  }

  longPress(): TouchAction[] {
    if (this.mode === 'one' && !this.moved) {
      this.mode = 'hold';
      return [{ kind: 'press', at: this.origin }];
    }
    return [];
  }

  move(id: number, at: Point): TouchAction[] {
    if (!this.points.has(id)) {
      return [];
    }
    this.points.set(id, at);
    if (this.mode === 'one') {
      if (!this.moved && away(this.origin, at, SLOP)) {
        this.moved = true;
      }
      return this.moved ? [{ kind: 'hover', at }] : [];
    }
    if (this.mode === 'hold') {
      return [{ kind: 'hover', at }];
    }
    if (this.mode === 'two' && this.points.size === 2) {
      return this.moveTwo();
    }
    return [];
  }

  private moveTwo(): TouchAction[] {
    const now = pairOf([...this.points.values()]);
    const shift = { x: now.centroid.x - this.last.centroid.x, y: now.centroid.y - this.last.centroid.y };
    const stretch = now.distance - this.first.distance;
    const travelled = Math.hypot(now.centroid.x - this.first.centroid.x, now.centroid.y - this.first.centroid.y);
    if (this.twoKind === 'undecided') {
      if (Math.abs(stretch) > PINCH_SLOP && Math.abs(stretch) > travelled) {
        this.twoKind = 'pinch';
      } else if (travelled > SLOP) {
        this.twoKind = 'drag';
      }
    }
    if (this.twoKind !== 'undecided') {
      this.twoMoved = true;
    }
    const factor = this.last.distance > 0 ? now.distance / this.last.distance : 1;
    this.last = now;
    if (this.twoKind === 'pinch') {
      return [
        { kind: 'zoom', factor, at: now.centroid },
        { kind: 'pan', dx: shift.x, dy: shift.y },
      ];
    }
    if (this.twoKind !== 'drag') {
      return [];
    }
    if (this.zoomed()) {
      return [{ kind: 'pan', dx: shift.x, dy: shift.y }];
    }
    this.accumulated = { x: this.accumulated.x + shift.x, y: this.accumulated.y + shift.y };
    const stepsX = Math.trunc(this.accumulated.x / SCROLL_STEP);
    const stepsY = Math.trunc(this.accumulated.y / SCROLL_STEP);
    if (stepsX === 0 && stepsY === 0) {
      return [];
    }
    this.accumulated = {
      x: this.accumulated.x - stepsX * SCROLL_STEP,
      y: this.accumulated.y - stepsY * SCROLL_STEP,
    };
    const horizontal = -stepsX;
    const vertical = stepsY;
    return [{ kind: 'scroll', horizontal, vertical }];
  }

  up(id: number, at: Point, time: number): TouchAction[] {
    if (!this.points.has(id)) {
      return [];
    }
    const before = this.mode;
    const centroid = this.points.size === 2 ? pairOf([...this.points.values()]).centroid : at;
    this.points.delete(id);
    const actions: TouchAction[] = [];
    if (before === 'one') {
      if (!this.moved && time - this.began <= TAP_MAX_MS * 3) {
        actions.push({ kind: 'hover', at }, { kind: 'tap', button: 'left', at });
      }
    } else if (before === 'hold') {
      actions.push({ kind: 'release', at });
    } else if (before === 'two') {
      if (!this.twoMoved && time - this.began <= TAP_MAX_MS) {
        actions.push({ kind: 'hover', at: centroid }, { kind: 'tap', button: 'right', at: centroid });
      }
      this.mode = 'ignore';
    }
    if (this.points.size === 0) {
      this.mode = 'idle';
    } else if (before !== 'two') {
      this.mode = 'ignore';
    }
    return actions;
  }

  cancel(): TouchAction[] {
    const held = this.mode === 'hold';
    const at = this.origin;
    this.points.clear();
    this.mode = 'idle';
    return held ? [{ kind: 'release', at }] : [];
  }
}
