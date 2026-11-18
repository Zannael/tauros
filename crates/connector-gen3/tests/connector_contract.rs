//! The Gen 3 connector against `git archive` copies of the pinned expansion: the
//! MOD-001 contract suite, lossless no-op writes, local trainer and map edits,
//! scenes and flag allocation (G3-002, G3-003, G3-004).

mod common;

use std::fs;
use std::path::Path;
use std::process::Command;
use tauros_connector_gen3::Gen3Connector;
use tauros_model::contract::assert_contract;
use tauros_model::*;

/// What the connector reads, without graphics and sound.
const PATHS: [&str; 4] = ["src", "include", "data", "tools/trainerproc"];

fn open(dir: &Path) -> Gen3Connector {
    Gen3Connector::open(dir, None).unwrap()
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git").arg("-C").arg(dir).args(args).output().unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Changed lines (`-`/`+`) of the last commit, without file headers.
fn last_diff(dir: &Path) -> Vec<String> {
    git(dir, &["show", "--format=", "-U0", "HEAD"])
        .lines()
        .filter(|l| (l.starts_with('-') || l.starts_with('+')) && !l.starts_with("---") && !l.starts_with("+++"))
        .map(str::to_owned)
        .collect()
}

#[test]
fn gen3_connector_satisfies_the_contract() {
    let Some(engine) = common::engine() else { return };
    // Each check gets a fresh copy; the copies live until the suite ends.
    let copies = std::cell::RefCell::new(Vec::new());
    assert_contract(|| {
        let dir = common::fixture(&engine, "g3-contract", &PATHS);
        let connector = open(dir.path());
        copies.borrow_mut().push(dir);
        connector
    });
}

#[test]
fn loading_and_rewriting_everything_is_lossless() {
    let Some(engine) = common::engine() else { return };
    let dir = common::fixture(&engine, "g3-noop", &PATHS);
    let mut c = open(dir.path());
    let snapshot = c.load().unwrap();
    assert_eq!(snapshot.game.engine, "pokeemerald-expansion 1.16.4");
    assert_eq!(snapshot.trainers.len(), 855);
    assert_eq!(snapshot.maps.len(), 939);
    assert!(snapshot.scenes.len() > 10_000);
    // flags.h holds only the Emerald branch's FLAG_ defines (FRLG's are in flags_frlg.h).
    assert_eq!(snapshot.flags.len(), 2301);
    let before = c.fingerprint().unwrap();
    let head = git(dir.path(), &["rev-parse", "HEAD"]);
    let mut set = ChangeSet::new("rewrite everything unchanged", Provenance::Human);
    for t in snapshot.trainers.values() {
        set = set.with(Change::UpsertTrainer { trainer: t.clone() });
    }
    for m in snapshot.maps.values() {
        set = set.with(Change::UpsertMap { map: m.clone() });
    }
    for s in snapshot.scenes.values().filter(|s| !matches!(s.label.0.as_str(), "MtChimney_EventScript_BagIsFull")) {
        set = set.with(Change::UpsertScene { scene: s.clone() });
    }
    let applied = c.apply(&set).unwrap();
    assert_eq!(applied, Applied { commit: None, touched: vec![] });
    assert_eq!(c.fingerprint().unwrap(), before);
    assert_eq!(git(dir.path(), &["rev-parse", "HEAD"]), head);
    assert!(git(dir.path(), &["status", "--porcelain"]).is_empty());
    eprintln!(
        "rewrote {} trainers, {} maps and {} scenes unchanged: no file touched, no commit",
        snapshot.trainers.len(),
        snapshot.maps.len(),
        set.changes.len() - snapshot.trainers.len() - snapshot.maps.len()
    );
}

#[test]
fn editing_one_trainer_changes_only_its_lines() {
    let Some(engine) = common::engine() else { return };
    let dir = common::fixture(&engine, "g3-trainer", &PATHS);
    let mut c = open(dir.path());
    let before = c.load().unwrap();
    let id = TrainerId::from("TRAINER_ROXANNE_1");
    let mut roxanne = before.trainers[&id].clone();
    roxanne.party.members[0].level += 1;
    roxanne.party.members[1].held_item = Some(ItemId::from("ITEM_ORAN_BERRY"));
    let applied = c
        .apply(&ChangeSet::new("Roxanne: level and berry", Provenance::AiAssisted).with(Change::UpsertTrainer { trainer: roxanne.clone() }))
        .unwrap();
    assert_eq!(applied.touched, ["src/data/trainers.party"]);
    let diff = last_diff(dir.path());
    eprintln!("diff: {diff:#?}");
    assert_eq!(diff.len(), 4, "two lines replaced: {diff:?}");
    assert!(diff.iter().any(|l| l.starts_with("+Level: ")));
    assert!(diff.iter().any(|l| l.contains(" @ Oran Berry")));
    let after = c.load().unwrap();
    assert_eq!(after.trainers[&id], roxanne);
    assert!(after.trainers.iter().all(|(k, t)| *k == id || before.trainers[k] == *t));
    let message = git(dir.path(), &["log", "-1", "--format=%B"]);
    assert!(message.contains("Tauros-Provenance: ai-assisted"));
}

#[test]
fn map_edits_are_local_and_rejections_untouched() {
    let Some(engine) = common::engine() else { return };
    let dir = common::fixture(&engine, "g3-map", &PATHS);
    let mut c = open(dir.path());
    let map = MapId::from("MAP_ROUTE101");
    let npc = Event::Npc {
        local_id: None,
        graphics: "OBJ_EVENT_GFX_BOY_1".into(),
        position: Position { x: 12, y: 10, elevation: Some(3) },
        movement: Some("MOVEMENT_TYPE_FACE_DOWN".into()),
        script: Some(ScriptLabel::from("Route101_EventScript_Boy")),
        visibility_flag: None,
        extensions: [
            ("movement_range_x".to_owned(), Value::Int(0)),
            ("movement_range_y".to_owned(), Value::Int(0)),
            ("trainer_type".to_owned(), Value::Text("TRAINER_TYPE_NONE".into())),
            ("trainer_sight_or_berry_tree_id".to_owned(), Value::Text("0".into())),
        ]
        .into(),
    };
    let applied = c
        .apply(&ChangeSet::new("Route 101: one more boy", Provenance::Human).with(Change::AddEvent { map: map.clone(), event: npc.clone() }))
        .unwrap();
    assert_eq!(applied.touched, ["data/maps/Route101/map.json"]);
    let diff = last_diff(dir.path());
    assert!(diff.iter().all(|l| l.starts_with('+')), "only additions: {diff:?}");
    // The previous last object gains a comma: git shows that as "    }," plus the
    // new object, all additions.
    assert_eq!(diff.len(), 13, "{diff:#?}");
    let loaded = c.load().unwrap();
    assert!(loaded.maps[&map].events.contains(&npc));
    // Removing it again restores the original bytes.
    let index = loaded.maps[&map].events.iter().filter(|e| e.kind() == EventKind::Npc).count() - 1;
    c.apply(&ChangeSet::new("remove it", Provenance::Human).with(Change::RemoveEvent { map: map.clone(), kind: EventKind::Npc, index }))
        .unwrap();
    assert!(git(dir.path(), &["diff", "HEAD~2", "--", "data/maps/Route101/map.json"]).is_empty());
    // A map change that the layout cannot hold is rejected without writes.
    let mut wider = loaded.maps[&map].clone();
    if let MapGeometry::Grid { width, .. } = &mut wider.geometry {
        *width += 1;
    }
    let print = c.fingerprint().unwrap();
    let err = c.apply(&ChangeSet::new("resize", Provenance::Human).with(Change::UpsertMap { map: wider })).unwrap_err();
    assert!(matches!(err, ConnectorError::Rejected(ref v) if v.iter().any(|v| v.rule == "map.layout_size")), "{err}");
    assert_eq!(c.fingerprint().unwrap(), print);
}

#[test]
fn flags_are_read_and_allocated_without_collisions() {
    let Some(engine) = common::engine() else { return };
    let dir = common::fixture(&engine, "g3-flags", &PATHS);
    let mut c = open(dir.path());
    let before = c.load().unwrap();
    let flags = &before.flags;
    let defeated = &flags[&FlagId::from("FLAG_DEFEATED_RUSTBORO_GYM")];
    assert!(defeated.in_use && defeated.value.is_some());
    let unused: Vec<&Flag> = flags.values().filter(|f| f.id.0.starts_with("FLAG_UNUSED_")).collect();
    eprintln!(
        "{} flags in the Emerald build, {} in use, {} FLAG_UNUSED_* placeholders ({} referenced somewhere)",
        flags.len(),
        flags.values().filter(|f| f.in_use).count(),
        unused.len(),
        unused.iter().filter(|f| f.in_use).count()
    );
    let mut values: Vec<u32> = flags.values().filter_map(|f| f.value).filter(|v| *v != 0).collect();
    let all = values.len();
    values.sort();
    values.dedup();
    eprintln!("{} non-zero values, {} distinct", all, values.len());

    let set = ChangeSet::new("flags for a scene", Provenance::AiGenerated)
        .with(Change::AllocateFlag { id: FlagId::from("FLAG_TAUROS_SCENE_SEEN") })
        .with(Change::AllocateFlag { id: FlagId::from("FLAG_TAUROS_SCENE_DONE") });
    let applied = c.apply(&set).unwrap();
    assert_eq!(applied.touched, ["include/constants/flags.h"]);
    let diff = last_diff(dir.path());
    assert_eq!(diff.len(), 4, "{diff:#?}");
    eprintln!("allocation diff: {diff:#?}");
    let after = c.load().unwrap();
    let seen = &after.flags[&FlagId::from("FLAG_TAUROS_SCENE_SEEN")];
    let done = &after.flags[&FlagId::from("FLAG_TAUROS_SCENE_DONE")];
    assert_ne!(seen.value, done.value);
    for new in [seen, done] {
        let v = new.value.unwrap();
        assert!((0x20..0x500).contains(&v), "{v:#x} outside the event flag range");
        let shared: Vec<_> = after.flags.values().filter(|f| f.value == Some(v) && f.id != new.id).collect();
        assert!(shared.is_empty(), "{} shares {v:#x} with {shared:?}", new.id);
        let old = before.flags.values().find(|f| f.value == Some(v)).unwrap();
        assert!(old.id.0.starts_with("FLAG_UNUSED_") && !old.in_use, "took {} which was in use", old.id);
    }
    // Every other flag keeps its value.
    for (id, f) in &before.flags {
        if let Some(g) = after.flags.get(id) {
            assert_eq!(g.value, f.value, "{id}");
        }
    }
    assert_eq!(before.flags.len(), after.flags.len());
    // Names already defined or referenced are rejected.
    let dup = ChangeSet::new("dup", Provenance::Human).with(Change::AllocateFlag { id: FlagId::from("FLAG_DEFEATED_RUSTBORO_GYM") });
    assert!(matches!(c.apply(&dup), Err(ConnectorError::Rejected(_))));
    let text = fs::read_to_string(dir.path().join("include/constants/flags.h")).unwrap();
    assert!(text.contains("FLAG_TAUROS_SCENE_SEEN"));
}

#[test]
fn assembly_scenes_are_edited_in_place() {
    let Some(engine) = common::engine() else { return };
    let dir = common::fixture(&engine, "g3-scene", &PATHS);
    let mut c = open(dir.path());
    let before = c.load().unwrap();
    let label = ScriptLabel::from("Route101_EventScript_Boy");
    let mut scene = before.scenes[&label].clone();
    assert_eq!(scene.language, ScriptLanguage::Assembly);
    assert_eq!(scene.map, Some(MapId::from("MAP_ROUTE101")));
    scene.source = scene.source.replace("MSGBOX_NPC", "MSGBOX_DEFAULT");
    let applied = c.apply(&ChangeSet::new("boy speaks", Provenance::Human).with(Change::UpsertScene { scene: scene.clone() })).unwrap();
    assert_eq!(applied.touched, ["data/maps/Route101/scripts.inc"]);
    assert_eq!(last_diff(dir.path()).len(), 2);
    assert_eq!(c.load().unwrap().scenes[&label], scene);
    // A new assembly scene is appended to its map's scripts.
    let new = Scene {
        label: ScriptLabel::from("Route101_EventScript_TaurosHello"),
        map: Some(MapId::from("MAP_ROUTE101")),
        language: ScriptLanguage::Assembly,
        source: "Route101_EventScript_TaurosHello::\n\tmsgbox Route101_Text_WhatAreYouDoing, MSGBOX_NPC\n\tend\n".into(),
    };
    c.apply(&ChangeSet::new("hello", Provenance::Human).with(Change::UpsertScene { scene: new.clone() })).unwrap();
    assert_eq!(c.load().unwrap().scenes[&new.label], new);
    c.apply(&ChangeSet::new("bye", Provenance::Human).with(Change::RemoveScene { label: new.label.clone() })).unwrap();
    // Adding then removing the scene leaves scripts.inc as after the first edit.
    assert!(git(dir.path(), &["diff", "HEAD~2", "HEAD", "--", "data/maps/Route101/scripts.inc"]).is_empty());
    // A scene that would redefine its label twice is rejected.
    let mut bad = scene.clone();
    bad.source.push_str("Route101_EventScript_Other::\n\tend\n");
    assert!(matches!(c.apply(&ChangeSet::new("bad", Provenance::Human).with(Change::UpsertScene { scene: bad })), Err(ConnectorError::Rejected(_))));
}
