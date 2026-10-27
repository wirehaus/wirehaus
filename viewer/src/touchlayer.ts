import { LONG_PRESS_MS, type TouchAction, TouchRecognizer } from './touch.js';
import type { Viewer } from './viewer.js';

export class TouchLayer {
  private readonly recognizer: TouchRecognizer;
  private timer: number | undefined;

  constructor(
    private readonly surface: HTMLElement,
    private readonly viewer: Viewer,
  ) {
    this.recognizer = new TouchRecognizer(() => viewer.isZoomed());
    surface.addEventListener('pointerdown', (event) => this.down(event));
    surface.addEventListener('pointermove', (event) => this.move(event));
    surface.addEventListener('pointerup', (event) => this.up(event));
    surface.addEventListener('pointercancel', () => this.cancel());
    surface.addEventListener('contextmenu', (event) => event.preventDefault());
  }

  private ignored(event: PointerEvent): boolean {
    return event.pointerType !== 'touch' || (event.target as Element).closest('.overlay') !== null;
  }

  private down(event: PointerEvent): void {
    if (this.ignored(event)) {
      return;
    }
    event.preventDefault();
    this.surface.setPointerCapture(event.pointerId);
    window.clearTimeout(this.timer);
    this.timer = window.setTimeout(() => {
      this.run(this.recognizer.longPress());
      navigator.vibrate?.(12);
    }, LONG_PRESS_MS);
    this.run(this.recognizer.down(event.pointerId, { x: event.clientX, y: event.clientY }, event.timeStamp));
  }

  private move(event: PointerEvent): void {
    if (this.ignored(event)) {
      return;
    }
    event.preventDefault();
    this.run(this.recognizer.move(event.pointerId, { x: event.clientX, y: event.clientY }));
  }

  private up(event: PointerEvent): void {
    if (this.ignored(event)) {
      return;
    }
    window.clearTimeout(this.timer);
    this.run(this.recognizer.up(event.pointerId, { x: event.clientX, y: event.clientY }, event.timeStamp));
  }

  private cancel(): void {
    window.clearTimeout(this.timer);
    this.run(this.recognizer.cancel());
  }

  private run(actions: TouchAction[]): void {
    for (const action of actions) {
      switch (action.kind) {
        case 'hover':
          this.viewer.touchHover(action.at.x, action.at.y);
          break;
        case 'tap':
          this.viewer.touchButton(action.button, true, action.at.x, action.at.y);
          this.viewer.touchButton(action.button, false, action.at.x, action.at.y);
          break;
        case 'press':
          this.viewer.touchButton('left', true, action.at.x, action.at.y);
          break;
        case 'release':
          this.viewer.touchButton('left', false, action.at.x, action.at.y);
          break;
        case 'scroll':
          this.viewer.touchScroll(action.horizontal, action.vertical);
          break;
        case 'zoom':
          this.viewer.zoomBy(action.factor, action.at.x, action.at.y);
          break;
        case 'pan':
          this.viewer.panBy(action.dx, action.dy);
          break;
      }
    }
  }
}
