import { type Language, type Token, lineStarts, piecesForLine, tokenize } from './highlight.js';

const OVERSCAN = 6;
const PAIRS: Record<string, string> = { '(': ')', '[': ']', '{': '}', '"': '"', "'": "'", '`': '`' };
const CLOSERS = new Set([')', ']', '}']);
const COMMENT_PREFIX: Partial<Record<Language, string>> = {
  js: '//', rust: '//', go: '//', c: '//', java: '//', php: '//', python: '#', shell: '#', yaml: '#', toml: '#', sql: '--',
};

export type CodeAreaEvents = {
  changed(): void;
  caret(line: number, column: number, selected: number): void;
  save(): void;
};

export class CodeArea {
  readonly root: HTMLElement;
  readonly textarea: HTMLTextAreaElement;
  private readonly gutter: HTMLElement;
  private readonly gutterInner: HTMLElement;
  private readonly highlight: HTMLElement;
  private readonly band: HTMLElement;
  private tokens: Token[] = [];
  private starts: number[] = [0];
  private stale = true;
  private frame = 0;
  private timer: number | undefined;
  private lineHeight = 21;
  private padTop = 10;
  private padLeft = 14;
  private language: Language;
  private disposed = false;

  constructor(
    text: string,
    language: Language,
    label: string,
    private readonly events: CodeAreaEvents,
  ) {
    this.language = language;
    this.root = document.createElement('div');
    this.root.className = 'fm-code';
    this.gutter = document.createElement('div');
    this.gutter.className = 'fm-gutter';
    this.gutter.setAttribute('aria-hidden', 'true');
    this.gutterInner = document.createElement('div');
    this.gutter.append(this.gutterInner);
    const stage = document.createElement('div');
    stage.className = 'fm-stage';
    this.band = document.createElement('div');
    this.band.className = 'fm-curline';
    this.highlight = document.createElement('pre');
    this.highlight.className = 'fm-hl';
    this.highlight.setAttribute('aria-hidden', 'true');
    this.textarea = document.createElement('textarea');
    this.textarea.className = 'fm-textarea';
    this.textarea.value = text;
    this.textarea.spellcheck = false;
    this.textarea.autocapitalize = 'off';
    this.textarea.autocomplete = 'off';
    this.textarea.setAttribute('wrap', 'off');
    this.textarea.setAttribute('aria-label', label);
    stage.append(this.band, this.highlight, this.textarea);
    this.root.append(this.gutter, stage);

    this.textarea.addEventListener('input', () => {
      this.stale = true;
      this.events.changed();
      this.schedule();
      this.reportCaret();
    });
    this.textarea.addEventListener('scroll', () => this.schedule(), { passive: true });
    for (const name of ['keyup', 'mouseup', 'focus', 'select'] as const) {
      this.textarea.addEventListener(name, () => this.reportCaret());
    }
    this.textarea.addEventListener('blur', () => this.paintBand());
    this.textarea.addEventListener('keydown', (event) => this.keydown(event));
    document.addEventListener('selectionchange', this.onSelection);
    new ResizeObserver(() => this.schedule()).observe(this.root);
  }

  private readonly onSelection = (): void => {
    if (!this.disposed && document.activeElement === this.textarea) {
      this.reportCaret();
    }
  };

  mount(): void {
    const style = getComputedStyle(this.textarea);
    this.lineHeight = Number.parseFloat(style.lineHeight) || 21;
    this.padTop = Number.parseFloat(style.paddingTop) || 0;
    this.padLeft = Number.parseFloat(style.paddingLeft) || 0;
    this.stale = true;
    this.paint();
    this.reportCaret();
  }

  get value(): string {
    return this.textarea.value;
  }

  focus(): void {
    this.textarea.focus();
  }

  setLanguage(language: Language): void {
    this.language = language;
    this.stale = true;
    this.schedule();
  }

  dispose(): void {
    this.disposed = true;
    document.removeEventListener('selectionchange', this.onSelection);
    cancelAnimationFrame(this.frame);
    window.clearTimeout(this.timer);
  }

  private schedule(): void {
    if (this.frame !== 0) {
      return;
    }
    this.frame = requestAnimationFrame(() => {
      this.frame = 0;
      this.paint();
    });
  }

