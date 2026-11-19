//! Static map pressure checks. Existing scripted maps may exceed the potential
//! viewport population (walls and scripts are not simulated). Warn on that
//! baseline; reject new/worsened unconditional pressure. Palette pressure is a
//! warning, as weather, dynamic graphics and script visibility require runtime.
use crate::flags;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use tauros_model::{Event, Map, MapGeometry, Value, Violation};

pub struct Limits {
    pub map_blocks: i64,
    pub offset_w: i64,
    pub offset_h: i64,
    pub npc_slots: usize,
    pub templates: usize,
    palettes: BTreeMap<String, String>,
}

impl Limits {
    pub fn read(root: &Path) -> Self {
        let mut macros = BTreeMap::new();
        for file in ["include/fieldmap.h", "include/constants/global.h", "include/config/overworld.h"] {
            for (n, _, b) in flags::active_defines(&fs::read_to_string(root.join(file)).unwrap_or_default(), &BTreeMap::new()) {
                macros.insert(n, b);
            }
        }
        let get = |n, default| flags::evaluate(n, &macros).filter(|v| *v > 0).unwrap_or(default);
        let follower = flags::evaluate("OW_FOLLOWERS_ENABLED", &macros).unwrap_or(0) != 0;
        let info = fs::read_to_string(root.join("src/data/object_events/object_event_graphics_info.h")).unwrap_or_default();
        let mut tags = BTreeMap::new();
        for block in info.split("const struct ObjectEventGraphicsInfo ").skip(1) {
            let name = block.split_whitespace().next().unwrap_or("");
            if let Some(tag) = block.split(".paletteTag =").nth(1).and_then(|s| s.split(',').next()) {
                tags.insert(name.to_owned(), tag.trim().to_owned());
            }
        }
        let pointers = fs::read_to_string(root.join("src/data/object_events/object_event_graphics_info_pointers.h")).unwrap_or_default();
        let mut palettes = BTreeMap::new();
        for line in pointers.lines() {
            if let Some((key, value)) = line.split_once("] =") {
                if let (Some(key), Some(value)) = (key.split('[').nth(1), value.trim().strip_prefix('&')) {
                    if let Some(tag) = tags.get(value.trim_end_matches(',').trim()) {
                        palettes.insert(key.trim().into(), tag.clone());
                    }
                }
            }
        }
        Self {
            map_blocks: get("MAX_MAP_DATA_SIZE", 10240),
            offset_w: get("MAP_OFFSET_W", 15),
            offset_h: get("MAP_OFFSET_H", 14),
            npc_slots: get("OBJECT_EVENTS_COUNT", 16).saturating_sub(1 + i64::from(follower)) as usize,
            templates: get("OBJECT_EVENT_TEMPLATES_COUNT", 64) as usize,
            palettes,
        }
    }

    /// Maximum unconditional population in any spawn window (20 x 17 at the pin).
    pub fn peak(&self, map: &Map) -> usize {
        let points = npcs(map, false);
        if points.len() > self.templates {
            return points.len();
        }
        windows(&points, self.offset_w + 4, self.offset_h + 2).into_iter().map(|w| w.len()).max().unwrap_or(0)
    }

