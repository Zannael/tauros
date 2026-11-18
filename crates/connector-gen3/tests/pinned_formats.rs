//! G3-002 and G3-003 against the pinned expansion, read-only: every trainer file,
//! map, layout and binary block file round-trips byte for byte, and every display
//! name in the trainer files resolves to a constant the project defines.

mod common;

use std::fs;
use tauros_connector_gen3::maps::{Blockdata, Layouts, MapFile, MetatileAttributes, Metatiles, WarpIds, LAYOUTS_FILE, WARP_IDS_HEADER};
use tauros_connector_gen3::names::{Kind, Vocabulary};
use tauros_connector_gen3::party::PartyFile;
use tauros_model::{Event, EventKind, Value};

const PARTY_FILES: [&str; 4] =
    ["src/data/trainers.party", "src/data/trainers_frlg.party", "src/data/battle_partners.party", "src/data/debug_trainers.party"];

#[test]
fn every_party_file_roundtrips_and_resolves() {
    let Some(engine) = common::engine() else { return };
    let mut vocab = Vocabulary::read(&engine).unwrap();
    // trainerproc's own tables, read from the project.
    assert_eq!((vocab.rules.gendered.len(), vocab.rules.itemed.len()), (5, 41));
    let mut trainers_seen = 0;
    for file in PARTY_FILES {
        let text = fs::read_to_string(engine.join(file)).unwrap();
        let party = PartyFile::parse(&text).unwrap();
        assert_eq!(party.emit(), text, "{file}: split and join differ");
        party.observe_names(&mut vocab).unwrap();
        let trainers = party.trainers(&vocab).unwrap();
        let mut rewritten = party.clone();
        for trainer in &trainers {
            rewritten.upsert(trainer, &vocab).unwrap();
        }
        assert_eq!(rewritten.emit(), text, "{file}: unchanged rewrite differs");
        // Every trainer written from the model alone reads back as the same model.
        let mut fresh = party.clone();
        for trainer in &trainers {
            assert!(fresh.remove(&trainer.id.0));
            fresh.upsert(trainer, &vocab).unwrap();
            assert_eq!(fresh.trainer(&trainer.id.0, &vocab).unwrap().as_ref(), Some(trainer), "{file}: {}", trainer.id);
        }
        let mut unknown = Vec::new();
        for t in &trainers {
            let mut check = |kind: Kind, constant: &str| {
                if !vocab.is_defined(kind, constant) {
                    unknown.push(format!("{}: {constant}", t.id));
                }
            };
            t.items.iter().for_each(|i| check(Kind::Item, &i.0));
            t.ai_flags.iter().for_each(|f| check(Kind::AiFlag, f));
            for m in &t.party.members {
                check(Kind::Species, &m.species.0);
                m.held_item.iter().for_each(|i| check(Kind::Item, &i.0));
                m.moves.iter().for_each(|mv| check(Kind::Move, &mv.0));
                m.ability.iter().for_each(|a| check(Kind::Ability, a));
            }
        }
        assert!(unknown.is_empty(), "{file}: names resolving to undefined constants: {unknown:?}");
        eprintln!("{file}: {} trainers, byte-identical, all names resolved", trainers.len());
        trainers_seen += trainers.len();
    }
    assert!(trainers_seen > 1400);
    // Spot-check the examples from the specification.
    assert_eq!(vocab.resolve(Kind::Species, "Geodude"), "SPECIES_GEODUDE");
    assert_eq!(vocab.resolve(Kind::Item, "Oran Berry"), "ITEM_ORAN_BERRY");
    assert!(vocab.is_defined(Kind::Species, "SPECIES_GEODUDE") && vocab.is_defined(Kind::Item, "ITEM_ORAN_BERRY"));
    assert_eq!(vocab.display(Kind::Item, "ITEM_ORAN_BERRY"), "Oran Berry");
}

