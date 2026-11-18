//! The contract suite passes on the reference connector and catches broken ones.

use std::collections::BTreeMap;
use tauros_model::contract::{assert_contract, run_all};
use tauros_model::memory::MemoryConnector;
use tauros_model::*;

fn member(species: &str, level: u8) -> PartyMember {
    PartyMember {
        species: SpeciesId::from(species),
        level,
        held_item: None,
        moves: vec![MoveId::from("MOVE_TACKLE")],
        nickname: None,
        ability: None,
        extensions: Extensions::new(),
    }
}

fn trainer(id: &str, members: Vec<PartyMember>) -> Trainer {
    Trainer {
        id: TrainerId::from(id),
        name: id.trim_start_matches("TRAINER_").to_owned(),
        class: Some("Leader".into()),
        items: vec![],
        double_battle: false,
        ai_flags: vec![],
        party: Party { members },
        extensions: Extensions::new(),
    }
}

fn sample() -> ProjectSnapshot {
    let species = ["SPECIES_GEODUDE", "SPECIES_NOSEPASS"]
        .into_iter()
        .map(|s| (SpeciesId::from(s), Species { id: SpeciesId::from(s), name: s[8..].to_owned(), national_dex: None }))
        .collect();
    let map = Map {
        id: MapId::from("MAP_ROUTE101"),
        name: "Route101".into(),
        geometry: MapGeometry::Grid { layout: LayoutId::from("LAYOUT_ROUTE101"), width: 20, height: 20 },
        events: vec![Event::Warp {
            position: Position { x: 1, y: 2, elevation: Some(0) },
            destination_map: MapId::from("MAP_ROUTE101"),
            destination_warp: 0,
            extensions: Extensions::new(),
        }],
        connections: vec![],
        properties: Extensions::new(),
    };
    ProjectSnapshot {
        game: GameInfo {
            family: GameFamily::Gen3,
            engine: "memory fixture".into(),
            capabilities: Capabilities {
                max_party_size: 6,
                max_moves: 4,
                min_level: 1,
                max_level: 100,
                held_items: true,
                abilities: true,
            },
        },
        trainers: [
            trainer("TRAINER_ROXANNE_1", vec![member("SPECIES_GEODUDE", 12), member("SPECIES_NOSEPASS", 15)]),
            trainer("TRAINER_BRAWLY_1", vec![member("SPECIES_GEODUDE", 16)]),
        ]
        .into_iter()
        .map(|t| (t.id.clone(), t))
        .collect(),
        maps: [(map.id.clone(), map)].into_iter().collect(),
        scenes: BTreeMap::new(),
        species,
        flags: BTreeMap::new(),
    }
}

#[test]
fn reference_connector_satisfies_the_contract() {
    assert_contract(|| MemoryConnector::new(sample()));
}

/// A connector that rewrites every trainer on any write (not lossless) and skips
/// validation must fail the suite on exactly those obligations.
struct Sloppy(MemoryConnector);

impl Connector for Sloppy {
    fn info(&self) -> GameInfo {
        self.0.info()
    }
    fn load(&self) -> Result<ProjectSnapshot, ConnectorError> {
        self.0.load()
    }
    fn validate(&self, _: &ChangeSet) -> Vec<Violation> {
        Vec::new()
    }
    fn apply(&mut self, changes: &ChangeSet) -> Result<Applied, ConnectorError> {
        let mut state = self.0.load()?;
        for change in &changes.changes {
            if let Change::UpsertTrainer { trainer } = change {
                state.trainers.insert(trainer.id.clone(), trainer.clone());
            }
        }
        for t in state.trainers.values_mut() {
            t.extensions.insert("rewritten".into(), Value::Bool(true));
        }
        self.0 = MemoryConnector::new(state);
        Ok(Applied { commit: None, touched: vec![] })
    }
    fn build(&mut self) -> Result<BuildOutput, ConnectorError> {
        self.0.build()
    }
    fn fingerprint(&self) -> Result<String, ConnectorError> {
        self.0.fingerprint()
    }
}

#[test]
fn suite_catches_a_lossy_unvalidated_connector() {
    let failures = run_all(|| Sloppy(MemoryConnector::new(sample())));
    let names: Vec<&str> = failures.iter().map(|f| f.split(':').next().unwrap()).collect();
    for expected in [
        "empty_change_set_is_noop",
        "unchanged_rewrite_is_lossless",
        "edit_is_visible_and_local",
        "invalid_change_is_rejected_untouched",
        "missing_target_is_rejected",
        "change_sets_are_atomic",
    ] {
        assert!(names.contains(&expected), "suite missed {expected}; got {failures:?}");
    }
    assert!(!names.contains(&"load_is_deterministic"));
}

