//! G3-010: new maps, new layouts and layout resizing against `git archive` copies
//! of the pinned expansion. Untouched files stay byte-identical, rejected sets
//! write nothing, and the build test (container, `TAUROS_DOCKER_TESTS=1`) compiles
//! a created and a resized map.

mod common;

use std::fs;
use std::path::Path;
use std::process::Command;
use tauros_connector_gen3::Gen3Connector;
use tauros_model::*;

const PATHS: [&str; 4] = ["src", "include", "data", "tools/trainerproc"];

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git").arg("-C").arg(dir).args(args).output().unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn layout_props(pairs: &[(&str, Value)]) -> Extensions {
    pairs.iter().map(|(k, v)| ((*k).to_owned(), v.clone())).collect()
}

fn new_map(id: &str, name: &str, layout: &str, size: (u16, u16)) -> Map {
    Map {
        id: MapId::from(id),
        name: name.into(),
        geometry: MapGeometry::Grid { layout: LayoutId::from(layout), width: size.0, height: size.1 },
        events: vec![Event::Warp {
            position: Position { x: 3, y: 4, elevation: Some(0) },
            destination_map: MapId::from("MAP_ROUTE101"),
            destination_warp: 0,
            extensions: Extensions::new(),
        }],
        connections: vec![],
        properties: layout_props(&[
            ("music", Value::Text("MUS_ROUTE101".into())),
            ("map_type", Value::Text("MAP_TYPE_ROUTE".into())),
            ("allow_running", Value::Bool(true)),
        ]),
    }
}

fn create_set() -> ChangeSet {
    ChangeSet::new("Add Tauros Meadow", Provenance::AiAssisted)
        .with(Change::CreateLayout { layout: LayoutId::from("LAYOUT_TAUROS_MEADOW"), width: 16, height: 12, properties: Extensions::new() })
        .with(Change::UpsertMap { map: new_map("MAP_TAUROS_MEADOW", "TaurosMeadow", "LAYOUT_TAUROS_MEADOW", (16, 12)) })
}

fn words(path: &Path) -> Vec<u16> {
    fs::read(path).unwrap().chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect()
}

fn rejected(c: &mut Gen3Connector, set: ChangeSet, rule: &str) {
    let print = c.fingerprint().unwrap();
    let err = c.apply(&set).unwrap_err();
    assert!(matches!(err, ConnectorError::Rejected(ref v) if v.iter().any(|v| v.rule == rule)), "expected {rule}: {err}");
    assert_eq!(c.fingerprint().unwrap(), print, "a rejected change set wrote files");
}

