//! APP-001: the Maps view's data for a real map, read-only, from the pinned engine.

use std::fs;
use std::path::PathBuf;

use base64::Engine;
use tauros_app::map_view::{map_view, read_map};
use tauros_connector_gen3::Gen3Connector;
use std::sync::OnceLock;
use tauros_model::{Connector, Event, MapId, ProjectSnapshot};

fn engine() -> Option<PathBuf> {
    let data = std::env::var_os("TAUROS_DATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share/tauros")))?;
    let found = fs::read_dir(data.join("engines")).ok()?.flatten().map(|e| e.path()).find(|p| {
        p.is_dir() && p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("pokeemerald-expansion-") && !n.ends_with("-base"))
    });
    if found.is_none() {
        eprintln!("SKIPPED: no pinned pokeemerald-expansion under {}", data.display());
    }
    found
}

/// One connector open per test binary: opening sets the clone's push URL, and two
/// tests doing it at once collide on `.git/config`'s lock.
fn snapshot(dir: &PathBuf) -> &'static ProjectSnapshot {
    static SNAPSHOT: OnceLock<ProjectSnapshot> = OnceLock::new();
    SNAPSHOT.get_or_init(|| Gen3Connector::open(dir, None).unwrap().load().unwrap())
}

#[test]
fn route101_is_drawn_with_its_blocks_events_and_behaviours() {
    let Some(dir) = engine() else { return };
    let before = std::process::Command::new("git").arg("-C").arg(&dir).args(["status", "--porcelain"]).output().unwrap().stdout;
    let map = &snapshot(&dir).maps[&MapId::from("MAP_ROUTE101")];
    let started = std::time::Instant::now();
    let view = map_view(&dir, map).unwrap();
    eprintln!("map view {:?}", started.elapsed());
    assert_eq!((view.width, view.height), (20, 20));
    assert_eq!(view.metatiles.len(), 400);
    assert!(view.problems.is_empty(), "{:?}", view.problems);
    assert_eq!(view.refusal, None);
    assert_eq!((view.primary_tileset.as_str(), view.secondary_tileset.as_str()), ("gTileset_General", "gTileset_Petalburg"));
    // The picture is a PNG of 16 px per block.
    let png = base64::engine::general_purpose::STANDARD.decode(view.image.strip_prefix("data:image/png;base64,").unwrap()).unwrap();
    let info = png::Decoder::new(std::io::Cursor::new(png)).read_info().unwrap().info().clone();
    assert_eq!((info.width, info.height), (320, 320));
    // Route 101 has tall grass, solid trees and the model's events.
    assert!(view.behavior_names.values().any(|n| n == "MB_TALL_GRASS"), "{:?}", view.behavior_names);
    assert!(view.collision.iter().any(|c| *c != 0) && view.collision.contains(&0));
    assert_eq!(view.events, map.events);
    assert!(view.events.iter().any(|e| matches!(e, Event::Npc { .. })));
    assert_eq!(view.connections.len(), map.connections.len());
    // Nothing was written.
    let after = std::process::Command::new("git").arg("-C").arg(&dir).args(["status", "--porcelain"]).output().unwrap().stdout;
    assert_eq!(before, after);
}

#[test]
fn a_frlg_map_is_drawn_and_reports_its_refusal() {
    let Some(dir) = engine() else { return };
    let snapshot = snapshot(&dir);
    let Some(map) = snapshot.maps.values().find(|m| m.name == "PalletTown_Frlg") else { return };
    let view = map_view(&dir, map).unwrap();
    assert!(view.refusal.as_deref().is_some_and(|r| r.contains("REGION_KANTO")), "{:?}", view.refusal);
    assert!(view.problems.is_empty(), "{:?}", view.problems);
}

#[test]
fn one_map_read_alone_equals_the_connector_snapshot() {
    let Some(dir) = engine() else { return };
    let snapshot = snapshot(&dir);
    let started = std::time::Instant::now();
    for map in snapshot.maps.values() {
        assert_eq!(&read_map(&dir, &map.name).unwrap(), map, "{}", map.name);
    }
    eprintln!("{} maps read one by one in {:?}", snapshot.maps.len(), started.elapsed());
    assert!(read_map(&dir, "NoSuchMap").is_err());
}
