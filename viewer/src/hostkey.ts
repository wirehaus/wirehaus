export type HostKeyResult = 'pass' | 'swallow' | 'exit' | 'modifier-then-pass';

const HOST_KEY = 'ControlRight';

export class HostKey {
  private state: 'idle' | 'held' | 'passed' = 'idle';

  reset(): void {
    this.state = 'idle';
  }

  feed(code: string, pressed: boolean): HostKeyResult {
    if (code === HOST_KEY) {
      if (pressed) {
        if (this.state === 'idle') {
          this.state = 'held';
        }
        return this.state === 'held' ? 'swallow' : 'pass';
      }
      const wasHeld = this.state === 'held';
      this.state = 'idle';
      return wasHeld ? 'exit' : 'pass';
    }
    if (pressed && this.state === 'held') {
      this.state = 'passed';
      return 'modifier-then-pass';
    }
    return 'pass';
  }
}
