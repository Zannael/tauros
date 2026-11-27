// WCAG AA contrast for the theme tokens in both themes: 4.5:1 for text pairs,
// 3:1 for the focus ring and accent fills against their surfaces.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

const css = readFileSync(new URL('../src/app.css', import.meta.url), 'utf8');
function tokens(block) {
  return Object.fromEntries([...block.matchAll(/--([\w-]+):\s*(#[0-9a-f]{6})/gi)].map((m) => [m[1], m[2]]));
}
const light = tokens(css.slice(css.indexOf(':root {'), css.indexOf('@media (prefers-color-scheme: dark)')));
const dark = { ...light, ...tokens(css.slice(css.indexOf(':root[data-theme="dark"]'))) };

function luminance(hex) {
  const [r, g, b] = [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255)
    .map((c) => (c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4));
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}
const ratio = (a, b) => { const [x, y] = [luminance(a), luminance(b)].sort((p, q) => q - p); return (x + 0.05) / (y + 0.05); };

const TEXT = [
  ['text', 'bg'], ['text', 'surface'], ['text', 'surface-2'], ['text', 'surface-3'], ['text', 'accent-soft'],
  ['muted', 'bg'], ['muted', 'surface'], ['muted', 'surface-2'],
  ['accent-text', 'surface'], ['accent-text', 'surface-2'], ['accent-text', 'accent-soft'], ['on-accent', 'accent'], ['on-accent', 'accent-hover'],
  ['ok', 'ok-soft'], ['ok', 'surface'], ['warn', 'warn-soft'], ['warn', 'surface'], ['err', 'err-soft'], ['err', 'surface'],
];
const NON_TEXT = [['focus', 'surface'], ['focus', 'bg'], ['accent', 'surface'], ['border-strong', 'surface']];

for (const [name, theme] of [['light', light], ['dark', dark]]) {
  test(`${name} theme meets WCAG AA`, () => {
    for (const [fg, bg] of TEXT) assert.ok(ratio(theme[fg], theme[bg]) >= 4.5, `${name}: ${fg} on ${bg} = ${ratio(theme[fg], theme[bg]).toFixed(2)}`);
    for (const [fg, bg] of NON_TEXT) assert.ok(ratio(theme[fg], theme[bg]) >= 3, `${name}: ${fg} on ${bg} = ${ratio(theme[fg], theme[bg]).toFixed(2)}`);
  });
}