#[test]
fn every_map_and_layout_roundtrips() {
    let Some(engine) = common::engine() else { return };
    let layouts_text = fs::read_to_string(engine.join(LAYOUTS_FILE)).unwrap();
    let layouts = Layouts::parse(&layouts_text).unwrap();
    assert_eq!(layouts.render(), layouts_text);
    let mut unchanged = layouts.clone();
    for layout in layouts.all() {
        unchanged.update(layout).unwrap();
    }
    assert_eq!(unchanged.render(), layouts_text, "layouts.json unchanged rewrite differs");
    let warp_ids = WarpIds::parse(&fs::read_to_string(engine.join(WARP_IDS_HEADER)).unwrap());
    let maps = common::files(&engine, "data/maps", |n| n == "map.json");
    let groups = fs::read_to_string(engine.join("data/maps/map_groups.json")).unwrap();
    assert_eq!(tauros_connector_gen3::json::Document::parse(&groups).unwrap().render(), groups);
    let mut events = 0;
    for path in &maps {
        let text = fs::read_to_string(path).unwrap();
        let mut file = MapFile::parse(&text).unwrap();
        assert_eq!(file.render(), text, "{}", path.display());
        let map = file.to_model(&layouts, &warp_ids).unwrap();
        file.apply(&map, &map.clone()).unwrap();
        assert_eq!(file.render(), text, "{}: unchanged rewrite differs", path.display());
        // An edit then its reversal restores every byte.
        let mut edited = map.clone();
        edited.name.push('X');
        // Order is meaningful within a kind only: the file keeps one array per kind.
        edited.events.sort_by_key(|e| e.kind());
        let mut by_kind: Vec<Event> = Vec::new();
        for kind in [EventKind::Npc, EventKind::Warp, EventKind::Trigger, EventKind::Background] {
            by_kind.extend(edited.events.iter().filter(|e| e.kind() == kind).rev().cloned());
        }
        edited.events = by_kind;
        edited.properties.insert("tauros_probe".into(), Value::Int(1));
        file.apply(&map, &edited).unwrap();
        let reread = MapFile::parse(&file.render()).unwrap().to_model(&layouts, &warp_ids).unwrap();
        assert_eq!(reread, edited, "{}: edit not read back", path.display());
        file.apply(&edited, &map).unwrap();
        let back = file.render();
        if back != text {
            let at = back.bytes().zip(text.bytes()).take_while(|(a, b)| a == b).count();
            panic!("{}: edit and revert differs at byte {at}: {:?} vs {:?}", path.display(), &back[at.saturating_sub(80)..(at + 80).min(back.len())], &text[at.saturating_sub(80)..(at + 80).min(text.len())]);
        }
        events += map.events.len();
    }
    assert_eq!(maps.len(), 939);
    eprintln!("{} map.json files and layouts.json ({} layouts): byte-identical; {events} events", maps.len(), layouts.all().len());
}

#[test]
fn every_block_and_metatile_file_roundtrips() {
    let Some(engine) = common::engine() else { return };
    let layouts = Layouts::parse(&fs::read_to_string(engine.join(LAYOUTS_FILE)).unwrap()).unwrap();
    let mut checked = 0;
    let mut size_mismatches = Vec::new();
    for layout in layouts.all() {
        let (bw, bh) = layout.border_size();
        for (file, blocks) in [
            (&layout.blockdata_filepath, layout.width as usize * layout.height as usize),
            (&layout.border_filepath, bw as usize * bh as usize),
        ] {
            let bytes = fs::read(engine.join(file)).unwrap();
            let data = Blockdata::parse(&bytes).unwrap();
            if data.0.len() != blocks {
                // Every upstream mismatch is one trailing extra block, which the
                // engine never reads (it indexes width × height blocks).
                assert_eq!(data.0.len(), blocks + 1, "{file}");
                size_mismatches.push(file.clone());
            }
            assert_eq!(data.to_bytes(), bytes, "{file}");
            checked += 1;
        }
    }
    let mut tilesets = 0;
    for path in common::files(&engine, "data/tilesets", |n| n == "metatiles.bin") {
        let bytes = fs::read(&path).unwrap();
        let metatiles = Metatiles::parse(&bytes).unwrap();
        assert_eq!(metatiles.to_bytes(), bytes);
        let attr_path = path.with_file_name("metatile_attributes.bin");
        let attr_bytes = fs::read(&attr_path).unwrap();
        let attrs = MetatileAttributes::parse(&attr_bytes, metatiles.count()).unwrap();
        assert_eq!(attrs.to_bytes(), attr_bytes, "{}", attr_path.display());
        tilesets += 1;
    }
    eprintln!("{checked} map.bin/border.bin files and {tilesets} tilesets: byte-identical");
    // Upstream data, recorded rather than fixed: sizes that disagree with layouts.json.
    eprintln!("{} files hold one block more than layouts.json declares", size_mismatches.len());
}
