// Every interface string has an Italian translation with the same placeholders.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { interfaceStrings } from './strings.js';
import { it } from '../src/lib/i18n_it.js';
import { translate } from '../src/lib/i18n.js';

const placeholders = (s) => [...s.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort();

test('every interface string is translated', () => {
  const missing = interfaceStrings().filter((key) => !(key in it));
  assert.deepEqual(missing, []);
});

test('translations keep their placeholders', () => {
  for (const [key, value] of Object.entries(it)) assert.deepEqual(placeholders(value), placeholders(key), key);
});

test('translate fills placeholders and falls back to English', () => {
  assert.equal(translate('it', 'Frame {n}', { n: 12 }), 'Frame 12');
  assert.equal(translate('it', 'Pause'), 'Pausa');
  assert.equal(translate('en', 'Pause'), 'Pause');
  assert.equal(translate('it', 'Not a known string'), 'Not a known string');
});
