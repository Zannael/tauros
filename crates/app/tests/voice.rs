//! Voice lifecycle uses original PCM and local process peers; no microphone/model call.
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauros_app::voice::{Voice, VoiceConfig};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "tauros-voice-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::create_dir(&dir).unwrap();
        Self(dir)
    }
    fn script(&self, name: &str, body: &str) -> PathBuf {
        let file = self.0.join(name);
        fs::write(&file, format!("#!/usr/bin/python3\n{body}\n")).unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(0o700)).unwrap();
        file
    }
    fn config(&self, behaviour: &str) -> VoiceConfig {
        let recorder = self.script(
            "recorder",
            "import os, time\nos.write(1, b'\\x00\\x20' * 16000)\ntime.sleep(10)",
        );
        let binary = self.script("whisper", &format!("import sys, pathlib, wave, time\na = sys.argv\nwith wave.open(a[a.index('--file')+1], 'rb') as w:\n assert w.getnchannels() == 1 and w.getframerate() == 16000 and w.getsampwidth() == 2\nassert a[a.index('--language')+1] == 'it'\n{behaviour}"));
        let model = self.0.join("model");
        fs::write(&model, b"original dummy model").unwrap();
        VoiceConfig {
            binary,
            model,
            recorder,
            device: "fixture".into(),
            temporary: self.0.clone(),
            timeout: Duration::from_secs(2),
        }
    }
    fn jobs(&self) -> Vec<PathBuf> {
        fs::read_dir(&self.0)
            .unwrap()
            .map(|x| x.unwrap().path())
            .filter(|p| p.is_dir())
            .collect()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn wait_for_pcm(f: &Fixture) {
    let start = Instant::now();
    while !f
        .jobs()
        .iter()
        .any(|dir| fs::metadata(dir.join("audio.pcm")).is_ok_and(|x| x.len() >= 32000))
    {
        assert!(start.elapsed() < Duration::from_secs(3));
        std::thread::sleep(Duration::from_millis(10));
    }
}
fn successful() -> &'static str {
    "pathlib.Path(a[a.index('--output-file')+1] + '.txt').write_text('Mostra la mappa.\\n')"
}

#[test]
fn local_pcm_becomes_text_and_every_job_cleans_up() {
    let f = Fixture::new();
    let voice = Voice::new(f.config(successful()));
    assert!(voice.status().ready);
    voice.start().unwrap();
    assert!(voice.start().is_err());
    wait_for_pcm(&f);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&f.jobs()[0]).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }
    assert_eq!(voice.finish("it").unwrap(), "Mostra la mappa.");
    assert!(f.jobs().is_empty());
    assert!(voice.finish("it").is_err());
    voice.start().unwrap();
    voice.cancel();
    assert!(f.jobs().is_empty());
    voice.start().unwrap();
    drop(voice);
    assert!(f.jobs().is_empty());
}

#[test]
fn failed_silent_and_missing_output_recordings_never_become_requests() {
    let f = Fixture::new();
    for mode in [
        "raise SystemExit(1)",
        "pass",
        "pathlib.Path(a[a.index('--output-file')+1] + '.txt').write_text('')",
    ] {
        let voice = Voice::new(f.config(mode));
        voice.start().unwrap();
        wait_for_pcm(&f);
        assert!(voice.finish("it").is_err());
        assert!(f.jobs().is_empty());
    }
    let mut config = f.config(successful());
    config.recorder = f.script(
        "silent",
        "import os, time\nos.write(1, b'\\x00' * 16000)\ntime.sleep(10)",
    );
    let voice = Voice::new(config);
    voice.start().unwrap();
    std::thread::sleep(Duration::from_millis(100));
    assert!(voice.finish("it").unwrap_err().contains("silent"));
    assert!(f.jobs().is_empty());
    let mut config = f.config(successful());
    config.model = Path::new("/nonexistent/tauros-model").into();
    let voice = Voice::new(config);
    assert!(!voice.status().ready);
    assert!(voice.start().is_err());
}

#[test]
fn transcription_cancel_and_timeout_leave_a_reusable_session() {
    let f = Fixture::new();
    let mut config = f.config("time.sleep(10)");
    config.timeout = Duration::from_millis(60);
    let voice = Voice::new(config);
    voice.start().unwrap();
    wait_for_pcm(&f);
    assert!(voice.finish("it").unwrap_err().contains("timed out"));
    assert!(f.jobs().is_empty());
    let voice = Arc::new(Voice::new(f.config("time.sleep(10)")));
    voice.start().unwrap();
    wait_for_pcm(&f);
    let worker = Arc::clone(&voice);
    let thread = std::thread::spawn(move || worker.finish("it"));
    let start = Instant::now();
    while !f.jobs().iter().any(|p| p.join("audio.wav").exists()) {
        assert!(start.elapsed() < Duration::from_secs(3));
        std::thread::sleep(Duration::from_millis(5));
    }
    voice.cancel();
    assert!(thread.join().unwrap().is_err());
    assert!(f.jobs().is_empty());
    voice.start().unwrap();
    voice.cancel();
    assert!(f.jobs().is_empty());
}

#[test]
#[ignore = "requires TAUROS_LOCAL_VOICE_TEST=1, installed pinned whisper/model, espeak-ng and ffmpeg; synthetic Italian speech only"]
fn installed_whisper_transcribes_original_italian_speech_offline() {
    use std::process::Command;
    assert_eq!(std::env::var("TAUROS_LOCAL_VOICE_TEST").as_deref(), Ok("1"));
    let f = Fixture::new();
    let wav = f.0.join("synthetic.wav");
    let pcm = f.0.join("synthetic.pcm");
    assert!(Command::new("espeak-ng")
        .args(["-v", "it", "-s", "135", "-w"])
        .arg(&wav)
        .arg("Questa è una prova di trascrizione in italiano.")
        .status()
        .unwrap()
        .success());
    assert!(Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(&wav)
        .args(["-ar", "16000", "-ac", "1", "-f", "s16le"])
        .arg(&pcm)
        .status()
        .unwrap()
        .success());
    let recorder = f.script("synthetic-recorder", &format!("import os, pathlib, time\nos.write(1, pathlib.Path({:?}).read_bytes())\ntime.sleep(10)", pcm.to_str().unwrap()));
    let config = VoiceConfig {
        recorder,
        temporary: f.0.clone(),
        ..VoiceConfig::default()
    };
    let voice = Voice::new(config);
    assert!(voice.status().ready);
    voice.start().unwrap();
    wait_for_pcm(&f);
    let text = voice.finish("it").unwrap();
    eprintln!("Local Italian transcription: {text}");
    // Require meaningful Italian content; synthetic speech is not an exact-word accuracy benchmark.
    let lower = text.to_lowercase();
    assert!(
        lower.contains("trascrizione") && lower.contains("italiano"),
        "{text}"
    );
    assert!(f.jobs().is_empty());
}
