// Thin wrapper over the Tauri IPC so the controllers can run against mocks in tests.
export function tauriIpc() {
  const core = window.__TAURI__.core;
  return {
    invoke: (command, args, options) => core.invoke(command, args, options),
    channel(onmessage) {
      const channel = new core.Channel();
      channel.onmessage = onmessage;
      return channel;
    },
  };
}

export const log = (ipc, message) => ipc.invoke('log', { message: String(message) }).catch(() => {});
