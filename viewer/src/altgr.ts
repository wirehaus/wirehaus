export interface KeyChange {
  code: number;
  pressed: boolean;
  time: number;
}

const CONTROL_LEFT = 29;
const ALT_RIGHT = 100;

export const ALTGR_WINDOW_MS = 5;

export class AltGrFilter {
  private held: KeyChange | undefined;
  private swallowNextControlRelease = false;
  private altDown = false;
  private altReleasedAt = Number.NEGATIVE_INFINITY;

  feed(change: KeyChange): KeyChange[] {
    this.expire(change.time);
    if (change.code === CONTROL_LEFT && change.pressed) {
      const out = this.release();
      this.held = change;
      return out;
    }
    if (change.code === ALT_RIGHT && change.pressed) {
      return this.altGrPressed(change);
    }
    if (change.code === ALT_RIGHT) {
      this.altDown = false;
      this.altReleasedAt = change.time;
      return [...this.release(), change];
    }
    if (change.code === CONTROL_LEFT) {
      return this.controlReleased(change);
    }
    return [...this.release(), change];
  }

  flush(now: number): KeyChange[] {
    if (this.held !== undefined && now - this.held.time >= ALTGR_WINDOW_MS) {
      return this.release();
    }
    return [];
  }

  pending(): boolean {
    return this.held !== undefined;
  }

  private altGrPressed(change: KeyChange): KeyChange[] {
    this.altDown = true;
    if (this.held !== undefined && change.time - this.held.time <= ALTGR_WINDOW_MS) {
      this.held = undefined;
      this.swallowNextControlRelease = true;
      return [change];
    }
    return [...this.release(), change];
  }

  private controlReleased(change: KeyChange): KeyChange[] {
    if (this.swallowNextControlRelease) {
      this.swallowNextControlRelease = false;
      return [];
    }
    return [...this.release(), change];
  }

  private release(): KeyChange[] {
    const held = this.held;
    this.held = undefined;
    return held === undefined ? [] : [held];
  }

  private expire(now: number): void {
    if (
      this.swallowNextControlRelease &&
      !this.altDown &&
      now - this.altReleasedAt > ALTGR_WINDOW_MS
    ) {
      this.swallowNextControlRelease = false;
    }
  }
}