  private paint(): void {
    if (this.disposed) {
      return;
    }
    const text = this.textarea.value;
    if (this.stale) {
      this.starts = lineStarts(text);
      this.tokens = tokenize(text, this.language);
      this.stale = false;
    }
    const top = this.textarea.scrollTop;
    const left = this.textarea.scrollLeft;
    const lines = this.starts.length;
    const first = Math.max(0, Math.floor(top / this.lineHeight) - OVERSCAN);
    const last = Math.min(lines - 1, Math.ceil((top + this.textarea.clientHeight) / this.lineHeight) + OVERSCAN);
    const fragment = document.createDocumentFragment();
    const numbers = document.createDocumentFragment();
    for (let line = first; line <= last; line += 1) {
      for (const piece of piecesForLine(text, this.starts, this.tokens, line)) {
        if (piece.cls === '') {
          fragment.append(piece.text);
        } else {
          const span = document.createElement('span');
          span.className = `tk-${piece.cls}`;
          span.textContent = piece.text;
          fragment.append(span);
        }
      }
      fragment.append('\n');
      const number = document.createElement('div');
      number.textContent = String(line + 1);
      number.dataset['line'] = String(line);
      numbers.append(number);
    }
    this.highlight.replaceChildren(fragment);
    this.gutterInner.replaceChildren(numbers);
    const offset = this.padTop + first * this.lineHeight - top;
    this.highlight.style.transform = `translate(${this.padLeft - left}px, ${offset}px)`;
    this.gutterInner.style.transform = `translateY(${offset}px)`;
    this.gutter.style.width = `${Math.max(3, String(lines).length) + 2}ch`;
    this.paintBand();
  }

  private lineOf(index: number): number {
    let low = 0;
    let high = this.starts.length - 1;
    while (low < high) {
      const mid = (low + high + 1) >> 1;
      if ((this.starts[mid] ?? 0) <= index) low = mid;
      else high = mid - 1;
    }
    return low;
  }

  private paintBand(): void {
    const area = this.textarea;
    const focused = document.activeElement === area;
    const single = area.selectionStart === area.selectionEnd;
    this.band.hidden = !focused || !single;
    if (this.band.hidden) {
      return;
    }
    const line = this.lineOf(area.selectionStart);
    this.band.style.height = `${this.lineHeight}px`;
    this.band.style.transform = `translateY(${this.padTop + line * this.lineHeight - area.scrollTop}px)`;
    for (const number of this.gutterInner.children) {
      number.classList.toggle('current', (number as HTMLElement).dataset['line'] === String(line));
    }
  }

  private reportCaret(): void {
    const area = this.textarea;
    if (this.stale) {
      this.starts = lineStarts(area.value);
    }
    const line = this.lineOf(area.selectionStart);
    const column = area.selectionStart - (this.starts[line] ?? 0) + 1;
    this.events.caret(line + 1, column, Math.abs(area.selectionEnd - area.selectionStart));
    this.paintBand();
  }

  private insert(text: string): void {
    const area = this.textarea;
    area.focus();
    let done = false;
    try {
      done = document.execCommand('insertText', false, text);
    } catch {
      done = false;
    }
    if (!done) {
      area.setRangeText(text, area.selectionStart, area.selectionEnd, 'end');
      area.dispatchEvent(new Event('input', { bubbles: true }));
    }
  }

  private replaceRange(from: number, to: number, text: string, selectFrom: number, selectTo: number): void {
    const area = this.textarea;
    area.focus();
    area.setSelectionRange(from, to);
    this.insert(text);
    area.setSelectionRange(selectFrom, selectTo);
    this.reportCaret();
  }

  private keydown(event: KeyboardEvent): void {
    const area = this.textarea;
    const mod = event.ctrlKey || event.metaKey;
    if (event.isComposing || event.keyCode === 229) {
      return;
    }
    if (mod && event.key.toLowerCase() === 's') {
      event.preventDefault();
      this.events.save();
      return;
    }
    if (mod && event.key === '/') {
      event.preventDefault();
      this.toggleComment();
      return;
    }
    if (event.key === 'Tab' && !mod && !event.altKey) {
      event.preventDefault();
      this.indent(event.shiftKey);
      return;
    }
    if (event.key === 'Enter' && !mod && !event.shiftKey) {
      this.enter(event);
      return;
    }
    if (mod || event.altKey) {
      return;
    }
    if (event.key === 'Backspace' && area.selectionStart === area.selectionEnd) {
      const at = area.selectionStart;
      const pair = PAIRS[area.value[at - 1] ?? ''];
      if (pair !== undefined && area.value[at] === pair) {
        event.preventDefault();
        this.replaceRange(at - 1, at + 1, '', at - 1, at - 1);
      }
      return;
    }
    this.pairing(event);
  }

