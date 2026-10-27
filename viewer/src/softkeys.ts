import { evdevCode } from './keymap.js';

export type Layout = 'es' | 'us';

export type KeyChange = { code: number; pressed: boolean };

type Combo = { key: string; shift?: boolean; altGr?: boolean };

const SHIFT = 42;
const ALT_GR = 100;

const common: Record<string, Combo> = {
  ' ': { key: 'Space' },
  '\n': { key: 'Enter' },
  '\t': { key: 'Tab' },
};

const us: Record<string, Combo> = {
  '-': { key: 'Minus' },
  _: { key: 'Minus', shift: true },
  '=': { key: 'Equal' },
  '+': { key: 'Equal', shift: true },
  '[': { key: 'BracketLeft' },
  '{': { key: 'BracketLeft', shift: true },
  ']': { key: 'BracketRight' },
  '}': { key: 'BracketRight', shift: true },
  '\\': { key: 'Backslash' },
  '|': { key: 'Backslash', shift: true },
  ';': { key: 'Semicolon' },
  ':': { key: 'Semicolon', shift: true },
  "'": { key: 'Quote' },
  '"': { key: 'Quote', shift: true },
  ',': { key: 'Comma' },
  '<': { key: 'Comma', shift: true },
  '.': { key: 'Period' },
  '>': { key: 'Period', shift: true },
  '/': { key: 'Slash' },
  '?': { key: 'Slash', shift: true },
  '`': { key: 'Backquote' },
  '~': { key: 'Backquote', shift: true },
  '!': { key: 'Digit1', shift: true },
  '@': { key: 'Digit2', shift: true },
  '#': { key: 'Digit3', shift: true },
  $: { key: 'Digit4', shift: true },
  '%': { key: 'Digit5', shift: true },
  '^': { key: 'Digit6', shift: true },
  '&': { key: 'Digit7', shift: true },
  '*': { key: 'Digit8', shift: true },
  '(': { key: 'Digit9', shift: true },
  ')': { key: 'Digit0', shift: true },
};

const es: Record<string, Combo> = {
  'ñ': { key: 'Semicolon' },
  'Ñ': { key: 'Semicolon', shift: true },
  'ç': { key: 'Backslash' },
  'Ç': { key: 'Backslash', shift: true },
  '-': { key: 'Slash' },
  _: { key: 'Slash', shift: true },
  ',': { key: 'Comma' },
  ';': { key: 'Comma', shift: true },
  '.': { key: 'Period' },
  ':': { key: 'Period', shift: true },
  "'": { key: 'Minus' },
  '?': { key: 'Minus', shift: true },
  '¡': { key: 'Equal' },
  '¿': { key: 'Equal', shift: true },
  '+': { key: 'BracketRight' },
  '*': { key: 'BracketRight', shift: true },
  '<': { key: 'IntlBackslash' },
  '>': { key: 'IntlBackslash', shift: true },
  '!': { key: 'Digit1', shift: true },
  '"': { key: 'Digit2', shift: true },
  '·': { key: 'Digit3', shift: true },
  $: { key: 'Digit4', shift: true },
  '%': { key: 'Digit5', shift: true },
  '&': { key: 'Digit6', shift: true },
  '/': { key: 'Digit7', shift: true },
  '(': { key: 'Digit8', shift: true },
  ')': { key: 'Digit9', shift: true },
  '=': { key: 'Digit0', shift: true },
  '|': { key: 'Digit1', altGr: true },
  '@': { key: 'Digit2', altGr: true },
  '#': { key: 'Digit3', altGr: true },
  '[': { key: 'BracketLeft', altGr: true },
  ']': { key: 'BracketRight', altGr: true },
  '{': { key: 'Quote', altGr: true },
  '}': { key: 'Backslash', altGr: true },
  '\\': { key: 'Backquote', altGr: true },
  '€': { key: 'Digit5', altGr: true },
};

function comboFor(character: string, layout: Layout): Combo | undefined {
  const fixed = common[character];
  if (fixed !== undefined) {
    return fixed;
  }
  if (character >= 'a' && character <= 'z') {
    return { key: `Key${character.toUpperCase()}` };
  }
  if (character >= 'A' && character <= 'Z') {
    return { key: `Key${character}`, shift: true };
  }
  if (character >= '0' && character <= '9') {
    return { key: `Digit${character}` };
  }
  return (layout === 'es' ? es : us)[character];
}

export function keysForCharacter(character: string, layout: Layout): KeyChange[] | undefined {
  const combo = comboFor(character, layout);
  if (combo === undefined) {
    return undefined;
  }
  const code = evdevCode(combo.key);
  if (code === undefined) {
    return undefined;
  }
  const modifiers: number[] = [];
  if (combo.shift === true) {
    modifiers.push(SHIFT);
  }
  if (combo.altGr === true) {
    modifiers.push(ALT_GR);
  }
  return [
    ...modifiers.map((modifier) => ({ code: modifier, pressed: true })),
    { code, pressed: true },
    { code, pressed: false },
    ...[...modifiers].reverse().map((modifier) => ({ code: modifier, pressed: false })),
  ];
}
