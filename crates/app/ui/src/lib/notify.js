// Notifications: errors stay until dismissed; other messages fade after a few
// seconds. Every message is also kept in a short session list (the bell).
import { writable } from 'svelte/store';

let nextId = 1;
export const toasts = writable([]);
export const notifications = writable([]);

export function notify(text, kind = 'info') {
  const item = { id: nextId++, text: String(text), kind, at: new Date() };
  toasts.update((list) => [...list.slice(-3), item]);
  notifications.update((list) => [item, ...list].slice(0, 50));
  if (kind !== 'error') setTimeout(() => dismissToast(item.id), 4500);
  return item.id;
}

export function dismissToast(id) {
  toasts.update((list) => list.filter((item) => item.id !== id));
}

export function clearNotifications() {
  notifications.set([]);
}