  private pairing(event: KeyboardEvent): void {
    const area = this.textarea;
    const key = event.key;
    const text = area.value;
    const from = area.selectionStart;
    const to = area.selectionEnd;
    if ((CLOSERS.has(key) || key === '"' || key === "'" || key === '`') && from === to && text[from] === key) {
      event.preventDefault();
      area.setSelectionRange(from + 1, from + 1);
      this.reportCaret();
      return;
    }
    const closer = PAIRS[key];
    if (closer === undefined) {
      return;
    }
    if (from !== to) {
      event.preventDefault();
      const inner = text.slice(from, to);
      this.replaceRange(from, to, `${key}${inner}${closer}`, from + 1, from + 1 + inner.length);
      return;
    }
    const before = text[from - 1] ?? '';
    const after = text[from] ?? '';
    const quote = key === '"' || key === "'" || key === '`';
    if (quote && (/[\w]/u.test(before) || (after !== '' && /[\w]/u.test(after)))) {
      return;
    }
    if (after !== '' && !/[\s)\]},;:.]/u.test(after)) {
      return;
    }
    event.preventDefault();
    this.replaceRange(from, from, `${key}${closer}`, from + 1, from + 1);
  }

  private enter(event: KeyboardEvent): void {
    const area = this.textarea;
    const text = area.value;
    const from = area.selectionStart;
    const to = area.selectionEnd;
    const lineStart = text.lastIndexOf('\n', from - 1) + 1;
    const indent = /^[ \t]*/u.exec(text.slice(lineStart, from))?.[0] ?? '';
    const before = text[from - 1] ?? '';
    const after = text[to] ?? '';
    const between = (before === '{' && after === '}') || (before === '[' && after === ']') || (before === '(' && after === ')');
    if (between) {
      event.preventDefault();
      const unit = indent.includes('\t') || indent === '' ? '\t' : '  ';
      const insertion = `\n${indent}${unit}\n${indent}`;
      const caret = from + 1 + indent.length + unit.length;
      this.replaceRange(from, to, insertion, caret, caret);
      return;
    }
    if (indent !== '') {
      event.preventDefault();
      this.insert(`\n${indent}`);
    }
  }

  private selectedLines(): { first: number; last: number } {
    const area = this.textarea;
    this.starts = lineStarts(area.value);
    const first = this.lineOf(area.selectionStart);
    let endIndex = area.selectionEnd;
    if (endIndex > area.selectionStart && area.value[endIndex - 1] === '\n') {
      endIndex -= 1;
    }
    return { first, last: this.lineOf(endIndex) };
  }

  private indent(outdent: boolean): void {
    const area = this.textarea;
    const multi = area.selectionStart !== area.selectionEnd && area.value.slice(area.selectionStart, area.selectionEnd).includes('\n');
    if (!multi && !outdent) {
      this.insert('\t');
      return;
    }
    const { first, last } = this.selectedLines();
    const start = this.starts[first] ?? 0;
    const end = last + 1 < this.starts.length ? (this.starts[last + 1] ?? area.value.length) - 1 : area.value.length;
    const lines = area.value.slice(start, end).split('\n');
    const changed = lines.map((line) => (outdent ? line.replace(/^(?:\t| {1,4})/u, '') : `\t${line}`));
    const replacement = changed.join('\n');
    this.replaceRange(start, end, replacement, start, start + replacement.length);
  }

  private toggleComment(): void {
    const prefix = COMMENT_PREFIX[this.language];
    if (prefix === undefined) {
      return;
    }
    const area = this.textarea;
    const { first, last } = this.selectedLines();
    const start = this.starts[first] ?? 0;
    const end = last + 1 < this.starts.length ? (this.starts[last + 1] ?? area.value.length) - 1 : area.value.length;
    const lines = area.value.slice(start, end).split('\n');
    const active = lines.filter((line) => line.trim() !== '');
    const commented = active.length > 0 && active.every((line) => line.trimStart().startsWith(prefix));
    const changed = lines.map((line) => {
      if (line.trim() === '') return line;
      if (commented) return line.replace(new RegExp(`^(\\s*)${prefix.replace(/[-/\\^$*+?.()|[\]{}]/gu, '\\$&')} ?`, 'u'), '$1');
      const lead = /^\s*/u.exec(line)?.[0] ?? '';
      return `${lead}${prefix} ${line.slice(lead.length)}`;
    });
    const replacement = changed.join('\n');
    this.replaceRange(start, end, replacement, start, start + replacement.length);
  }
}
