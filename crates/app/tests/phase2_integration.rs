//! EMU-005: CLI -> project -> boot plan -> desktop worker -> Tauri frame channel.
//! Opt in with TAUROS_DOCKER_TESTS=1. No display server is needed; the Owner's
//! window check remains separate. Uses only a disposable copy of the base build.
#[path = "../../connector-gen3/tests/common/mod.rs"]
mod common;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;
use tauri::ipc::{Channel, InvokeResponseBody};
use tauros_app::launch::Launch;
use tauros_app::playback::{emulator_thread, Command};
use tauros_connector_gen3::boot::Location;
use tauros_emu::Mgba;
use tauros_mcp::emulator::EmuSession;

fn new_session() -> Arc<Mutex<EmuSession>> {
    Arc::new(Mutex::new(EmuSession::new(Mgba::gba().unwrap())))
}

fn frame_count(session: &Arc<Mutex<EmuSession>>) -> u32 {
    session.lock().unwrap().emulator().frame_count()
}

#[test]
fn chosen_point_reaches_the_window_channel() {
    if std::env::var_os("TAUROS_DOCKER_TESTS").is_none() {
        eprintln!("SKIPPED: set TAUROS_DOCKER_TESTS=1 to run the Phase 2 integration test");
        return;
    }
    let engine = common::engine().expect("pinned engine required for opted-in test");
    let dir = common::built_fixture(&engine, "phase2").expect("base build required for opted-in test");
    let project = dir.path().as_os_str().to_owned();
    let parse = |map: &str, x: &str, flag: bool| {
        let mut args =
            vec!["--project".into(), project.clone(), "--map".into(), map.into(), "--x".into(), x.into(), "--y".into(), "10".into()];
        if flag {
            args.extend(["--flag".into(), "FLAG_BADGE01_GET".into()]);
        }
        Launch::parse(args).unwrap().unwrap()
    };
    assert!(Launch::parse(["--project", "missing", "--map"].map(Into::into)).is_err());
    assert!(parse("MAP_NOWHERE", "15", false).prepare().is_err());
    assert!(parse("MAP_PETALBURG_CITY", "1000", false).prepare().is_err());

    let mut pictures = Vec::new();
    for (map, x, flag, want) in [
        ("MAP_PETALBURG_CITY", "15", true, Location { group: 0, num: 0, x: 15, y: 10 }),
        ("MAP_ROUTE101", "10", false, Location { group: 0, num: 16, x: 10, y: 10 }),
    ] {
        let prepared = parse(map, x, flag).prepare().unwrap();
        assert!(prepared.name.contains(map));
        let (tx, rx) = mpsc::channel();
        let (session, hold) = (new_session(), Arc::new(AtomicBool::new(false)));
        let (worker_session, worker_hold) = (session.clone(), hold.clone());
        let worker = std::thread::spawn(move || emulator_thread(rx, worker_session, move || worker_hold.load(Ordering::SeqCst)));
        let (frames, received) = mpsc::channel();
        let channel = Channel::new(move |body| {
            if let InvokeResponseBody::Raw(bytes) = body {
                let _ = frames.send(bytes);
            }
            Ok(())
        });
        tx.send(Command::Attach(channel)).unwrap();
        let (reply, result) = mpsc::channel();
        tx.send(Command::Load(prepared, reply)).unwrap();
        let report = result.recv_timeout(Duration::from_secs(30)).unwrap().unwrap().expect("boot must have run");
        eprintln!("{map}: {report:?}");
        assert_eq!(report.location, want);
        let bytes = received.recv_timeout(Duration::from_secs(5)).unwrap();
        const VIDEO: usize = 240 * 160 * 4;
        // Binary contract consumed by ui/main.js: RGBA then interleaved LE i16 audio.
        assert!(bytes.len() > VIDEO && bytes.len() < VIDEO + 4096, "one frame of audio, not the skipped boot's backlog");
        assert_eq!((bytes.len() - VIDEO) % 4, 0);
        assert!(bytes[..VIDEO].chunks_exact(4).all(|p| p[3] == 255));
        assert!(bytes[..VIDEO].chunks_exact(4).any(|p| p != &bytes[..4]), "map is not a uniform screen");
        pictures.push(bytes[..VIDEO].to_vec());
        // While the agent holds the game the window still gets frames (video only)
        // but the shared session does not advance; play resumes after the hold.
        hold.store(true, Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(100));
        while received.try_recv().is_ok() {}
        let held_at = frame_count(&session);
        let held_frame = received.recv_timeout(Duration::from_secs(2)).expect("frames keep coming while held");
        assert_eq!(held_frame.len(), VIDEO, "no audio while held");
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(frame_count(&session), held_at, "the window does not run frames while held");
        hold.store(false, Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(300));
        assert!(frame_count(&session) > held_at + 5, "play resumes after the hold");
        drop(tx);
        worker.join().unwrap();
    }
    assert_ne!(pictures[0], pictures[1], "changing the target must change the displayed map");
    // A plain ROM still uses the same worker, without a boot plan.
    let launch = Launch::parse([dir.path().join("pokeemerald.gba").into_os_string()]).unwrap().unwrap();
    let prepared = launch.prepare().unwrap();
    assert!(prepared.boot.is_none());
    let (tx, rx) = mpsc::channel();
    let worker = std::thread::spawn(move || emulator_thread(rx, new_session(), || false));
    let (reply, result) = mpsc::channel();
    tx.send(Command::Load(prepared, reply)).unwrap();
    assert!(result.recv_timeout(Duration::from_secs(5)).unwrap().unwrap().is_none());
    drop(tx);
    worker.join().unwrap();
}
