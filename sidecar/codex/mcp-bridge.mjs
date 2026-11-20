import net from 'node:net';

/**
 * Stdio-to-Unix-socket bridge for connecting a stdio MCP client (such as the
 * Codex app-server's MCP transport) to Tauros's in-process `TaurosServer`.
 *
 * Usage: node mcp-bridge.mjs <unix-socket-path>
 */
const socketPath = process.argv[2];
if (!socketPath) {
  process.stderr.write('usage: mcp-bridge <unix-socket-path>\n');
  process.exit(2);
}

const socket = net.createConnection(socketPath);

socket.on('error', (err) => {
  process.stderr.write(`mcp-bridge socket error: ${err.message}\n`);
  process.exit(1);
});

process.stdin.pipe(socket);
socket.pipe(process.stdout);

socket.on('end', () => {
  process.exit(0);
});

process.stdin.on('end', () => {
  socket.end();
});
