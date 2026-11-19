//! EMU-005: CLI -> project -> boot plan -> desktop worker -> Tauri frame channel.
//! Opt in with TAUROS_DOCKER_TESTS=1. No display server is needed; the Owner's
//! window check remains separate. Uses only a disposable copy of the base build.
#[path = "../../connector-gen3/tests/common/mod.rs"]
mod common;

use std::sync::mpsc;
use std::time::Duration;
use tauri::ipc::{Channel, InvokeResponseBody};
use tauros_app::launch::Launch;
use tauros_app::playback::{emulator_thread, Command};
use tauros_connector_gen3::boot::Location;

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
        let worker = std::thread::spawn(move || emulator_thread(rx));
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
        drop(tx);
        worker.join().unwrap();
    }
    assert_ne!(pictures[0], pictures[1], "changing the target must change the displayed map");
    // A plain ROM still uses the same worker, without a boot plan.
    let launch = Launch::parse([dir.path().join("pokeemerald.gba").into_os_string()]).unwrap().unwrap();
    let prepared = launch.prepare().unwrap();
    assert!(prepared.boot.is_none());
    let (tx, rx) = mpsc::channel();
    let worker = std::thread::spawn(move || emulator_thread(rx));
    let (reply, result) = mpsc::channel();
    tx.send(Command::Load(prepared, reply)).unwrap();
    assert!(result.recv_timeout(Duration::from_secs(5)).unwrap().unwrap().is_none());
    drop(tx);
    worker.join().unwrap();
}
