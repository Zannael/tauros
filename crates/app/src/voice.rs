//! Offline push-to-talk dictation. Audio never reaches the agent bridge.
//!
//! Linux `arecord` produces bounded raw PCM; whisper.cpp reads a private WAV and
//! returns a draft. Dropping/cancelling a job kills its child and removes audio.
use serde::Serialize;
use std::fs::{self, File};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const MAX_SECONDS: usize = 60;
const MAX_PCM: usize = MAX_SECONDS * 16_000 * 2;

#[derive(Clone)]
pub struct VoiceConfig {
    pub binary: PathBuf,
    pub model: PathBuf,
    pub recorder: PathBuf,
    pub device: String,
    pub temporary: PathBuf,
    pub timeout: Duration,
}

impl Default for VoiceConfig {
    fn default() -> Self {
        let data = std::env::var_os("TAUROS_DATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
                    .join(".local/share/tauros")
            });
        Self {
            binary: std::env::var_os("TAUROS_WHISPER_BIN")
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    data.join("dependencies/whisper.cpp-v1.8.2/build/bin/whisper-cli")
                }),
            model: std::env::var_os("TAUROS_WHISPER_MODEL")
                .map(PathBuf::from)
                .unwrap_or_else(|| data.join("models/whisper/ggml-base.bin")),
            recorder: PathBuf::from("/usr/bin/arecord"),
            device: std::env::var("TAUROS_AUDIO_DEVICE").unwrap_or_else(|_| "default".into()),
            temporary: std::env::temp_dir(),
            timeout: Duration::from_secs(180),
        }
    }
}

#[derive(Serialize)]
pub struct VoiceStatus {
    pub ready: bool,
    pub message: String,
    pub max_seconds: usize,
}

struct Job {
    dir: PathBuf,
    child: Mutex<Option<Child>>,
    cancelled: AtomicBool,
    finishing: AtomicBool,
}

impl Job {
    fn stop_child(&self) {
        if let Some(mut child) = self.child.lock().unwrap().take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        self.stop_child();
        let _ = fs::remove_dir_all(&self.dir);
    }
}

pub struct Voice {
    config: VoiceConfig,
    closed: AtomicBool,
    current: Mutex<Option<Arc<Job>>>,
}

impl Default for Voice {
    fn default() -> Self {
        Self::new(VoiceConfig::default())
    }
}

impl Voice {
    pub fn new(config: VoiceConfig) -> Self {
        Self {
            config,
            closed: AtomicBool::new(false),
            current: Mutex::new(None),
        }
    }

    pub fn status(&self) -> VoiceStatus {
        let message = if !cfg!(target_os = "linux") {
            "Voice capture currently requires Linux."
        } else if !self.config.recorder.is_file() {
            "Microphone capture needs /usr/bin/arecord (alsa-utils)."
        } else if !self.config.binary.is_file() || !self.config.model.is_file() {
            "Local transcription is not installed. Run python3 scripts/setup_voice.py, then retry."
        } else {
            "Ready. Audio is transcribed locally; review the draft before sending."
        };
        VoiceStatus {
            ready: message.starts_with("Ready."),
            message: message.into(),
            max_seconds: MAX_SECONDS,
        }
    }