#[test]
fn a_created_map_is_a_complete_porymap_map() {
    let Some(engine) = common::engine() else { return };
    let dir = common::fixture(&engine, "g3-newmap", &PATHS);
    let mut c = Gen3Connector::open(dir.path(), None).unwrap();
    let applied = c.apply(&create_set()).unwrap();
    assert_eq!(
        applied.touched,
        [
            "data/event_scripts.s",
            "data/layouts/TaurosMeadow/border.bin",
            "data/layouts/TaurosMeadow/map.bin",
            "data/layouts/layouts.json",
            "data/maps/TaurosMeadow/map.json",
            "data/maps/TaurosMeadow/scripts.inc",
            "data/maps/map_groups.json",
        ]
    );
    // Only additions: nothing existing was rewritten.
    let diff = git(dir.path(), &["show", "--format=", "-U0", "HEAD"]);
    let removed: Vec<&str> = diff.lines().filter(|l| l.starts_with('-') && !l.starts_with("---")).collect();
    // The array's last element gains a comma, nothing else disappears.
    assert!(removed.len() <= 2, "{removed:?}");
    let map_json = fs::read_to_string(dir.path().join("data/maps/TaurosMeadow/map.json")).unwrap();
    assert!(map_json.starts_with("{\n  \"id\": \"MAP_TAUROS_MEADOW\",\n  \"name\": \"TaurosMeadow\",\n  \"layout\": \"LAYOUT_TAUROS_MEADOW\",\n  \"music\": \"MUS_ROUTE101\",\n"));
    assert!(map_json.contains("  \"connections\": null,\n  \"object_events\": [],\n  \"warp_events\": [\n    {\n      \"x\": 3,"));
    assert!(map_json.ends_with("  \"coord_events\": [],\n  \"bg_events\": []\n}\n"));
    assert_eq!(fs::read_to_string(dir.path().join("data/maps/TaurosMeadow/scripts.inc")).unwrap(), "TaurosMeadow_MapScripts::\n\t.byte 0\n");
    let groups = fs::read_to_string(dir.path().join("data/maps/map_groups.json")).unwrap();
    assert!(groups.contains("    \"Underwater_Route125\",\n    \"TaurosMeadow\"\n  ],"), "new map is the last of the first group");
    let events = fs::read_to_string(dir.path().join("data/event_scripts.s")).unwrap();
    assert_eq!(events.matches("data/maps/TaurosMeadow/scripts.inc").count(), 1);
    // Blocks: 16x12 fill words (metatile 1, elevation 3) and the 2x2 Emerald border.
    assert_eq!(words(&dir.path().join("data/layouts/TaurosMeadow/map.bin")), vec![0x3001; 16 * 12]);
    assert_eq!(words(&dir.path().join("data/layouts/TaurosMeadow/border.bin")), [0x1D4, 0x1D5, 0x1DC, 0x1DD]);
    let layouts = fs::read_to_string(dir.path().join("data/layouts/layouts.json")).unwrap();
    assert!(layouts.ends_with("      \"layout_version\": \"frlg\"\n    },\n    {\n      \"id\": \"LAYOUT_TAUROS_MEADOW\",\n      \"name\": \"TaurosMeadow_Layout\",\n      \"width\": 16,\n      \"height\": 12,\n      \"primary_tileset\": \"gTileset_General\",\n      \"secondary_tileset\": \"gTileset_Petalburg\",\n      \"border_filepath\": \"data/layouts/TaurosMeadow/border.bin\",\n      \"blockdata_filepath\": \"data/layouts/TaurosMeadow/map.bin\",\n      \"layout_version\": \"emerald\"\n    }\n  ]\n}\n"));

    // It loads back, and a scene can be added to the new map in a later change set.
    let loaded = c.load().unwrap();
    let map = &loaded.maps[&MapId::from("MAP_TAUROS_MEADOW")];
    assert_eq!(map.geometry, MapGeometry::Grid { layout: LayoutId::from("LAYOUT_TAUROS_MEADOW"), width: 16, height: 12 });
    assert_eq!(map.properties["weather"], Value::Text("WEATHER_NONE".into()), "defaults fill the header");
    assert!(!map.properties.contains_key("group"));
    let scene = Scene {
        label: ScriptLabel::from("TaurosMeadow_EventScript_Sign"),
        map: Some(MapId::from("MAP_TAUROS_MEADOW")),
        language: ScriptLanguage::Assembly,
        source: "TaurosMeadow_EventScript_Sign::\n\tend\n".into(),
    };
    c.apply(&ChangeSet::new("sign script", Provenance::Human).with(Change::UpsertScene { scene: scene.clone() })).unwrap();
    assert_eq!(c.load().unwrap().scenes[&scene.label], scene);

    // The whole create is one commit: restoring it leaves the project as it was.
    let before = git(dir.path(), &["rev-parse", "HEAD~2"]);
    assert!(git(dir.path(), &["diff", "--stat", before.trim(), "HEAD~1", "--", "data/maps/Route101"]).is_empty());
}

