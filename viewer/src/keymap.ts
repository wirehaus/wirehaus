const entries: [string, number][] = [
  ['Escape', 1],
  ['Minus', 12],
  ['Equal', 13],
  ['Backspace', 14],
  ['Tab', 15],
  ['BracketLeft', 26],
  ['BracketRight', 27],
  ['Enter', 28],
  ['ControlLeft', 29],
  ['Semicolon', 39],
  ['Quote', 40],
  ['Backquote', 41],
  ['ShiftLeft', 42],
  ['Backslash', 43],
  ['Comma', 51],
  ['Period', 52],
  ['Slash', 53],
  ['ShiftRight', 54],
  ['NumpadMultiply', 55],
  ['AltLeft', 56],
  ['Space', 57],
  ['CapsLock', 58],
  ['NumLock', 69],
  ['ScrollLock', 70],
  ['NumpadSubtract', 74],
  ['NumpadAdd', 78],
  ['NumpadDecimal', 83],
  ['IntlBackslash', 86],
  ['IntlRo', 89],
  ['NumpadEnter', 96],
  ['ControlRight', 97],
  ['NumpadDivide', 98],
  ['PrintScreen', 99],
  ['AltRight', 100],
  ['Home', 102],
  ['ArrowUp', 103],
  ['PageUp', 104],
  ['ArrowLeft', 105],
  ['ArrowRight', 106],
  ['End', 107],
  ['ArrowDown', 108],
  ['PageDown', 109],
  ['Insert', 110],
  ['Delete', 111],
  ['AudioVolumeMute', 113],
  ['AudioVolumeDown', 114],
  ['AudioVolumeUp', 115],
  ['Pause', 119],
  ['MetaLeft', 125],
  ['MetaRight', 126],
  ['ContextMenu', 127],
];

const letterRows: [string, number][] = [
  ['QWERTYUIOP', 16],
  ['ASDFGHJKL', 30],
  ['ZXCVBNM', 44],
];

const keys = new Map<string, number>(entries);

for (const [row, first] of letterRows) {
  [...row].forEach((letter, index) => keys.set(`Key${letter}`, first + index));
}
for (let digit = 1; digit <= 9; digit += 1) {
  keys.set(`Digit${digit}`, 1 + digit);
}
keys.set('Digit0', 11);
for (let number = 1; number <= 10; number += 1) {
  keys.set(`F${number}`, 58 + number);
}
keys.set('F11', 87);
keys.set('F12', 88);
for (let number = 13; number <= 24; number += 1) {
  keys.set(`F${number}`, 170 + number);
}
[7, 8, 9].forEach((digit, index) => keys.set(`Numpad${digit}`, 71 + index));
[4, 5, 6].forEach((digit, index) => keys.set(`Numpad${digit}`, 75 + index));
[1, 2, 3].forEach((digit, index) => keys.set(`Numpad${digit}`, 79 + index));
keys.set('Numpad0', 82);

export function evdevCode(domCode: string): number | undefined {
  return keys.get(domCode);
}

export function mappedCodes(): number[] {
  return [...keys.values()];
}