#[test]
fn validation_rules() {
    let snap = sample();
    let caps = &snap.game.capabilities;
    let bad = |t: Trainer| validate::check(&snap, &ChangeSet::new("x", Provenance::Human).with(Change::UpsertTrainer { trainer: t }));
    let rule = |v: Vec<Violation>| v.into_iter().map(|v| v.rule).collect::<Vec<_>>();

    assert_eq!(rule(bad(trainer("T", vec![]))), ["party.size"]);
    assert_eq!(rule(bad(trainer("T", vec![member("SPECIES_MISSINGNO", 5)]))), ["species.exists"]);
    assert_eq!(rule(bad(trainer("T", vec![member("SPECIES_GEODUDE", caps.max_level + 1)]))), ["member.level"]);
    let mut five_moves = member("SPECIES_GEODUDE", 5);
    five_moves.moves = vec![MoveId::from("MOVE_TACKLE"); 5];
    assert_eq!(rule(bad(trainer("T", vec![five_moves]))), ["member.moves"]);

    let warp_nowhere = ChangeSet::new("x", Provenance::Human).with(Change::AddEvent {
        map: MapId::from("MAP_ROUTE101"),
        event: Event::Warp {
            position: Position { x: 0, y: 0, elevation: None },
            destination_map: MapId::from("MAP_NOWHERE"),
            destination_warp: 0,
            extensions: Extensions::new(),
        },
    });
    assert_eq!(rule(validate::check(&snap, &warp_nowhere)), ["warp.destination"]);

    let remove_second_warp = ChangeSet::new("x", Provenance::Human).with(Change::RemoveEvent {
        map: MapId::from("MAP_ROUTE101"),
        kind: EventKind::Warp,
        index: 1,
    });
    assert_eq!(rule(validate::check(&snap, &remove_second_warp)), ["event.exists"]);

    let twice = ChangeSet::new("x", Provenance::Human)
        .with(Change::AllocateFlag { id: FlagId::from("FLAG_TAUROS_SCENE") })
        .with(Change::AllocateFlag { id: FlagId::from("FLAG_TAUROS_SCENE") });
    assert_eq!(rule(validate::check(&snap, &twice)), ["flag.unique"]);
    assert_eq!(rule(validate::check(&snap, &ChangeSet::new(" ", Provenance::Human))), ["changeset.summary"]);
}

#[test]
fn gen1_capabilities_reject_held_items() {
    let mut snap = sample();
    snap.game.capabilities.held_items = false;
    let mut m = member("SPECIES_GEODUDE", 5);
    m.held_item = Some(ItemId::from("ITEM_ORAN_BERRY"));
    let set = ChangeSet::new("x", Provenance::Human).with(Change::UpsertTrainer { trainer: trainer("T", vec![m]) });
    assert_eq!(validate::check(&snap, &set)[0].rule, "member.held_item");
}

#[test]
fn history_records_provenance_labels() {
    let mut c = MemoryConnector::new(sample());
    let mut t = c.load().unwrap().trainers.into_values().next().unwrap();
    t.party.members[0].level += 1;
    c.apply(&ChangeSet::new("raise level", Provenance::AiGenerated).with(Change::UpsertTrainer { trainer: t })).unwrap();
    assert_eq!(c.history, vec![("raise level".to_owned(), "ai-generated".to_owned())]);
}

#[test]
fn change_sets_round_trip_through_json() {
    // The MCP layer exchanges change sets as JSON; tags and labels must stay stable.
    let set = ChangeSet::new("add flag", Provenance::AiAssisted)
        .with(Change::AllocateFlag { id: FlagId::from("FLAG_X") })
        .with(Change::RemoveEvent { map: MapId::from("MAP_ROUTE101"), kind: EventKind::Warp, index: 0 });
    let json = serde_json::to_value(&set).unwrap();
    assert_eq!(json["provenance"], "ai-assisted");
    assert_eq!(json["changes"][0]["op"], "allocate_flag");
    assert_eq!(json["changes"][0]["id"], "FLAG_X");
    assert_eq!(json["changes"][1]["kind"], "warp");
    let back: ChangeSet = serde_json::from_value(json).unwrap();
    assert_eq!(back, set);
    let snap = serde_json::to_string(&sample()).unwrap();
    assert_eq!(serde_json::from_str::<ProjectSnapshot>(&snap).unwrap(), sample());
}