#[test]
fn creating_and_resizing_in_one_change_set_is_all_or_nothing() {
    let Some(engine) = common::engine() else { return };
    let dir = common::fixture(&engine, "g3-newmap-rules", &PATHS);
    let mut c = Gen3Connector::open(dir.path(), None).unwrap();
    let set = |changes: Vec<Change>| changes.into_iter().fold(ChangeSet::new("x", Provenance::Human), ChangeSet::with);
    let meadow = |layout: &str| Change::UpsertMap { map: new_map("MAP_TAUROS_MEADOW", "TaurosMeadow", layout, (16, 12)) };
    let layout = |id: &str, w, h, p: Extensions| Change::CreateLayout { layout: LayoutId::from(id), width: w, height: h, properties: p };

    rejected(&mut c, set(vec![meadow("LAYOUT_ROUTE101")]), "map.layout_size");
    rejected(&mut c, set(vec![meadow("LAYOUT_NOPE")]), "map.layout");
    rejected(&mut c, set(vec![layout("LAYOUT_ROUTE101", 4, 4, Extensions::new())]), "layout.unique");
    rejected(&mut c, set(vec![layout("TAUROS", 4, 4, Extensions::new())]), "layout.id");
    rejected(&mut c, set(vec![layout("LAYOUT_A", 0, 4, Extensions::new())]), "layout.size");
    rejected(&mut c, set(vec![layout("LAYOUT_A", 200, 100, Extensions::new())]), "layout.size_ceiling");
    rejected(&mut c, set(vec![layout("LAYOUT_A", 4, 4, layout_props(&[("primary_tileset", Value::Text("gTileset_Nope".into()))]))]), "layout.tileset");
    rejected(&mut c, set(vec![layout("LAYOUT_A", 4, 4, layout_props(&[("colour", Value::Text("red".into()))]))]), "layout.property");
    rejected(&mut c, set(vec![layout("LAYOUT_A", 4, 4, layout_props(&[("fill_metatile", Value::Int(0x400))]))]), "layout.property");
    let mut m = new_map("MAP_TAUROS_MEADOW", "Route101", "LAYOUT_ROUTE101", (20, 20));
    rejected(&mut c, set(vec![Change::UpsertMap { map: m.clone() }]), "map.name");
    m.name = "TaurosMeadow".into();
    m.id = MapId::from("MAP_DYNAMIC");
    rejected(&mut c, set(vec![Change::UpsertMap { map: m.clone() }]), "map.id");
    m.id = MapId::from("tauros");
    rejected(&mut c, set(vec![Change::UpsertMap { map: m.clone() }]), "map.id");
    m.id = MapId::from("MAP_TAUROS_MEADOW");
    m.properties.insert("group".into(), Value::Text("gMapGroup_Nope".into()));
    rejected(&mut c, set(vec![Change::UpsertMap { map: m.clone() }]), "map.group");
    // 'group' is only for creation.
    let mut route = c.load().unwrap().maps[&MapId::from("MAP_ROUTE101")].clone();
    route.properties.insert("group".into(), Value::Text("gMapGroup_TownsAndRoutes".into()));
    rejected(&mut c, set(vec![Change::UpsertMap { map: route }]), "map.property");
    // Resizing: unknown layout, an event that would fall outside, a bad fill.
    let resize = |id: &str, w, h, p: Extensions| Change::ResizeLayout { layout: LayoutId::from(id), width: w, height: h, properties: p };
    rejected(&mut c, set(vec![resize("LAYOUT_NOPE", 4, 4, Extensions::new())]), "layout.exists");
    rejected(&mut c, set(vec![resize("LAYOUT_ROUTE101", 5, 5, Extensions::new())]), "layout.resize_events");
    rejected(&mut c, set(vec![resize("LAYOUT_ROUTE101", 30, 30, layout_props(&[("fill_metatile", Value::Text("x".into()))]))]), "layout.property");
    rejected(&mut c, set(vec![resize("LAYOUT_ROUTE101", 300, 300, Extensions::new())]), "layout.size_ceiling");
    // A valid create followed by an invalid change leaves nothing behind.
    rejected(
        &mut c,
        set(vec![layout("LAYOUT_TAUROS_MEADOW", 16, 12, Extensions::new()), meadow("LAYOUT_TAUROS_MEADOW"), resize("LAYOUT_NOPE", 4, 4, Extensions::new())]),
        "layout.exists",
    );
    assert!(!dir.path().join("data/layouts/TaurosMeadow").exists());
}

