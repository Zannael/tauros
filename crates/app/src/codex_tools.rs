//! Host execution boundary for Codex dynamic tools. Uses the AGT-008 transaction
//! socket rather than relying on vendor approval callbacks as write hooks.
use crate::native_edit::source_path;
use serde_json::{json, Value};
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;
use tauros_agent_bridge::codex::CodexTools;

pub struct ProjectCodexTools {
    root: PathBuf,
    socket: Option<PathBuf>,
    serial: Mutex<()>,
    failure: Mutex<Option<String>>,
}
impl ProjectCodexTools {
    pub fn new(root: PathBuf, socket: Option<PathBuf>) -> Self {
        Self {
            root,
            socket,
            serial: Mutex::new(()),
            failure: Mutex::new(None),
        }
    }
}
fn field<'a>(args: &'a Value, name: &str) -> Result<&'a str, String> {
    args[name]
        .as_str()
        .ok_or_else(|| format!("missing string field {name}"))
}
fn response(reader: &mut BufReader<UnixStream>) -> Result<Value, String> {
    let mut line = String::new();
    reader.read_line(&mut line).map_err(|e| e.to_string())?;
    let result: Value =
        serde_json::from_str(&line).map_err(|_| "native transaction host disconnected")?;
    if result["ok"] != true {
        return Err(result["message"]
            .as_str()
            .unwrap_or("native transaction rejected")
            .into());
    }
    Ok(result)
}
impl CodexTools for ProjectCodexTools {
    fn definitions(&self) -> Vec<Value> {
        if self.socket.is_none() {
            return vec![];
        }
        [
            ("tauros_read_file", "Read a UTF-8 source file inside the game project.", json!({"path":{"type":"string"}}), vec!["path"]),
            ("tauros_write_file", "Create or replace a UTF-8 source file, with a snapshot before writing and AI provenance afterwards. Read before replacing.", json!({"path":{"type":"string"},"content":{"type":"string"}}), vec!["path","content"]),
            ("tauros_shell", "Run an offline development command in the game project. Confined filesystem, read-only Git, 50 seconds; no background jobs. Use Tauros MCP for builds and emulator work. Also supports source search/listing.", json!({"command":{"type":"string"}}), vec!["command"]),
        ].into_iter().map(|(name, description, properties, required)| json!({"type":"function", "name":name,
            "description":description,"inputSchema":{"type":"object","properties":properties,"required":required,"additionalProperties":false}})).collect()
    }
    fn call(&self, name: &str, args: Value, cancelled: &AtomicBool) -> Result<Value, String> {
        let socket = self
            .socket
            .as_ref()
            .ok_or("open a game project before using native tools")?;
        let _serial = self
            .serial
            .try_lock()
            .map_err(|_| "native operation in progress; retry sequentially")?;
        if let Some(failure) = self.failure.lock().unwrap().as_ref() {
            return Err(failure.clone());
        }
        if cancelled.load(Ordering::SeqCst) {
            return Err("turn cancelled".into());
        }
        if name == "tauros_read_file" {
            let target = source_path(&self.root, Path::new(field(&args, "path")?))?;
            if fs::metadata(&target).map_err(|e| e.to_string())?.len() > 2 * 1024 * 1024 {
                return Err("source file exceeds 2 MiB; use a focused search".into());
            }
            return fs::read_to_string(target)
                .map(|text| json!({"text":text}))
                .map_err(|e| e.to_string());
        }
        let (tool, target) = match name {
            "tauros_write_file" => {
                field(&args, "content")?;
                (
                    "Write",
                    Some(source_path(&self.root, Path::new(field(&args, "path")?))?),
                )
            }
            "tauros_shell" => {
                field(&args, "command")?;
                ("Bash", None)
            }
            _ => return Err("unsupported native tool".into()),
        };
        let mut stream = UnixStream::connect(socket).map_err(|e| e.to_string())?;
        stream
            .set_read_timeout(Some(Duration::from_secs(80)))
            .map_err(|e| e.to_string())?;
        let mut reader = BufReader::new(stream.try_clone().map_err(|e| e.to_string())?);
        writeln!(stream, "{}", json!({"tool":tool,"path":target})).map_err(|e| e.to_string())?;
        response(&mut reader)?; // Nothing may mutate before this succeeds.
        let result = if cancelled.load(Ordering::SeqCst) {
            Err("turn cancelled".into())
        } else if let Some(target) = target {
            // Revalidate after acquiring the gate. Parents must already exist.
            source_path(&self.root, &target)
                .and_then(|target| {
                    fs::write(target, field(&args, "content")?).map_err(|e| e.to_string())
                })
                .map(|()| json!({"written":true}))
        } else {
            shell(&self.root, field(&args, "command")?, cancelled)
        };
        // Always record partial edits, including failed commands and cancellation.
        let finish = writeln!(stream, "{{}}")
            .map_err(|e| e.to_string())
            .and_then(|()| response(&mut reader));
        if let Err(error) = finish {
            let error = format!(
                "native edits retained; recording failed: {error}; start a new chat to recover"
            );
            *self.failure.lock().unwrap() = Some(error.clone());
            return Err(error);
        }
        result
    }
}

