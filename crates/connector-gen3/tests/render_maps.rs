//! APP-001 against the pinned expansion, read-only: every layout draws, and the
//! Emerald layouts resolve all their tileset files and metatiles, except the listed
//! layouts whose own data is incomplete.

mod common;

use std::collections::BTreeSet;
use std::fs;
use tauros_connector_gen3::maps::{Layouts, LAYOUTS_FILE};
use tauros_connector_gen3::render::{behavior_names, render_layout, METATILE_PX};

/// Unused layout with no secondary tileset (`"0"`) whose blocks use secondary metatiles.
const INCOMPLETE: &[&str] = &["LAYOUT_UNUSED_OUTDOOR_AREA"];

#[test]
fn every_layout_renders_and_emerald_layouts_resolve_their_tilesets() {
    let Some(engine) = common::engine() else { return };
    let layouts = Layouts::parse(&fs::read_to_string(engine.join(LAYOUTS_FILE)).unwrap()).unwrap();
    let names = behavior_names(&engine);
    assert!(names.len() > 200, "metatile behaviours parsed: {}", names.len());
    let (mut emerald, mut other, mut other_with_problems, mut incomplete) = (0, 0, 0, Vec::new());
    for layout in layouts.all() {
        let drawn = render_layout(&engine, layout).unwrap_or_else(|e| panic!("{}: {e}", layout.id));
        let (w, h) = (layout.width as usize, layout.height as usize);
        assert_eq!(drawn.rgba.len(), w * h * METATILE_PX * METATILE_PX * 4, "{}", layout.id);
        assert_eq!(drawn.blocks.len(), w * h);
        if layout.layout_version.as_deref() == Some("frlg") {
            other += 1;
            if !drawn.problems.is_empty() {
                other_with_problems += 1;
                eprintln!("{}: {:?}", layout.id, drawn.problems);
            }
            continue;
        }
        emerald += 1;
        if !drawn.problems.is_empty() {
            incomplete.push(layout.id.as_str());
            continue;
        }
        assert!(drawn.behaviors.iter().all(Option::is_some), "{}: blocks without attributes", layout.id);
        // A real map is never a single colour.
        let colours: BTreeSet<&[u8]> = drawn.rgba.chunks_exact(4).collect();
        assert!(colours.len() > 1 || w * h <= 1, "{} draws as one colour", layout.id);
    }
    assert_eq!(incomplete, INCOMPLETE);
    eprintln!("rendered {} of {emerald} Emerald layouts cleanly and {other} FRLG layouts ({other_with_problems} with undefined metatiles or files)", emerald - incomplete.len());
    assert!(emerald > 400);
}
