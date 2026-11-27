// Interface language. Keys are the English strings; `{name}` placeholders are
// filled from the second argument. Messages coming from the backend stay as sent.
import { derived } from 'svelte/store';
import { uiPrefs } from './prefs.js';
import { it } from './i18n_it.js';

export const LANGUAGES = { en: 'English', it: 'Italiano' };

export function translate(language, key, params) {
  const text = (language === 'it' && it[key]) || key;
  return params ? text.replace(/\{(\w+)\}/g, (_, name) => (name in params ? String(params[name]) : `{${name}}`)) : text;
}

export const t = derived(uiPrefs, ($prefs) => (key, params) => translate($prefs.language, key, params));
