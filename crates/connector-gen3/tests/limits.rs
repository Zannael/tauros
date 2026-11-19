//! G3-008: boundaries, cumulative edits, pre-build refusal and pinned-map audit.
mod common;
use std::fs;
use tauros_connector_gen3::{limits::Limits, Gen3Connector};
use tauros_model::*;

fn npc(x: i32, y: i32, graphics: &str, flagged: bool) -> Event {
    Event::Npc {
        local_id: None,
        graphics: graphics.into(),
        position: Position { x, y, elevation: Some(3) },
        movement: None,
        script: None,
        visibility_flag: flagged.then(|| FlagId::from("FLAG_HIDE")),
        extensions: Default::default(),
    }
}
fn map() -> Map {
    Map {
        id: "MAP_TEST".into(),
        name: "Test".into(),
        geometry: MapGeometry::Grid { layout: "LAYOUT_TEST".into(), width: 65, height: 114 },
        events: vec![],
        connections: vec![],
        properties: Default::default(),
    }
}
fn error(v: &[Violation], rule: &str) -> bool {
    v.iter().any(|v| v.rule == rule && v.severity == Severity::Error)
}

#[test]
fn static_boundaries_and_palette_warnings() {
    let dir = common::TempDir::new("limits-boundaries");
    let graphics_dir = dir.path().join("src/data/object_events");
    fs::create_dir_all(&graphics_dir).unwrap();
    fs::write(
        graphics_dir.join("object_event_graphics_info.h"),
        (0..16).map(|i| format!("const struct ObjectEventGraphicsInfo Info{i} = {{ .paletteTag = PAL_{i}, }};\n")).collect::<String>(),
    )
    .unwrap();
    fs::write(
        graphics_dir.join("object_event_graphics_info_pointers.h"),
        (0..16).map(|i| format!("[GFX_{i}] = &Info{i},\n")).collect::<String>(),
    )
    .unwrap();
    let limits = Limits::read(dir.path());
    let mut m = map();
    assert!(!error(&limits.check(&m, 0, None), "map.size_ceiling"));
    if let MapGeometry::Grid { width, .. } = &mut m.geometry {
        *width += 1;
    }
    assert!(error(&limits.check(&m, 0, None), "map.size_ceiling"));
    m = map();
    m.events = (0..64).map(|i| npc(i * 25, 0, "GFX_0", false)).collect();
    assert!(!error(&limits.check(&m, 0, None), "map.object_templates"));
    m.events.push(npc(2000, 0, "GFX_0", false));
    assert!(error(&limits.check(&m, 0, None), "map.object_templates"));
    m.events = (0..15).map(|_| npc(5, 5, "GFX_0", false)).collect();
    assert!(!error(&limits.check(&m, 0, None), "map.npc_slots"));
    m.events.push(npc(5, 5, "GFX_0", false));
    assert!(error(&limits.check(&m, 0, None), "map.npc_slots"));
    // More than 15 total is allowed when windows do not overlap.
    m.events = (0..32).map(|i| npc(i * 25, 0, "GFX_0", false)).collect();
    assert!(!error(&limits.check(&m, 0, None), "map.npc_slots"));
    // Shared palettes count once. Distinct tags plus player consume 16 slots.
    m.events = (0..15).map(|i| npc(5, 5, &format!("GFX_{i}"), true)).collect();
    assert!(!limits.check(&m, 0, None).iter().any(|v| v.rule == "map.palette_slots"));
    m.events.push(npc(5, 5, "GFX_15", true));
    let v = limits.check(&m, 0, None);
    assert!(v.iter().any(|v| v.rule == "map.palette_slots" && v.severity == Severity::Warning));
    assert!(!error(&v, "map.npc_slots"));
    m.events = (0..15).map(|_| npc(5, 5, "GFX_0", false)).collect();
    assert!(!limits.check(&m, 0, None).iter().any(|v| v.rule == "map.palette_slots"));
}