    pub fn start(&self) -> Result<(), String> {
        let mut current = self.current.lock().unwrap();
        if self.closed.load(Ordering::SeqCst) { return Err("Tauros is shutting down.".into()); }
        if current.is_some() {
            return Err("Voice input is already active.".into());
        }
        let status = self.status();
        if !status.ready {
            return Err(status.message);
        }
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let dir = self.config.temporary.join(format!(
            "tauros-voice-{}-{stamp}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let mut builder = fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder
            .create(&dir)
            .map_err(|_| "Cannot create private recording storage.".to_owned())?;
        let job = Arc::new(Job {
            dir,
            child: Mutex::new(None),
            cancelled: AtomicBool::new(false),
            finishing: AtomicBool::new(false),
        });
        let pcm = File::create(job.dir.join("audio.pcm"))
            .map_err(|_| "Cannot create recording.".to_owned())?;
        let child = Command::new(&self.config.recorder)
            .args(["-q", "-D", &self.config.device, "-t", "raw", "-f", "S16_LE", "-r", "16000", "-c", "1", "-d", &MAX_SECONDS.to_string()])
            .stdin(Stdio::null()).stdout(pcm).stderr(Stdio::null()).spawn()
            .map_err(|error| format!("Cannot start the microphone recorder ({:?}). Check the audio device and permissions.", error.kind()))?;
        *job.child.lock().unwrap() = Some(child);
        *current = Some(job);
        Ok(())
    }

    /// May run on a worker thread; cancel remains callable throughout inference.
    pub fn finish(&self, language: &str) -> Result<String, String> {
        if !["it", "en", "auto"].contains(&language) {
            return Err("Choose Italian, English or automatic language.".into());
        }
        let job = self
            .current
            .lock()
            .unwrap()
            .clone()
            .ok_or("No recording is active.")?;
        if job.finishing.swap(true, Ordering::SeqCst) {
            return Err("Transcription is already running.".into());
        }
        let result = self.transcribe(&job, language);
        let mut current = self.current.lock().unwrap();
        if current
            .as_ref()
            .is_some_and(|value| Arc::ptr_eq(value, &job))
        {
            current.take();
        }
        result
    }

    fn transcribe(&self, job: &Job, language: &str) -> Result<String, String> {
        // A recorder that exited with an error must not turn partial PCM into a
        // seemingly successful request. Killing a running recorder is intentional.
        {
            let mut child = job.child.lock().unwrap();
            if let Some(child) = child.as_mut() {
                if let Some(status) = child
                    .try_wait()
                    .map_err(|_| "Cannot inspect microphone recorder.")?
                {
                    if !status.success() {
                        return Err("Microphone recording failed. Check the default input device or TAUROS_AUDIO_DEVICE.".into());
                    }
                }
            }
        }
        job.stop_child();
        if job.cancelled.load(Ordering::SeqCst) {
            return Err("Voice input cancelled.".into());
        }
        let raw = job.dir.join("audio.pcm");
        let size = fs::metadata(&raw)
            .map_err(|_| "Recording is unavailable.")?
            .len() as usize;
        if size < 3200 {
            return Err("No audio captured. Check the microphone and record for longer.".into());
        }
        if size > MAX_PCM {
            return Err("Recording exceeded the 60-second limit.".into());
        }
        let pcm = fs::read(raw).map_err(|_| "Cannot read recording.")?;
        // Prevent a silent clip from producing Whisper hallucinations.
        if !pcm
            .chunks_exact(2)
            .any(|sample| i16::from_le_bytes([sample[0], sample[1]]).unsigned_abs() > 100)
        {
            return Err(
                "No speech detected: the recording is silent. Check the input device.".into(),
            );
        }
        let wav = job.dir.join("audio.wav");
        fs::write(&wav, pcm_wav(&pcm)?).map_err(|_| "Cannot prepare audio for transcription.")?;
        let output = job.dir.join("transcript");
        {
            let mut slot = job.child.lock().unwrap();
            if job.cancelled.load(Ordering::SeqCst) {
                return Err("Voice input cancelled.".into());
            }
            *slot = Some(
                Command::new(&self.config.binary)
                    .arg("--model")
                    .arg(&self.config.model)
                    .arg("--file")
                    .arg(&wav)
                    .args(["--language", language, "--output-txt", "--output-file"])
                    .arg(&output)
                    .args(["--no-prints", "--no-gpu", "--threads", "4"])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .map_err(|_| "Cannot start local whisper.cpp. Check its installation.")?,
            );
        }
        let started = Instant::now();
        loop {
            if job.cancelled.load(Ordering::SeqCst) {
                return Err("Voice input cancelled.".into());
            }
            if started.elapsed() > self.config.timeout {
                job.stop_child();
                return Err("Local transcription timed out. Try a shorter recording.".into());
            }
            let status = {
                let mut slot = job.child.lock().unwrap();
                match slot.as_mut() {
                    Some(child) => child
                        .try_wait()
                        .map_err(|_| "Cannot inspect local transcription.")?,
                    None => return Err("Voice input cancelled.".into()),
                }
            };
            if let Some(status) = status {
                if !status.success() {
                    return Err(
                        "Local transcription failed. Check the whisper.cpp model and retry.".into(),
                    );
                }
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        let path = output.with_extension("txt");
        if fs::metadata(&path)
            .map_err(|_| "Local transcription produced no text output.")?
            .len()
            > 64 * 1024
        {
            return Err("Local transcription output was unexpectedly large.".into());
        }
        let text =
            fs::read_to_string(path).map_err(|_| "Cannot read local transcription output.")?;
        let text = text.trim();
        if text.is_empty() {
            return Err("No speech recognised. Try again closer to the microphone.".into());
        }
        Ok(text.to_owned())
    }

    pub fn shutdown(&self) {
        self.closed.store(true, Ordering::SeqCst);
        self.cancel();
    }

    pub fn cancel(&self) {
        if let Some(job) = self.current.lock().unwrap().take() {
            job.cancelled.store(true, Ordering::SeqCst);
            job.stop_child();
            // A transcribing worker may still retain the job. Remove private
            // audio now rather than relying on its final Arc being dropped.
            let _ = fs::remove_dir_all(&job.dir);
        }
    }
}

impl Drop for Voice {
    fn drop(&mut self) {
        self.cancel();
    }
}

/// Canonical 16 kHz mono PCM WAV, independent of recorder shutdown/header repair.
fn pcm_wav(pcm: &[u8]) -> Result<Vec<u8>, String> {
    if pcm.len() > MAX_PCM || !pcm.len().is_multiple_of(2) {
        return Err("Invalid recording length.".into());
    }
    let mut wav = Vec::with_capacity(44 + pcm.len());
    wav.extend(b"RIFF");
    wav.extend((36 + pcm.len() as u32).to_le_bytes());
    wav.extend(b"WAVEfmt ");
    wav.extend(16u32.to_le_bytes());
    wav.extend(1u16.to_le_bytes());
    wav.extend(1u16.to_le_bytes());
    wav.extend(16_000u32.to_le_bytes());
    wav.extend(32_000u32.to_le_bytes());
    wav.extend(2u16.to_le_bytes());
    wav.extend(16u16.to_le_bytes());
    wav.extend(b"data");
    wav.extend((pcm.len() as u32).to_le_bytes());
    wav.extend(pcm);
    Ok(wav)
}
