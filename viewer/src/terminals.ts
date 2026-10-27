import { FitAddon } from '@xterm/addon-fit';
import { Terminal } from '@xterm/xterm';

import { type TerminalMessage, type TerminalRefusal, inputChunks } from './protocol.js';

export type TerminalIo = {
  open(stream: number, columns: number, rows: number): void;
  input(stream: number, data: Uint8Array): void;
  resize(stream: number, columns: number, rows: number): void;
  close(stream: number): void;
};

export type TerminalTexts = {
  exited: (code: number) => string;
  refused: (reason: TerminalRefusal) => string;
};

type Entry = {
  terminal: Terminal;
  fit: FitAddon;
  element: HTMLElement;
  exited: boolean;
};

export class Terminals {
  private readonly entries = new Map<number, Entry>();
  private readonly encoder = new TextEncoder();
  private next = 1;
  private active: number | null = null;

  constructor(
    private readonly host: HTMLElement,
    private readonly io: TerminalIo,
    private readonly texts: TerminalTexts,
    private readonly changed: () => void,
  ) {
    new ResizeObserver(() => this.fitActive()).observe(host);
  }

  streams(): number[] {
    return [...this.entries.keys()];
  }

  isExited(stream: number): boolean {
    return this.entries.get(stream)?.exited ?? true;
  }

  activeStream(): number | null {
    return this.active;
  }

  create(): number {
    const stream = this.next;
    this.next += 1;
    const element = document.createElement('div');
    element.className = 'terminal';
    element.hidden = true;
    this.host.append(element);
    const terminal = new Terminal({
      cursorBlink: true,
      fontSize: 14,
      fontFamily: 'ui-monospace, SFMono-Regular, Menlo, Consolas, monospace',
      theme: { background: '#0d1117', foreground: '#e6edf3' },
    });
    const fit = new FitAddon();
    terminal.loadAddon(fit);
    terminal.open(element);
    const entry: Entry = { terminal, fit, element, exited: false };
    this.entries.set(stream, entry);
    this.activate(stream);
    fit.fit();
    terminal.onData((text) => {
      if (entry.exited) {
        return;
      }
      for (const chunk of inputChunks(this.encoder.encode(text))) {
        this.io.input(stream, chunk);
      }
    });
    terminal.onResize(({ cols, rows }) => this.io.resize(stream, cols, rows));
    this.io.open(stream, Math.max(terminal.cols, 1), Math.max(terminal.rows, 1));
    this.changed();
    return stream;
  }

  activate(stream: number | null): void {
    this.active = stream;
    for (const [id, entry] of this.entries) {
      entry.element.hidden = id !== stream;
    }
    this.changed();
    if (stream !== null) {
      const entry = this.entries.get(stream);
      entry?.fit.fit();
      entry?.terminal.focus();
    }
  }

  sendToActive(text: string): void {
    if (this.active !== null) {
      this.entries.get(this.active)?.terminal.input(text, true);
    }
  }

  focusActive(): void {
    if (this.active !== null) {
      this.entries.get(this.active)?.terminal.focus();
    }
  }

  receive(stream: number, message: TerminalMessage): void {
    const entry = this.entries.get(stream);
    if (entry === undefined) {
      return;
    }
    switch (message.kind) {
      case 'output':
        entry.terminal.write(message.data);
        break;
      case 'exited':
        entry.exited = true;
        entry.terminal.write(`\r\n${this.texts.exited(message.code)}\r\n`);
        this.changed();
        break;
      case 'refused':
        entry.exited = true;
        entry.terminal.write(`${this.texts.refused(message.reason)}\r\n`);
        this.changed();
        break;
    }
  }

  close(stream: number): void {
    const entry = this.entries.get(stream);
    if (entry === undefined) {
      return;
    }
    if (!entry.exited) {
      this.io.close(stream);
    }
    entry.terminal.dispose();
    entry.element.remove();
    this.entries.delete(stream);
    if (this.active === stream) {
      this.active = null;
    }
    this.changed();
  }

  endAll(): void {
    for (const entry of this.entries.values()) {
      if (!entry.exited) {
        entry.exited = true;
        entry.terminal.write(`\r\n${this.texts.exited(-1)}\r\n`);
      }
    }
    this.changed();
  }

  private fitActive(): void {
    if (this.active !== null) {
      this.entries.get(this.active)?.fit.fit();
    }
  }
}
