// Extracts the interface strings that go through translation: literal arguments
// of t(), $t() and the dictation message helper, label pairs in option arrays
// and the values of the label maps. Shared by the i18n test.
import { readFileSync, readdirSync, statSync } from 'node:fs';
import { join } from 'node:path';

function files(dir) {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    return statSync(path).isDirectory() ? files(path) : /\.(svelte|js)$/.test(name) && !name.startsWith('i18n') ? [path] : [];
  });
}

const isText = (s) => /[a-z]/i.test(s) && !/^[A-Z0-9_\-.]+$/.test(s) && !/^[a-z_]+$/.test(s) && !/^(\.|#|\/|ui\/)/.test(s);

function callArgs(source, start) {
  let depth = 0, i = start, quote = null;
  const strings = [];
  let current = '';
  for (; i < source.length; i++) {
    const c = source[i];
    if (quote) {
      if (c === '\\') { current += source[++i]; continue; }
      if (c === quote) { strings.push(current); quote = null; current = ''; continue; }
      current += c; continue;
    }
    if (c === "'" || c === '"') { quote = c; continue; }
    if (c === '(') depth++;
    else if (c === ')') { depth--; if (depth === 0) break; }
    else if (c === '{' && depth === 1) { // skip the params object
      let d = 0;
      for (; i < source.length; i++) { if (source[i] === '{') d++; else if (source[i] === '}') { d--; if (d === 0) break; } }
    }
  }
  return strings;
}

export function interfaceStrings(root = new URL('../src', import.meta.url).pathname) {
  const found = new Set();
  for (const file of files(root)) {
    const source = readFileSync(file, 'utf8');
    for (const m of source.matchAll(/(?<![\w.])(?:\$t|t|tr|message)\(/g)) {
      for (const s of callArgs(source, m.index + m[0].length - 1)) if (isText(s) || /^[a-z]+$/.test(s)) found.add(s);
    }
    for (const m of source.matchAll(/\[\s*'[\w-]+'\s*,\s*'([^']+)'\s*\]/g)) if (isText(m[1])) found.add(m[1]);
    for (const m of source.matchAll(/const (?:ORIGIN|OPERATIONS|BUTTON_LABELS|STEP_STATUS) = \{([^}]*)\}/g)) {
      for (const v of m[1].matchAll(/:\s*'([^']+)'/g)) if (isText(v[1])) found.add(v[1]);
    }
    for (const m of source.matchAll(/\blabel: '([^']+)'/g)) if (isText(m[1])) found.add(m[1]);
    for (const m of source.matchAll(/const examples = \[([^\]]*)\]/g)) for (const v of m[1].matchAll(/'([^']+)'/g)) found.add(v[1]);
    for (const m of source.matchAll(/\b(?:title|opening|idle|signed_out|pending|signed_in|cancelled|failed|expired): '([^']+)'/g)) if (isText(m[1])) found.add(m[1]);
  }
  // Keys chosen at run time (tool states, model list placeholders).
  for (const extra of ['done', 'failed', 'Default', 'Sign in to choose']) found.add(extra);
  return [...found].sort();
}

if (process.argv[1] === new URL(import.meta.url).pathname) console.log(interfaceStrings().join('\n'));
