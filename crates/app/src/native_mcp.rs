//! Native project tools as a minimal MCP server (AGT-007). Antigravity has no
//! host dynamic tools, so the same snapshot-guarded tools Codex receives are
//! served over a private Unix socket and attached as the `project` MCP server.
//! Cancellation (`notifications/cancelled`) and disconnection stop running calls;
//! the tools themselves record partial edits before returning.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde_json::{json, Value};
use tauros_agent_bridge::codex::CodexTools;
use tauros_agent_bridge::McpServerConfig;

pub struct NativeMcpHost {
    dir: PathBuf,
    socket: PathBuf,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

fn send(stream: &Mutex<UnixStream>, value: Value) {
    let _ = writeln!(stream.lock().unwrap(), "{value}");
}

fn serve(tools: Arc<dyn CodexTools>, stream: UnixStream) {
    let Ok(read) = stream.try_clone() else { return };
    let out = Arc::new(Mutex::new(stream));
    let running: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>> = Arc::default();
    let mut workers = Vec::new();
    for line in BufReader::new(read).lines() {
        let Ok(line) = line else { break };
        let Ok(message) = serde_json::from_str::<Value>(&line) else { continue };
        let id = message["id"].clone();
        let params = &message["params"];
        let result = match message["method"].as_str().unwrap_or("") {
            "initialize" => json!({
                "protocolVersion": params["protocolVersion"].as_str().unwrap_or("2025-06-18"),
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "tauros-project", "version": env!("CARGO_PKG_VERSION")}
            }),
            "notifications/cancelled" => {
                if let Some(flag) = running.lock().unwrap().get(&params["requestId"].to_string()) {
                    flag.store(true, Ordering::SeqCst);
                }
                continue;
            }
            "ping" => json!({}),
            "tools/list" => json!({"tools": tools.definitions().into_iter().map(|tool| json!({
                "name": tool["name"], "description": tool["description"], "inputSchema": tool["inputSchema"]
            })).collect::<Vec<_>>()}),
            "tools/call" => {
                let cancelled = Arc::new(AtomicBool::new(false));
                running.lock().unwrap().insert(id.to_string(), Arc::clone(&cancelled));
                let (tools, out, running) = (Arc::clone(&tools), Arc::clone(&out), Arc::clone(&running));
                let name = params["name"].as_str().unwrap_or("").to_owned();
                let arguments = params["arguments"].clone();
                workers.push((Arc::clone(&cancelled), thread::spawn(move || {
                    let result = tools.call(&name, arguments, &cancelled);
                    running.lock().unwrap().remove(&id.to_string());
                    let (text, is_error) = match result {
                        Ok(value) => (value.to_string(), false),
                        Err(error) => (error, true),
                    };
                    send(&out, json!({"jsonrpc": "2.0", "id": id, "result": {
                        "content": [{"type": "text", "text": text}], "isError": is_error
                    }}));
                })));
                continue;
            }
            method if method.starts_with("notifications/") || id.is_null() => continue,
            _ => {
                send(&out, json!({"jsonrpc": "2.0", "id": id,
                    "error": {"code": -32601, "message": "method not found"}}));
                continue;
            }
        };
        send(&out, json!({"jsonrpc": "2.0", "id": id, "result": result}));
    }
    // The client is gone: stop commands, but let every call record its edits.
    for (cancelled, worker) in workers {
        cancelled.store(true, Ordering::SeqCst);
        let _ = worker.join();
    }
}

impl NativeMcpHost {
    pub fn start(tools: Arc<dyn CodexTools>) -> Result<Self, String> {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let dir = std::env::temp_dir().join(format!(
            "tauros-project-mcp-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&dir).map_err(|e| e.to_string())?;
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
        let socket = dir.join("project.sock");
        let listener = UnixListener::bind(&socket).map_err(|e| e.to_string())?;
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = Arc::clone(&stop);
        let thread = thread::spawn(move || {
            let mut connections: Vec<(UnixStream, JoinHandle<()>)> = Vec::new();
            while !stopping.load(Ordering::SeqCst) {
                connections.retain(|(_, worker)| !worker.is_finished());
                match listener.accept() {
                    Ok((stream, _)) => {
                        let _ = stream.set_nonblocking(false);
                        let Ok(control) = stream.try_clone() else { continue };
                        let tools = Arc::clone(&tools);
                        connections.push((control, thread::spawn(move || serve(tools, stream))));
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => thread::sleep(Duration::from_millis(10)),
                    Err(_) => break,
                }
            }
            for (stream, worker) in connections {
                let _ = stream.shutdown(std::net::Shutdown::Both);
                let _ = worker.join();
            }
        });
        Ok(Self { dir, socket, stop, thread: Some(thread) })
    }

    pub fn socket_path(&self) -> &Path {
        &self.socket
    }

    /// Stdio launch through the same socket bridge used for the Tauros server.
    pub fn mcp_server_config(&self, node: &Path, bridge: &Path) -> McpServerConfig {
        McpServerConfig::Stdio {
            command: node.to_string_lossy().into_owned(),
            args: vec![bridge.to_string_lossy().into_owned(), self.socket.to_string_lossy().into_owned()],
            env: BTreeMap::new(),
        }
    }
}

impl Drop for NativeMcpHost {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        let _ = fs::remove_dir_all(&self.dir);
    }
}
