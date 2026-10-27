import { evdevCode } from './keymap.js';
import { type KeyChange, type Layout, keysForCharacter } from './softkeys.js';

export type KeyTarget = 'desktop' | 'terminal';

export type KeyBarIo = {
  target: () => KeyTarget;
  keys: (changes: readonly KeyChange[]) => void;
  terminal: (text: string) => void;
  focusTerminal: () => void;
  layout: () => Layout;
  unsupported: () => void;
};

type Modifier = 'Control' | 'Alt' | 'Shift' | 'Meta';

const MODIFIER_CODES: Record<Modifier, string> = {
  Control: 'ControlLeft',
  Alt: 'AltLeft',
  Shift: 'ShiftLeft',
  Meta: 'MetaLeft',
};

const TERMINAL_KEYS: Record<string, string> = {
  Escape: '\u001b',
  Tab: '\t',
  Enter: '\r',
  Backspace: '\u007f',
  ArrowUp: '\u001b[A',
  ArrowDown: '\u001b[B',
  ArrowRight: '\u001b[C',
  ArrowLeft: '\u001b[D',
  PageUp: '\u001b[5~',
  PageDown: '\u001b[6~',
  Delete: '\u001b[3~',
};

export class KeyBar {
  private readonly held = new Set<Modifier>();
  private readonly toggles: HTMLButtonElement[];

  constructor(
    root: HTMLElement,
    private readonly input: HTMLTextAreaElement,
    keyboard: HTMLButtonElement,
    private readonly io: KeyBarIo,
  ) {
    this.toggles = [...root.querySelectorAll<HTMLButtonElement>('button[data-mod]')];
    for (const button of root.querySelectorAll<HTMLButtonElement>('button[data-key]')) {
      button.addEventListener('pointerdown', (event) => event.preventDefault());
      button.addEventListener('click', () => this.tapNamed(button.dataset['key'] ?? ''));
    }
    for (const button of this.toggles) {
      button.addEventListener('pointerdown', (event) => event.preventDefault());
      button.addEventListener('click', () => this.toggle(button.dataset['mod'] as Modifier));
    }
    keyboard.addEventListener('click', () => this.openKeyboard());
    input.addEventListener('beforeinput', (event) => this.beforeInput(event));
    input.addEventListener('input', (event) => this.flush(event as InputEvent));
    input.addEventListener('compositionend', () => this.drain());
    input.addEventListener('keydown', (event) => {
      if (event.key === 'Enter') {
        event.preventDefault();
        this.tapNamed('Enter');
      }
    });
  }

  reset(): void {
    this.held.clear();
    this.paint();
    this.input.blur();
  }

  private paint(): void {
    for (const button of this.toggles) {
      button.setAttribute('aria-pressed', String(this.held.has(button.dataset['mod'] as Modifier)));
    }
  }

  private toggle(modifier: Modifier): void {
    if (this.held.has(modifier)) {
      this.held.delete(modifier);
    } else {
      this.held.add(modifier);
    }
    this.paint();
  }

  private openKeyboard(): void {
    if (this.io.target() === 'terminal') {
      this.io.focusTerminal();
      return;
    }
    this.input.value = '';
    this.input.focus();
  }

  private beforeInput(event: InputEvent): void {
    if (event.inputType === 'deleteContentBackward') {
      event.preventDefault();
      this.tapNamed('Backspace');
    } else if (event.inputType === 'insertLineBreak' || event.inputType === 'insertParagraph') {
      event.preventDefault();
      this.tapNamed('Enter');
    }
  }

  private flush(event: InputEvent): void {
    if (event.isComposing) {
      return;
    }
    this.drain();
  }

  private drain(): void {
    const text = this.input.value;
    this.input.value = '';
    for (const character of text) {
      this.typeCharacter(character);
    }
  }

  private release(): KeyChange[] {
    const changes: KeyChange[] = [];
    for (const modifier of [...this.held].reverse()) {
      const code = evdevCode(MODIFIER_CODES[modifier]);
      if (code !== undefined) {
        changes.push({ code, pressed: false });
      }
    }
    this.held.clear();
    this.paint();
    return changes;
  }

  private pressHeld(): KeyChange[] {
    const changes: KeyChange[] = [];
    for (const modifier of this.held) {
      const code = evdevCode(MODIFIER_CODES[modifier]);
      if (code !== undefined) {
        changes.push({ code, pressed: true });
      }
    }
    return changes;
  }

  private tapNamed(name: string): void {
    if (this.io.target() === 'terminal') {
      this.terminalKey(TERMINAL_KEYS[name] ?? '');
      return;
    }
    const code = evdevCode(name);
    if (code === undefined) {
      return;
    }
    this.io.keys([...this.pressHeld(), { code, pressed: true }, { code, pressed: false }, ...this.release()]);
  }

  private typeCharacter(character: string): void {
    if (this.io.target() === 'terminal') {
      this.terminalKey(character);
      return;
    }
    const sequence = keysForCharacter(character, this.io.layout());
    if (sequence === undefined) {
      this.io.unsupported();
      return;
    }
    this.io.keys([...this.pressHeld(), ...sequence, ...this.release()]);
  }

  private terminalKey(text: string): void {
    if (text === '') {
      return;
    }
    let out = text;
    if (this.held.has('Control') && text.length === 1) {
      const code = text.toUpperCase().charCodeAt(0);
      if (code >= 64 && code <= 95) {
        out = String.fromCharCode(code - 64);
      }
    }
    if (this.held.has('Alt')) {
      out = `\u001b${out}`;
    }
    this.held.clear();
    this.paint();
    this.io.terminal(out);
  }
}
