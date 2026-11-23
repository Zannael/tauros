//! Experimental, supervised interactive-CLI draft handoff; no agent bridge turn.
use serde_json::Value;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{atomic::{AtomicBool, Ordering}, mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

struct Job {
    child: Mutex<Child>,
    input: Mutex<ChildStdin>,
    events: Mutex<mpsc::Receiver<Value>>,
    finishing: AtomicBool,
}

impl Job {
    fn stop(&self) {
        let mut child = self.child.lock().unwrap();
        if child.try_wait().ok().flatten().is_some() { return; }
        // Let Python close the PTY/runtime and remove its private directory.
        let _ = Command::new("/bin/kill").args(["-TERM", &child.id().to_string()])
            .stdout(Stdio::null()).stderr(Stdio::null()).status();
        let deadline = Instant::now() + Duration::from_secs(4);
        while Instant::now() < deadline {
            if child.try_wait().ok().flatten().is_some() { return; }
            std::thread::sleep(Duration::from_millis(20));
        }
        let _ = child.kill();
        let _ = child.wait();
    }

    fn event(&self, timeout: Duration) -> Result<Value, String> {
        let value = self.events.lock().unwrap().recv_timeout(timeout)
            .map_err(|_| "Claude dictation timed out or closed. Check the official CLI sign-in and microphone.".to_owned())?;
        if let Some(error) = value["error"].as_str() { return Err(error.to_owned()); }
        Ok(value)
    }
}

impl Drop for Job {
    fn drop(&mut self) { self.stop(); }
}

#[derive(Default)]
pub struct ClaudeDictation {
    current: Mutex<Option<Arc<Job>>>,
}

impl ClaudeDictation {
    pub fn start(&self) -> Result<(), String> {
        let mut current = self.current.lock().unwrap();
        if current.is_some() { return Err("Claude dictation is already active.".into()); }
        let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../scripts/claude_dictation.py");
        let mut child = Command::new("/usr/bin/python3").arg(script)
            .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null())
            .spawn().map_err(|_| "Cannot start the experimental Claude dictation helper.".to_owned())?;
        let input = child.stdin.take().expect("piped stdin");
        let output = child.stdout.take().expect("piped stdout");
        let (send, events) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(output).lines() {
                let Ok(line) = line else { break; };
                if let Ok(value) = serde_json::from_str::<Value>(&line) {
                    if send.send(value).is_err() { break; }
                }
            }
        });
        let job = Arc::new(Job { child: Mutex::new(child), input: Mutex::new(input),
            events: Mutex::new(events), finishing: AtomicBool::new(false) });
        *current = Some(Arc::clone(&job));
        drop(current);
        match job.event(Duration::from_secs(60)) {
            Ok(value) if value["phase"] == "recording" => Ok(()),
            result => {
                self.clear(&job);
                job.stop();
                Err(result.err().unwrap_or_else(|| "Unexpected Claude dictation startup response.".into()))
            }
        }
    }

    pub fn finish(&self) -> Result<String, String> {
        let job = self.current.lock().unwrap().clone().ok_or("No Claude dictation is active.")?;
        if job.finishing.swap(true, Ordering::SeqCst) { return Err("Claude dictation is already finishing.".into()); }
        let result = (|| {
            // The helper may already have finalized its 60-second limit.
            let _ = writeln!(job.input.lock().unwrap(), "finish");
            let value = job.event(Duration::from_secs(25))?;
            value["text"].as_str().filter(|text| !text.trim().is_empty())
                .map(str::to_owned).ok_or_else(|| "Claude returned no dictated text.".to_owned())
        })();
        self.clear(&job);
        job.stop();
        result
    }

    fn clear(&self, job: &Arc<Job>) {
        let mut current = self.current.lock().unwrap();
        if current.as_ref().is_some_and(|value| Arc::ptr_eq(value, job)) { current.take(); }
    }

    pub fn cancel(&self) {
        let job = self.current.lock().unwrap().take();
        if let Some(job) = job { job.stop(); }
    }
}

impl Drop for ClaudeDictation {
    fn drop(&mut self) { self.cancel(); }
}
