//! Map generator view (GFX-009): runs `scripts/mapgen.py` jobs and streams their JSON-line events.
//!
//! One generation job runs at a time in its own process group, so stopping it also stops the
//! image and Claude CLI calls it started. Read-only commands (`list`, `state`) run alongside.
//! Images are served only from the mapgen data folder.
use serde_json::Value;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Commands that change a run; at most one is active.
const JOBS: [&str; 7] = ["create", "settings", "layout", "accept", "inventory", "sheet", "sheets"];
const READS: [&str; 2] = ["list", "state"];

pub struct MapGen {
    script: PathBuf,
    closed: AtomicBool,
    current: Mutex<Option<Arc<Mutex<Child>>>>,
}

impl Default for MapGen {
    fn default() -> Self {
        Self::with_script(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../scripts/mapgen.py"))
    }
}

pub fn data_root() -> PathBuf {
    let base = std::env::var_os("TAUROS_DATA").map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/share/tauros")
    });
    base.join("mapgen")
}

fn spawn(script: &Path, args: &[String]) -> Result<Child, String> {
    let mut command = Command::new("/usr/bin/python3");
    command.arg(script).args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    command.spawn().map_err(|_| "Cannot start the map generator script.".to_owned())
}

/// TERM the job's group, give the script time to save its state, then KILL what is left.
fn stop(child: &Mutex<Child>) {
    let mut child = child.lock().unwrap();
    if child.try_wait().ok().flatten().is_some() { return; }
    let group = format!("-{}", child.id());
    let signal = |name: &str| {
        let _ = Command::new("/bin/kill").args([name, "--", &group])
            .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status();
    };
    signal("-TERM");
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if child.try_wait().ok().flatten().is_some() { break; }
        std::thread::sleep(Duration::from_millis(25));
    }
    signal("-KILL");
    let _ = child.kill();
    let _ = child.wait();
}

impl MapGen {
    pub fn with_script(script: PathBuf) -> Self {
        Self { script, closed: AtomicBool::new(false), current: Mutex::new(None) }
    }

    pub fn busy(&self) -> bool {
        self.current.lock().unwrap().is_some()
    }

    /// Runs a job, passing every JSON event to `on_event`; returns the final `done` event.
    pub fn run(&self, args: Vec<String>, on_event: impl Fn(Value)) -> Result<Value, String> {
        let first = args.first().map(String::as_str).unwrap_or_default();
        if READS.contains(&first) { return self.read(args); }
        if !JOBS.contains(&first) { return Err(format!("Unknown map generator command: {first}")); }
        let child = {
            let mut current = self.current.lock().unwrap();
            if self.closed.load(Ordering::SeqCst) { return Err("Tauros is shutting down.".into()); }
            if current.is_some() { return Err("Another map generator step is running.".into()); }
            let child = Arc::new(Mutex::new(spawn(&self.script, &args)?));
            *current = Some(Arc::clone(&child));
            child
        };
        let output = child.lock().unwrap().stdout.take().expect("piped stdout");
        let result = collect(output, on_event);
        {
            let mut current = self.current.lock().unwrap();
            if current.as_ref().is_some_and(|value| Arc::ptr_eq(value, &child)) { current.take(); }
        }
        let _ = child.lock().unwrap().wait();
        result
    }

    fn read(&self, args: Vec<String>) -> Result<Value, String> {
        let mut child = spawn(&self.script, &args)?;
        let output = child.stdout.take().expect("piped stdout");
        let result = collect(output, |_| {});
        let _ = child.wait();
        result
    }

    pub fn cancel(&self) {
        let job = self.current.lock().unwrap().clone();
        if let Some(job) = job { stop(&job); }
    }

    pub fn shutdown(&self) {
        self.closed.store(true, Ordering::SeqCst);
        self.cancel();
    }

    /// Bytes of an image inside the mapgen data folder; anything else is refused.
    pub fn image(&self, path: &str) -> Result<Vec<u8>, String> {
        image_in(&data_root(), path)
    }
}

impl Drop for MapGen {
    fn drop(&mut self) { self.cancel(); }
}

fn collect(output: impl std::io::Read, on_event: impl Fn(Value)) -> Result<Value, String> {
    let mut last_error = None;
    for line in BufReader::new(output).lines() {
        let Ok(line) = line else { break };
        let Ok(value) = serde_json::from_str::<Value>(&line) else { continue };
        match value["event"].as_str() {
            Some("done") => return Ok(value),
            Some("error") => last_error = value["message"].as_str().map(str::to_owned),
            _ => on_event(value),
        }
    }
    Err(last_error.unwrap_or_else(|| "The map generator stopped.".into()))
}

pub fn image_in(root: &Path, path: &str) -> Result<Vec<u8>, String> {
    let refused = || "That image is outside the map generator folder.".to_owned();
    let root = root.canonicalize().map_err(|_| refused())?;
    let file = Path::new(path).canonicalize().map_err(|_| "Image not found.".to_owned())?;
    if !file.starts_with(&root) || file.extension().and_then(|e| e.to_str()) != Some("png") {
        return Err(refused());
    }
    std::fs::read(file).map_err(|_| "Image not readable.".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tauros-mapgen-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn images_are_served_only_from_the_mapgen_folder() {
        let dir = temp("images");
        let root = dir.join("mapgen");
        fs::create_dir_all(root.join("runs/a")).unwrap();
        fs::write(root.join("runs/a/sheet.png"), b"png").unwrap();
        fs::write(dir.join("outside.png"), b"png").unwrap();
        fs::write(root.join("openai.env"), b"secret").unwrap();
        assert_eq!(image_in(&root, root.join("runs/a/sheet.png").to_str().unwrap()).unwrap(), b"png");
        assert!(image_in(&root, dir.join("outside.png").to_str().unwrap()).is_err());
        assert!(image_in(&root, root.join("runs/a/../../../outside.png").to_str().unwrap()).is_err());
        assert!(image_in(&root, root.join("openai.env").to_str().unwrap()).is_err());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn jobs_stream_events_refuse_overlap_and_stop_on_shutdown() {
        let dir = temp("jobs");
        let script = dir.join("fake.py");
        fs::write(&script, "import json,sys,time\nprint(json.dumps({'event':'progress','message':sys.argv[1]}),flush=True)\n\
if sys.argv[1]=='sheets': time.sleep(60)\nprint(json.dumps({'event':'done','ok':True}),flush=True)\n").unwrap();
        let gen = Arc::new(MapGen::with_script(script));
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&seen);
        let done = gen.run(vec!["layout".into(), "r".into()], move |v| log.lock().unwrap().push(v)).unwrap();
        assert_eq!(done["ok"], true);
        assert_eq!(seen.lock().unwrap()[0]["message"], "layout");
        assert!(gen.run(vec!["rm".into()], |_| {}).unwrap_err().contains("Unknown"));
        let peer = Arc::clone(&gen);
        let long = std::thread::spawn(move || peer.run(vec!["sheets".into(), "r".into()], |_| {}));
        let deadline = Instant::now() + Duration::from_secs(5);
        while !gen.busy() { assert!(Instant::now() < deadline); std::thread::sleep(Duration::from_millis(10)); }
        assert!(gen.run(vec!["layout".into()], |_| {}).unwrap_err().contains("Another"));
        assert!(gen.run(vec!["list".into()], |_| {}).is_ok(), "reads run alongside a job");
        gen.shutdown();
        assert!(long.join().unwrap().is_err());
        assert!(!gen.busy());
        assert!(gen.run(vec!["layout".into()], |_| {}).unwrap_err().contains("shutting down"));
        fs::remove_dir_all(dir).unwrap();
    }
}