#[test]
fn a_resized_layout_keeps_its_top_left_corner() {
    let Some(engine) = common::engine() else { return };
    let dir = common::fixture(&engine, "g3-resize", &PATHS);
    let mut c = Gen3Connector::open(dir.path(), None).unwrap();
    let path = dir.path().join("data/layouts/Route101/map.bin");
    let original = words(&path);
    assert_eq!(original.len(), 20 * 20);
    let applied = c
        .apply(
            &ChangeSet::new("Grow Route 101", Provenance::Human)
                .with(Change::ResizeLayout { layout: LayoutId::from("LAYOUT_ROUTE101"), width: 24, height: 22, properties: Extensions::new() }),
        )
        .unwrap();
    assert_eq!(applied.touched, ["data/layouts/Route101/map.bin", "data/layouts/layouts.json"]);
    let grown = words(&path);
    assert_eq!(grown.len(), 24 * 22);
    for y in 0..22 {
        for x in 0..24 {
            let want = if x < 20 && y < 20 { original[y * 20 + x] } else { 0x3001 };
            assert_eq!(grown[y * 24 + x], want, "block ({x}, {y})");
        }
    }
    let diff: Vec<String> = git(dir.path(), &["show", "--format=", "-U0", "HEAD", "--", "data/layouts/layouts.json"])
        .lines()
        .filter(|l| (l.starts_with('+') || l.starts_with('-')) && !l.starts_with("+++") && !l.starts_with("---"))
        .map(str::to_owned)
        .collect();
    assert_eq!(diff, ["-      \"width\": 20,", "-      \"height\": 20,", "+      \"width\": 24,", "+      \"height\": 22,"]);
    let loaded = c.load().unwrap();
    assert_eq!(
        loaded.maps[&MapId::from("MAP_ROUTE101")].geometry,
        MapGeometry::Grid { layout: LayoutId::from("LAYOUT_ROUTE101"), width: 24, height: 22 }
    );
    // Shrinking back is allowed once nothing is outside, and drops the added blocks.
    c.apply(
        &ChangeSet::new("Shrink Route 101", Provenance::Human)
            .with(Change::ResizeLayout { layout: LayoutId::from("LAYOUT_ROUTE101"), width: 20, height: 20, properties: Extensions::new() }),
    )
    .unwrap();
    assert_eq!(words(&path), original);
    assert!(git(dir.path(), &["diff", "HEAD~2", "--", "data/layouts"]).is_empty());
    // An edit after a resize in the same set sees the new size (and the old one is refused).
    let route = c.load().unwrap().maps[&MapId::from("MAP_ROUTE101")].clone();
    let mut wide = route.clone();
    if let MapGeometry::Grid { width, .. } = &mut wide.geometry {
        *width = 30;
    }
    let set = ChangeSet::new("grow, then edit", Provenance::Human)
        .with(Change::ResizeLayout { layout: LayoutId::from("LAYOUT_ROUTE101"), width: 30, height: 20, properties: Extensions::new() })
        .with(Change::UpsertMap { map: wide });
    c.apply(&set).unwrap();
    assert_eq!(fs::metadata(&path).unwrap().len(), 30 * 20 * 2);
}