fn capture(mut stream: impl Read) -> String {
    let mut captured = Vec::new();
    let mut buf = [0; 8192];
    while let Ok(count) = stream.read(&mut buf) {
        if count == 0 {
            break;
        }
        let retain = count.min((2 * 1024 * 1024usize).saturating_sub(captured.len()));
        captured.extend_from_slice(&buf[..retain]);
    }
    String::from_utf8_lossy(&captured).into_owned()
}
/// Same minimal mount namespace as the Claude native command policy. There is
/// deliberately no unsandboxed fallback, network, host home or writable Git.
fn shell(root: &Path, command: &str, cancelled: &AtomicBool) -> Result<Value, String> {
    let mut child = Command::new("/usr/bin/timeout");
    child
        .args([
            "--kill-after=2s",
            "50s",
            "/usr/bin/bwrap",
            "--unshare-all",
            "--die-with-parent",
            "--new-session",
            "--cap-drop",
            "ALL",
            "--clearenv",
            "--ro-bind",
            "/usr",
            "/usr",
            "--symlink",
            "usr/bin",
            "/bin",
            "--symlink",
            "usr/lib",
            "/lib",
            "--symlink",
            "usr/lib64",
            "/lib64",
            "--proc",
            "/proc",
            "--dev",
            "/dev",
            "--tmpfs",
            "/tmp",
            "--bind",
        ])
        .arg(root)
        .arg(root)
        .arg("--ro-bind")
        .arg(root.join(".git"))
        .arg(root.join(".git"));
    for name in [".gitignore", ".gitattributes", ".gitmodules"] {
        child
            .arg("--ro-bind-try")
            .arg(root.join(name))
            .arg(root.join(name));
    }
    let mut child = child
        .args([
            "--setenv",
            "PATH",
            "/usr/bin:/bin",
            "--setenv",
            "HOME",
            "/tmp",
            "--setenv",
            "LANG",
            "C.UTF-8",
            "--chdir",
        ])
        .arg(root)
        .args(["/bin/bash", "--noprofile", "--norc", "-c", command])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("confined shell unavailable: {e}"))?;
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let out = thread::spawn(move || capture(stdout));
    let err = thread::spawn(move || capture(stderr));
    let status = loop {
        if cancelled.load(Ordering::SeqCst) {
            // timeout forwards TERM to its process group; wait for it to reap the
            // confined process tree before releasing the snapshot gate.
            let _ = Command::new("/bin/kill")
                .args(["-TERM", &child.id().to_string()])
                .status();
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => thread::sleep(Duration::from_millis(20)),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error.to_string());
            }
        }
    };
    let stdout = out.join().unwrap_or_default();
    let stderr = err.join().unwrap_or_default();
    if cancelled.load(Ordering::SeqCst) {
        return Err("command cancelled; partial changes recorded".into());
    }
    if !status.success() {
        return Err(format!(
            "command failed ({status}); partial changes recorded\n{stdout}\n{stderr}"
        ));
    }
    Ok(json!({"stdout":stdout,"stderr":stderr,"exit_code":status.code()}))
}