    pub fn check(&self, map: &Map, baseline: usize, change: Option<usize>) -> Vec<Violation> {
        let mut found = Vec::new();
        if let MapGeometry::Grid { width, height, .. } = map.geometry {
            let blocks = i64::from(width).saturating_add(self.offset_w).saturating_mul(i64::from(height).saturating_add(self.offset_h));
            if blocks > self.map_blocks {
                found.push(Violation::error(
                    change,
                    "map.size_ceiling",
                    format!("{}: padded map uses {blocks}/{} blocks", map.id, self.map_blocks),
                ));
            }
        }
        let count = map.events.iter().filter(|e| matches!(e, Event::Npc { .. })).count();
        if count > self.templates {
            found.push(Violation::error(
                change,
                "map.object_templates",
                format!("{}: {count}/{} object templates", map.id, self.templates),
            ));
            return found;
        }
        let peak = self.peak(map);
        if peak > self.npc_slots {
            let message = format!("{}: {peak}/{} unconditional NPCs in a spawn window; static positions only", map.id, self.npc_slots);
            found.push(if peak > baseline {
                Violation::error(change, "map.npc_slots", message)
            } else {
                Violation::warning(change, "map.npc_slots", message)
            });
        }
        let points = npcs(map, true);
        let windows = windows(&points, self.offset_w + 4, self.offset_h + 2);
        if windows.iter().any(|w| w.len() > self.npc_slots) && peak <= self.npc_slots {
            found.push(Violation::warning(
                change,
                "map.npc_visibility",
                format!("{}: possible NPC overflow if visibility flags are all clear", map.id),
            ));
        }
        let weather = matches!(map.properties.get("weather"), Some(Value::Text(s)) if !matches!(s.as_str(), "WEATHER_NONE" | "WEATHER_SUNNY" | "WEATHER_SUNNY_CLOUDS" | "WEATHER_INDOOR"));
        // One player palette, and two estimated weather slots when applicable.
        let reserve = 1 + if weather { 2 } else { 0 };
        let mut unresolved = false;
        let palette_peak = windows
            .iter()
            .map(|window| {
                let mut tags = BTreeSet::new();
                for &index in window {
                    if let Event::Npc { graphics, .. } = points[index].2 {
                        match self.palettes.get(graphics) {
                            Some(tag) if tag == "TAG_NONE" => {}
                            Some(tag) if tag != "OBJ_EVENT_PAL_TAG_DYNAMIC" => {
                                tags.insert(tag.clone());
                            }
                            _ => {
                                unresolved = true;
                                tags.insert(format!("unresolved:{graphics}"));
                            }
                        }
                    }
                }
                tags.len() + reserve
            })
            .max()
            .unwrap_or(reserve);
        if palette_peak > 16 {
            found.push(Violation::warning(
                change,
                "map.palette_slots",
                format!("{}: estimated {palette_peak}/16 OBJ palette slots including player/weather; verify at runtime", map.id),
            ));
        }
        if unresolved {
            found.push(Violation::warning(
                change,
                "map.palette_unknown",
                format!("{}: dynamic or unresolved graphics prevent a complete palette check", map.id),
            ));
        }
        found
    }
}

fn npcs(map: &Map, include_flagged: bool) -> Vec<(i64, i64, &Event)> {
    map.events
        .iter()
        .filter_map(|e| match e {
            Event::Npc { position, graphics, visibility_flag, .. }
                if graphics != "OBJ_EVENT_GFX_LIGHT_SPRITE" && (include_flagged || visibility_flag.is_none()) =>
            {
                Some((i64::from(position.x), i64::from(position.y), e))
            }
            _ => None,
        })
        .collect()
}

fn windows(points: &[(i64, i64, &Event)], width_span: i64, height_span: i64) -> Vec<Vec<usize>> {
    let mut result = Vec::new();
    for &(x, _, _) in points {
        for &(_, y, _) in points {
            let members = points
                .iter()
                .enumerate()
                .filter(|(_, (a, b, _))| *a >= x && *a <= x + width_span && *b >= y && *b <= y + height_span)
                .map(|(i, _)| i)
                .collect();
            result.push(members);
        }
    }
    result
}

/// Numeric source ids captured when a project is opened, to catch out-of-band
/// edits before a later apply/build can silently reinterpret existing saves/tiles.
pub struct IdBaseline {
    items: BTreeMap<String, i64>,
    behaviors: BTreeMap<String, i64>,
    dead_behaviors: BTreeSet<i64>,
}