#[test]
fn a_map_sharing_a_layout_follows_its_resize() {
    let Some(engine) = common::engine() else { return };
    let dir = common::fixture(&engine, "g3-shared", &PATHS);
    let mut c = Gen3Connector::open(dir.path(), None).unwrap();
    let twin = new_map("MAP_ROUTE101_TWIN", "Route101Twin", "LAYOUT_ROUTE101", (20, 20));
    c.apply(&ChangeSet::new("twin", Provenance::Human).with(Change::UpsertMap { map: twin })).unwrap();
    c.apply(
        &ChangeSet::new("grow", Provenance::Human)
            .with(Change::ResizeLayout { layout: LayoutId::from("LAYOUT_ROUTE101"), width: 21, height: 20, properties: Extensions::new() }),
    )
    .unwrap();
    let loaded = c.load().unwrap();
    for id in ["MAP_ROUTE101", "MAP_ROUTE101_TWIN"] {
        assert!(matches!(loaded.maps[&MapId::from(id)].geometry, MapGeometry::Grid { width: 21, .. }), "{id}");
    }
}

#[test]
fn created_and_resized_maps_build_in_the_container() {
    let Some(engine) = common::engine() else { return };
    if std::env::var_os("TAUROS_DOCKER_TESTS").is_none() {
        eprintln!("SKIPPED: set TAUROS_DOCKER_TESTS=1 to run the container build tests");
        return;
    }
    let toolchain = tauros_connector_gen3::toolchain::Toolchain::for_repository(&common::repo_root()).unwrap();
    let Some(dir) = common::built_fixture(&engine, "g3-newmap-build") else { return };
    let mut c = Gen3Connector::open(dir.path(), Some(toolchain)).unwrap();
    c.apply(&create_set()).unwrap();
    c.apply(
        &ChangeSet::new("Grow Route 101", Provenance::Human)
            .with(Change::ResizeLayout { layout: LayoutId::from("LAYOUT_ROUTE101"), width: 24, height: 22, properties: Extensions::new() }),
    )
    .unwrap();
    let out = c.build().unwrap_or_else(|e| panic!("build failed: {e:?}"));
    let syms = fs::read_to_string(out.symbols.unwrap()).unwrap();
    for label in ["TaurosMeadow_MapScripts", "TaurosMeadow_Layout_Blockdata", "TaurosMeadow_Layout_Border", "TaurosMeadow"] {
        assert!(syms.lines().any(|l| l.ends_with(&format!(" {label}"))), "{label} not linked");
    }
    assert!(dir.path().join("include/constants/map_groups.h").is_file());
    let groups_h = fs::read_to_string(dir.path().join("include/constants/map_groups.h")).unwrap();
    assert!(groups_h.contains("MAP_TAUROS_MEADOW"), "map constant not generated");
}

/// Prepares the project the owner opens in Porymap: a full copy of the pinned
/// expansion with `TaurosMeadow` created and Route 101 grown to 24x22.
/// `TAUROS_PORYMAP_CHECK_DIR=<empty dir> cargo test -p tauros-connector-gen3 --test new_maps prepare_porymap_project -- --ignored`
#[test]
#[ignore = "prepares a manual Porymap check project; needs TAUROS_PORYMAP_CHECK_DIR"]
fn prepare_porymap_project() {
    let Some(engine) = common::engine() else { return };
    let Some(target) = std::env::var_os("TAUROS_PORYMAP_CHECK_DIR") else { panic!("set TAUROS_PORYMAP_CHECK_DIR") };
    let dir = common::fixture(&engine, "g3-porymap-check", &[]);
    let mut c = Gen3Connector::open(dir.path(), None).unwrap();
    c.apply(&create_set()).unwrap();
    c.apply(
        &ChangeSet::new("Grow Route 101 to 24x22", Provenance::Human)
            .with(Change::ResizeLayout { layout: LayoutId::from("LAYOUT_ROUTE101"), width: 24, height: 22, properties: Extensions::new() }),
    )
    .unwrap();
    drop(c);
    let ok = Command::new("cp").arg("-a").arg(dir.path().join(".")).arg(&target).status().unwrap().success();
    assert!(ok, "copy failed");
}
