//! Layer 1: the Tauros desktop app. It wires connectors, emulator, MCP server and
//! agent bridge together; the Tauri shell arrives with EMU-004.

fn main() {
    println!("tauros {}", env!("CARGO_PKG_VERSION"));
}