fn enum_ids(root: &Path, file: &str, prefix: &str) -> BTreeMap<String, i64> {
    let text = fs::read_to_string(root.join(file)).unwrap_or_default();
    let mut out = BTreeMap::new();
    let mut next = 0;
    for line in text.lines() {
        let line = line.trim().split("//").next().unwrap_or("").trim();
        if !line.starts_with(prefix) {
            continue;
        }
        let member = line.split(',').next().unwrap_or("").trim();
        let (name, value) = match member.split_once('=') {
            Some((n, v)) => {
                (n.trim(), flags::evaluate(v.trim(), &out.iter().map(|(n, v): (&String, &i64)| (n.clone(), v.to_string())).collect()))
            }
            None => (member, Some(next)),
        };
        if !crate::scripts::is_identifier(name) {
            continue;
        }
        if let Some(value) = value {
            out.insert(name.to_owned(), value);
            next = value.saturating_add(1);
        }
    }
    out
}

impl IdBaseline {
    pub fn read(root: &Path) -> Self {
        let items = enum_ids(root, "include/constants/items.h", "ITEM_");
        let behaviors = enum_ids(root, "include/constants/metatile_behaviors.h", "MB_");
        let mut used = BTreeSet::new();
        let mut dirs = vec![root.join("data/tilesets")];
        while let Some(dir) = dirs.pop() {
            let Ok(entries) = fs::read_dir(dir) else { continue };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    dirs.push(path);
                } else if path.file_name().is_some_and(|n| n == "metatile_attributes.bin") {
                    if let Ok(bytes) = fs::read(&path) {
                        // The behavior is the low byte in both 16- and 32-bit formats.
                        // Reading every halfword is conservative for mixed projects.
                        used.extend(bytes.chunks_exact(2).map(|b| i64::from(b[0])));
                    }
                }
            }
        }
        let dead_behaviors = behaviors.iter().filter(|(n, v)| n.starts_with("MB_UNUSED_") && !used.contains(v)).map(|(_, v)| *v).collect();
        Self { items, behaviors, dead_behaviors }
    }

    pub fn check(&self, root: &Path) -> Vec<Violation> {
        let items = enum_ids(root, "include/constants/items.h", "ITEM_");
        let behaviors = enum_ids(root, "include/constants/metatile_behaviors.h", "MB_");
        let mut found = Vec::new();
        for (name, value) in &self.items {
            if items.get(name) != Some(value) {
                found.push(Violation::error(
                    None,
                    "item.id_stability",
                    format!("{name} must retain id {value}; existing saves store numeric ids"),
                ));
            }
        }
        let max_item = self.items.values().max().copied().unwrap_or(-1);
        let mut new_items = BTreeSet::new();
        if items.iter().any(|(n, v)| !self.items.contains_key(n) && (*v <= max_item || !new_items.insert(v))) {
            found.push(Violation::error(None, "item.append_only", "new item ids must follow existing ids"));
        }
        let max_behavior = self.behaviors.values().max().copied().unwrap_or(-1);
        for (name, value) in &self.behaviors {
            if behaviors.get(name) != Some(value)
                && !(name.starts_with("MB_UNUSED_") && self.dead_behaviors.contains(value) && !behaviors.contains_key(name))
            {
                found.push(Violation::error(None, "metatile.id_stability", format!("{name} must retain behavior id {value}")));
            }
        }
        let mut allocated = BTreeSet::new();
        for (name, value) in &behaviors {
            if !self.behaviors.contains_key(name) {
                let vacant =
                    self.dead_behaviors.contains(value) && !self.behaviors.iter().any(|(n, v)| v == value && behaviors.contains_key(n));
                if *value < 0 || *value >= 255 || (*value <= max_behavior && !vacant) || !allocated.insert(value) {
                    found.push(Violation::error(
                        None,
                        "metatile.append_or_dead",
                        format!("{name}={value} must append below MB_INVALID=255 or replace a confirmed-unused slot"),
                    ));
                }
            }
        }
        found
    }
}