#[test]
fn pinned_maps_and_cumulative_writes_are_checked() {
    let Some(engine) = common::engine() else { return };
    let dir = common::fixture(&engine, "limits-project", &["src", "include", "data", "tools/trainerproc"]);
    let mut c = Gen3Connector::open(dir.path(), None).unwrap();
    let snapshot = c.load().unwrap();
    let baseline = c.validate(&ChangeSet::new("audit pinned maps", Provenance::Human));
    assert!(!baseline.iter().any(|v| v.severity == Severity::Error), "{baseline:?}");
    assert_eq!(snapshot.maps.len(), 939);
    eprintln!("939 pinned maps audited without blocking violations; {} warnings", baseline.len());
    let item_path = dir.path().join("include/constants/items.h");
    let item_source = fs::read_to_string(&item_path).unwrap();
    fs::write(&item_path, item_source.replace("ITEM_POTION = 28", "ITEM_POTION = 29")).unwrap();
    assert!(matches!(c.build_report(), Err(ConnectorError::Rejected(v)) if error(&v, "item.id_stability")));
    fs::write(&item_path, item_source).unwrap();
    let id = MapId::from("MAP_ROUTE101");
    let mut m = snapshot.maps[&id].clone();
    m.events.clear();
    // Two changes in the same set must see each other's NPCs.
    let set = ChangeSet::new("overflow in two edits", Provenance::AiAssisted).with(Change::UpsertMap { map: m.clone() });
    let mut set = set;
    for _ in 0..16 {
        set.changes.push(Change::AddEvent { map: id.clone(), event: npc(5, 5, "OBJ_EVENT_GFX_BOY_1", false) });
    }
    let before = c.fingerprint().unwrap();
    assert!(error(&c.validate(&set), "map.npc_slots"));
    assert!(matches!(c.apply(&set), Err(ConnectorError::Rejected(_))));
    assert_eq!(c.fingerprint().unwrap(), before);
    set.changes.pop();
    c.apply(&set).unwrap();
    // A manual edit after opening must also be refused by build, before Docker.
    let path = dir.path().join("data/maps/Route101/map.json");
    let text = fs::read_to_string(&path).unwrap();
    let first = text.find("\"object_events\": [").unwrap();
    let from = first + text[first..].find('{').unwrap();
    let to = from + text[from..].find('}').unwrap() + 1;
    let mut edited = text.clone();
    edited.insert_str(to, &format!(",\n{}", &text[from..to]));
    fs::write(path, edited).unwrap();
    assert!(matches!(c.build_report(),Err(ConnectorError::Rejected(v)) if error(&v,"map.npc_slots")));
}

#[test]
fn saved_item_ids_and_live_metatile_ids_cannot_be_reassigned() {
    use tauros_connector_gen3::limits::IdBaseline;
    let dir = common::TempDir::new("id-stability");
    let headers = dir.path().join("include/constants");
    fs::create_dir_all(&headers).unwrap();
    let items = headers.join("items.h");
    let behaviors = headers.join("metatile_behaviors.h");
    fs::write(&items, "ITEM_NONE = 0,\nITEM_A = 1,\n").unwrap();
    fs::write(&behaviors, "MB_NORMAL,\nMB_UNUSED_01,\nMB_UNUSED_LIVE,\n").unwrap();
    let tiles = dir.path().join("data/tilesets/test");
    fs::create_dir_all(&tiles).unwrap();
    fs::write(tiles.join("metatile_attributes.bin"), [2, 0]).unwrap();
    let baseline = IdBaseline::read(dir.path());
    assert!(baseline.check(dir.path()).is_empty());
    fs::write(&items, "ITEM_NONE = 0,\nITEM_A = 2,\n").unwrap();
    assert!(error(&baseline.check(dir.path()), "item.id_stability"));
    fs::write(&items, "ITEM_NONE = 0,\nITEM_A = 1,\nITEM_NEW = 2,\n").unwrap();
    assert!(baseline.check(dir.path()).is_empty());
    // Reclaim a dead slot and append the largest valid behavior id.
    fs::write(&behaviors, "MB_NORMAL = 0,\nMB_NEW = 1,\nMB_UNUSED_LIVE = 2,\nMB_APPEND = 254,\n").unwrap();
    assert!(baseline.check(dir.path()).is_empty());
    fs::write(&behaviors, "MB_NORMAL = 0,\nMB_NEW = 1,\nMB_UNUSED_LIVE = 2,\nMB_APPEND = 255,\n").unwrap();
    assert!(error(&baseline.check(dir.path()), "metatile.append_or_dead"));
    fs::write(&behaviors, "MB_NORMAL = 0,\nMB_UNUSED_01 = 1,\nMB_NEW = 2,\n").unwrap();
    assert!(error(&baseline.check(dir.path()), "metatile.id_stability"));
}
