// Game key bindings: browser key events normalised to the canonical names that
// the backend stores (see emu_config.rs), in Button::ALL order.
export const BUTTON_ORDER = ['a', 'b', 'select', 'start', 'right', 'left', 'up', 'down', 'r', 'l'];
export const BUTTON_LABELS = {
  a: 'A', b: 'B', select: 'Select', start: 'Start',
  right: 'D-Pad Right', left: 'D-Pad Left', up: 'D-Pad Up', down: 'D-Pad Down', r: 'R', l: 'L',
};
export const DEFAULT_KEYMAP = {
  a: 'x', b: 'z', select: 'backspace', start: 'enter',
  right: 'arrowright', left: 'arrowleft', up: 'arrowup', down: 'arrowdown', r: 's', l: 'a',
};

const BY_CODE = {
  enter: 'enter', numpadenter: 'enter', backspace: 'backspace', space: 'space', tab: 'tab', escape: 'escape',
  arrowup: 'arrowup', arrowdown: 'arrowdown', arrowleft: 'arrowleft', arrowright: 'arrowright',
  shiftleft: 'shift', shiftright: 'shift', comma: 'comma', period: 'period', slash: 'slash',
  semicolon: 'semicolon', quote: 'quote', bracketleft: 'bracketleft', bracketright: 'bracketright',
  minus: 'minus', equal: 'equal',
};
const BY_KEY = {
  enter: 'enter', backspace: 'backspace', ' ': 'space', tab: 'tab', escape: 'escape',
  arrowup: 'arrowup', arrowdown: 'arrowdown', arrowleft: 'arrowleft', arrowright: 'arrowright',
  shift: 'shift', ',': 'comma', '.': 'period', '/': 'slash', ';': 'semicolon', "'": 'quote',
  '[': 'bracketleft', ']': 'bracketright', '-': 'minus', '=': 'equal',
};

// Physical code first; some input methods (IBus) report an empty code, so fall
// back to the key name.
export function normalizeWebKey(event) {
  const code = (event.code || '').toLowerCase();
  if (/^key[a-z]$/.test(code)) return code.slice(3);
  if (/^digit[0-9]$/.test(code)) return code.slice(5);
  if (/^numpad[0-9]$/.test(code)) return code;
  if (code in BY_CODE) return BY_CODE[code];
  const key = (event.key || '').toLowerCase();
  if (/^[a-z0-9]$/.test(key)) return key;
  if (/^f([1-9]|1[0-2])$/.test(key)) return key;
  return BY_KEY[key] || null;
}

const PRETTY = {
  arrowup: '↑', arrowdown: '↓', arrowleft: '←', arrowright: '→', enter: 'Enter', backspace: 'Backspace',
  space: 'Space', shift: 'Shift', comma: ',', period: '.', slash: '/', semicolon: ';', quote: "'",
  bracketleft: '[', bracketright: ']', minus: '-', equal: '=',
};
export function displayKey(norm) {
  if (!norm) return '—';
  if (norm in PRETTY) return PRETTY[norm];
  return norm.length === 1 ? norm.toUpperCase() : norm;
}

export function buttonBit(keymap, event) {
  const norm = normalizeWebKey(event);
  if (!norm) return undefined;
  const index = BUTTON_ORDER.findIndex((button) => keymap[button] === norm);
  return index >= 0 ? index : undefined;
}

export const isTextField = (el) => !!el && (['TEXTAREA', 'SELECT'].includes(el.tagName) ||
  (el.tagName === 'INPUT' && !['checkbox', 'radio', 'range', 'button', 'file'].includes(el.type)) || el.isContentEditable);
