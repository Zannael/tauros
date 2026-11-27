import { mount } from 'svelte';
import './app.css';
import App from './App.svelte';

// Report startup failures to the terminal log before anything else can fail.
const report = (message) => window.__TAURI__?.core.invoke('log', { message: String(message) }).catch(() => {});
window.addEventListener('error', (e) => report(`script error: ${e.message} at ${e.filename}:${e.lineno}`));
window.addEventListener('unhandledrejection', (e) => report(`unhandled: ${e.reason?.stack || e.reason}`));

try {
  mount(App, { target: document.getElementById('app') });
} catch (err) {
  report(`mount failed: ${err?.stack || err}`);
  throw err;
}
