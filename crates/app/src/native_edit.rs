//! Private transaction channel for the SDK's native editing hooks (AGT-008).
//! A connection holds the project's operation gate from snapshot through record.
//! EOF (cancellation or sidecar exit) also records any partial edits, never resets.

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::Shutdown;
use std::os::unix::fs::MetadataExt;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde::Deserialize;
use serde_json::json;
use tauros_connector_gen3::snapshot::GitGuard;

#[derive(Deserialize)]
struct Begin {
    tool: String,
    path: Option<PathBuf>,
}

/// Host-owned endpoint. It is deliberately separate from the agent's MCP tools.
pub struct NativeEditHost {
    dir: PathBuf,
    socket: PathBuf,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

fn reply(stream: &mut UnixStream, value: serde_json::Value) -> Result<(), String> {
    writeln!(stream, "{value}").map_err(|e| e.to_string())
}

/// Reject traversal, symlinks and Git control files, including absent targets.
pub fn source_path(root: &Path, path: &Path) -> Result<PathBuf, String> {
    let path = if path.is_absolute() {
        path.to_owned()
    } else {
        root.join(path)
    };
    let relative = path
        .strip_prefix(root)
        .map_err(|_| "path is outside the game project")?;
    if relative.as_os_str().is_empty() {
        return Err("a source file path is required".into());
    }
    let mut current = root.to_owned();
    for part in relative.components() {
        let Component::Normal(name) = part else {
            return Err("path traversal is not allowed".into());
        };
        if [".git", ".gitignore", ".gitattributes", ".gitmodules"]
            .iter()
            .any(|p| name == *p)
        {
            return Err("Git control files are managed by Tauros".into());
        }
        current.push(name);
        match fs::symlink_metadata(&current) {
            Ok(meta) if meta.file_type().is_symlink() || (meta.is_file() && meta.nlink() > 1) => {
                return Err("symlink and hardlink paths are not allowed".into())
            }
            Ok(_) => (),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(path)
}

fn transaction(root: &Path, gate: &Mutex<()>, mut stream: UnixStream) -> Result<(), String> {
    // Concurrent calls fail promptly instead of deadlocking a parallel SDK batch.
    let _operation = gate
        .try_lock()
        .map_err(|_| "project busy; retry tools sequentially")?;
    let mut reader = BufReader::new(stream.try_clone().map_err(|e| e.to_string())?);
    let mut line = String::new();
    reader.read_line(&mut line).map_err(|e| e.to_string())?;
    let begin: Begin = serde_json::from_str(&line).map_err(|e| e.to_string())?;
    if !["Edit", "Write", "Bash"].contains(&begin.tool.as_str()) {
        return Err("unsupported native mutation".into());
    }
    if begin.tool != "Bash" {
        let path = source_path(root, begin.path.as_deref().ok_or("missing file path")?)?;
        let ignored = std::process::Command::new("git")
            .current_dir(root)
            .args(["check-ignore", "-q", "--"])
            .arg(path)
            .status()
            .map_err(|e| e.to_string())?;
        if ignored.success() {
            return Err("ignored files cannot be edited with source tools".into());
        }
    } else {
        // A bind mount cannot isolate writes through a pre-existing hardlink to
        // an inode outside the project. Refuse such trees before allowing Bash.
        let mut dirs = vec![root.to_owned()];
        while let Some(dir) = dirs.pop() {
            for entry in fs::read_dir(dir).map_err(|e| e.to_string())? {
                let entry = entry.map_err(|e| e.to_string())?;
                if entry.file_name() == ".git" {
                    continue;
                }
                let meta = fs::symlink_metadata(entry.path()).map_err(|e| e.to_string())?;
                if meta.is_file() && meta.nlink() > 1 {
                    return Err(
                        "shell editing refuses a project containing hardlinked files".into(),
                    );
                }
                if meta.is_dir() {
                    dirs.push(entry.path());
                }
            }
        }
    }
    let guard = GitGuard::take_over(root).map_err(|e| e.to_string())?;
    let summary = format!("tauros: native {}", begin.tool);
    let snapshot = guard.begin_external(&summary).map_err(|e| e.to_string())?;
    // Even if the peer disappears after this write, finish recording below.
    let sent = reply(&mut stream, json!({ "ok": true, "snapshot": snapshot }));
    line.clear();
    if sent.is_ok() {
        let _ = reader.read_line(&mut line);
    }
    let receipt = guard.finish_external(&snapshot, &summary).map_err(|e| {
        format!("native edits retained; snapshot {snapshot}; recording failed: {e}")
    })?;
    let _ = reply(
        &mut stream,
        json!({ "ok": true, "snapshot": receipt.snapshot, "commit": receipt.commit }),
    );
    Ok(())
}

impl NativeEditHost {
    pub fn start(root: &Path, gate: Arc<Mutex<()>>) -> Result<Self, String> {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let root = fs::canonicalize(root).map_err(|e| e.to_string())?;
        let dir = std::env::temp_dir().join(format!(
            "tauros-native-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&dir).map_err(|e| e.to_string())?;
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
        let socket = dir.join("guard.sock");
        let listener = UnixListener::bind(&socket).map_err(|e| e.to_string())?;
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = Arc::clone(&stop);
        let thread = thread::spawn(move || {
            let mut workers: Vec<(UnixStream, JoinHandle<()>)> = Vec::new();
            while !stopping.load(Ordering::SeqCst) {
                workers.retain(|(_, worker)| !worker.is_finished());
                match listener.accept() {
                    Ok((stream, _)) => {
                        let Ok(control) = stream.try_clone() else {
                            continue;
                        };
                        let root = root.clone();
                        let gate = Arc::clone(&gate);
                        workers.push((
                            control,
                            thread::spawn(move || {
                                let _ = stream.set_read_timeout(Some(Duration::from_secs(90)));
                                let mut response =
                                    stream.try_clone().expect("connected native socket");
                                if let Err(message) = transaction(&root, &gate, stream) {
                                    let _ = reply(
                                        &mut response,
                                        json!({ "ok": false, "message": message }),
                                    );
                                }
                            }),
                        ));
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10))
                    }
                    Err(_) => break,
                }
            }
            for (stream, worker) in workers {
                let _ = stream.shutdown(Shutdown::Both);
                let _ = worker.join();
            }
        });
        Ok(Self {
            dir,
            socket,
            stop,
            thread: Some(thread),
        })
    }

    pub fn socket_path(&self) -> &Path {
        &self.socket
    }
}

impl Drop for NativeEditHost {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        let _ = fs::remove_dir_all(&self.dir);
    }
}
