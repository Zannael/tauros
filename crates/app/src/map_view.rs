//! Read-only map data for the Maps view (APP-001): the map drawn from its layout
//! and tilesets, each block's metatile, collision, elevation and behaviour, and the
//! map's events and connections from the model. Nothing here writes to the project.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use base64::Engine;
use serde::Serialize;
use tauros_connector_gen3::maps::{Layouts, MapFile, WarpIds, LAYOUTS_FILE, MAPS_DIR, WARP_IDS_HEADER};
use tauros_connector_gen3::preflight::MapRules;
use tauros_connector_gen3::render::{behavior_names, render_layout, METATILE_PX};
use tauros_model::{Connection, Event, Map, MapGeometry};

#[derive(Debug, Clone, Serialize)]
pub struct MapView {
    pub id: String,
    pub name: String,
    pub layout: String,
    pub width: u16,
    pub height: u16,
    pub primary_tileset: String,
    pub secondary_tileset: String,
    /// `data:image/png;base64,…`, 16 px per block.
    pub image: String,
    pub metatiles: Vec<u16>,
    pub collision: Vec<u8>,
    pub elevation: Vec<u8>,
    pub behaviors: Vec<Option<u32>>,
    /// `MB_*` names of the behaviours this map uses.
    pub behavior_names: BTreeMap<u32, String>,
    pub events: Vec<Event>,
    pub connections: Vec<Connection>,
    /// Header fields (music, weather, map type…) by source name.
    pub properties: serde_json::Value,
    /// Why the build cannot boot into this map, if it cannot.
    pub refusal: Option<String>,
    /// Files or entries that could not be drawn.
    pub problems: Vec<String>,
}

pub fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::Fast);
    let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(rgba).map_err(|e| e.to_string())?;
    writer.finish().map_err(|e| e.to_string())?;
    Ok(out)
}

/// Reads one map from `data/maps/<folder>/map.json` as the connector's snapshot
/// has it, without loading the whole project (seconds on a full expansion).
pub fn read_map(dir: &Path, folder: &str) -> Result<Map, String> {
    let read = |rel: &str| fs::read_to_string(dir.join(rel)).map_err(|e| format!("{rel}: {e}"));
    let layouts = Layouts::parse(&read(LAYOUTS_FILE)?).map_err(|e| format!("{LAYOUTS_FILE}: {}", e.0))?;
    let warp_ids = WarpIds::parse(&read(WARP_IDS_HEADER).unwrap_or_default());
    let rel = format!("{MAPS_DIR}/{folder}/map.json");
    let file = MapFile::parse(&read(&rel)?).map_err(|e| format!("{rel}: {}", e.0))?;
    file.to_model(&layouts, &warp_ids).map_err(|e| format!("{rel}: {}", e.0))
}

/// Draws `map` of the project at `dir`.
pub fn map_view(dir: &Path, map: &Map) -> Result<MapView, String> {
    let MapGeometry::Grid { layout: layout_id, .. } = &map.geometry else {
        return Err(format!("{} has no tile grid", map.id.0));
    };
    let rules = MapRules::read(dir).map_err(|e| e.0)?;
    let layout = rules.layout_path(map).ok_or_else(|| format!("layout {} of {} is not in layouts.json", layout_id.0, map.id.0))?;
    let drawn = render_layout(dir, layout).map_err(|e| e.0)?;
    let png = encode_png(
        u32::from(drawn.width) * METATILE_PX as u32,
        u32::from(drawn.height) * METATILE_PX as u32,
        &drawn.rgba,
    )?;
    let names = behavior_names(dir);
    let behavior_names = drawn.behaviors.iter().flatten().filter_map(|b| names.get(b).map(|n| (*b, n.clone()))).collect();
    Ok(MapView {
        id: map.id.0.clone(),
        name: map.name.clone(),
        layout: layout.id.clone(),
        width: drawn.width,
        height: drawn.height,
        primary_tileset: layout.primary_tileset.clone(),
        secondary_tileset: layout.secondary_tileset.clone(),
        image: format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(png)),
        metatiles: drawn.blocks.iter().map(|b| b.metatile).collect(),
        collision: drawn.blocks.iter().map(|b| b.collision).collect(),
        elevation: drawn.blocks.iter().map(|b| b.elevation).collect(),
        behaviors: drawn.behaviors,
        behavior_names,
        events: map.events.clone(),
        connections: map.connections.clone(),
        properties: serde_json::to_value(&map.properties).unwrap_or_default(),
        refusal: rules.refusal(map),
        problems: drawn.problems,
    })
}
