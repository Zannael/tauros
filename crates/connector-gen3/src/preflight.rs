//! Checks a boot point before anything runs (EMU-006): the map must exist in the
//! build and the tile must be walkable.
//!
//! A build only contains the maps its `mapjson` run keeps (tools/mapjson/mapjson.cpp,
//! `MAP_VERSION` of the Makefile, `emerald` by default): with `emerald` a map whose
//! `region` is not `REGION_HOENN` (absent means Hoenn) is left out of the map
//! groups, and a layout whose `layout_version` is not `emerald` gets a NULL entry in
//! the layout table. `map_groups.h` still defines the constants of those maps, so the
//! game accepts the boot and then hangs loading a NULL map header or layout. Probed
//! on the pinned expansion base build: every one of the 419 FireRed/LeafGreen maps
//! hangs, every kept map loads.

use crate::boot::BootError;
use crate::maps::{Layouts, MapGroups, LAYOUTS_FILE, MAP_GROUPS_FILE};
use crate::walk::{Blocked, PointCheck, Walkability};
use std::fs;
use std::path::Path;
use std::collections::{BTreeMap, BTreeSet};
use tauros_model::{Map, MapGeometry, MapId, Value};

/// What the build keeps, from the project's Makefile and layout table.
#[derive(Clone, Debug)]
pub struct MapRules {
    /// `MAP_VERSION` of the Makefile: `emerald`, `firered` or `ruby`.
    version: String,
    layouts: Layouts,
    /// Names of the maps in `data/maps/map_groups.json`; `None` when the file is absent.
    grouped: Option<BTreeSet<String>>,
}

fn makefile_version(project: &Path) -> String {
    let makefile = fs::read_to_string(project.join("Makefile")).unwrap_or_default();
    makefile
        .lines()
        .find_map(|l| l.strip_prefix("MAP_VERSION").and_then(|r| r.split_once("?=")).map(|(_, v)| v.trim().to_owned()))
        .unwrap_or_else(|| "emerald".into())
}

impl MapRules {
    pub fn read(project: &Path) -> Result<Self, BootError> {
        let text = fs::read_to_string(project.join(LAYOUTS_FILE)).map_err(|e| BootError(format!("{LAYOUTS_FILE}: {e}")))?;
        let layouts = Layouts::parse(&text).map_err(|e| BootError(format!("{LAYOUTS_FILE}: {}", e.0)))?;
        let grouped = match fs::read_to_string(project.join(MAP_GROUPS_FILE)) {
            Ok(text) => {
                let groups = MapGroups::parse(&text).map_err(|e| BootError(format!("{MAP_GROUPS_FILE}: {}", e.0)))?;
                Some(groups.groups().iter().flat_map(|g| groups.members(g)).collect())
            }
            Err(_) => None,
        };
        Ok(Self { version: makefile_version(project), layouts, grouped })
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    /// The version the build keeps, or `None` when it keeps everything (`ruby`).
    fn kept(&self) -> Option<(&'static str, &'static str)> {
        match self.version.as_str() {
            "emerald" => Some(("REGION_HOENN", "emerald")),
            "firered" => Some(("REGION_KANTO", "frlg")),
            _ => None,
        }
    }

    /// Why a build of this project cannot boot into `map`, or `None` when it can.
    pub fn refusal(&self, map: &Map) -> Option<String> {
        let MapGeometry::Grid { layout, .. } = &map.geometry else {
            return Some("it has no tile grid".into());
        };
        if self.grouped.as_ref().is_some_and(|names| !names.contains(&map.name)) {
            return Some(format!("it is in no group of {MAP_GROUPS_FILE}, so the build gives it no MAP_ constant"));
        }
        let Some(layout) = self.layouts.get(&layout.0) else {
            return Some(format!("its layout {} is not in {LAYOUTS_FILE}", layout.0));
        };
        let (region, layout_version) = self.kept()?;
        let own_region = match map.properties.get("region") {
            Some(Value::Text(r)) if !r.is_empty() => r.as_str(),
            _ => region,
        };
        if own_region != region {
            return Some(format!(
                "it belongs to {own_region}; a {} build leaves out the maps of other regions",
                self.version
            ));
        }
        let own_version = layout.layout_version.as_deref().filter(|v| !v.is_empty()).unwrap_or(layout_version);
        if own_version != layout_version {
            return Some(format!(
                "its layout {} is a {own_version} layout; a {} build has no layout for it",
                layout.id, self.version
            ));
        }
        None
    }

    /// The collision grid of `map`.
    pub fn grid(&self, project: &Path, map: &Map) -> Option<Result<Walkability, BootError>> {
        Some(Walkability::read_layout(project, self.layout_path(map)?))
    }

    pub fn layout_path(&self, map: &Map) -> Option<&crate::maps::Layout> {
        match &map.geometry {
            MapGeometry::Grid { layout, .. } => self.layouts.get(&layout.0),
            _ => None,
        }
    }
}

/// How a map appears in the boot point selector.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapUsability {
    /// The tile the picker starts on: the walkable tile nearest the centre.
    pub default_point: (i16, i16),
    /// Set when the build cannot boot into the map.
    pub refusal: Option<String>,
}

/// Selector entries for every map with a tile grid.
pub fn map_usability(project: &Path, maps: &BTreeMap<MapId, Map>) -> Result<Vec<(MapId, u16, u16, MapUsability)>, BootError> {
    let rules = MapRules::read(project)?;
    let mut out = Vec::new();
    for map in maps.values() {
        let MapGeometry::Grid { width, height, .. } = map.geometry else { continue };
        let centre = ((width / 2) as i16, (height / 2) as i16);
        let mut refusal = rules.refusal(map);
        let mut default_point = centre;
        if refusal.is_none() {
            match rules.grid(project, map) {
                Some(Ok(grid)) => match grid.default_point() {
                    Some(p) => default_point = p,
                    None => refusal = Some("it has no tile the player can stand on (all solid or deep water)".into()),
                },
                Some(Err(e)) => refusal = Some(format!("its blockdata cannot be read ({})", e.0)),
                None => {}
            }
        }
        out.push((map.id.clone(), width, height, MapUsability { default_point, refusal }));
    }
    Ok(out)
}

/// What a boot at a point would run into.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PointAssessment {
    Fine,
    /// The build cannot boot into the map at all.
    MapRefused(String),
    /// The tile cannot be stood on; `nearest` is the closest one that can.
    Blocked { reason: Blocked, nearest: Option<(i16, i16)> },
}

impl PointAssessment {
    /// The cause as a sentence for logs and notifications, `None` when fine. It
    /// does not repeat the map and point, which callers print themselves.
    pub fn problem(&self) -> Option<String> {
        match self {
            PointAssessment::Fine => None,
            PointAssessment::MapRefused(why) => Some(format!("the build cannot boot this map: {why}")),
            PointAssessment::Blocked { reason, nearest } => Some(match nearest {
                Some((x, y)) => format!("the tile is {}; the nearest walkable tile is ({x}, {y})", reason.describe()),
                None => format!("the tile is {} and the map has no walkable tile", reason.describe()),
            }),
        }
    }
}

/// Looks at a boot point before anything runs.
pub fn assess_boot_point(project: &Path, maps: &BTreeMap<MapId, Map>, map: &MapId, x: i16, y: i16) -> Result<PointAssessment, BootError> {
    let found = maps.get(map).ok_or_else(|| BootError(format!("unknown map {}", map.0)))?;
    let rules = MapRules::read(project)?;
    if let Some(why) = rules.refusal(found) {
        return Ok(PointAssessment::MapRefused(why));
    }
    let Some(grid) = rules.grid(project, found) else { return Ok(PointAssessment::Fine) };
    Ok(match grid?.check(i32::from(x), i32::from(y)) {
        PointCheck::Walkable => PointAssessment::Fine,
        PointCheck::Blocked { reason, nearest } => PointAssessment::Blocked { reason, nearest },
    })
}

/// Refuses a boot into a map the build lacks or onto a tile the player cannot
/// stand on, with the cause and the nearest walkable tile.
pub fn check_boot_point(project: &Path, maps: &BTreeMap<MapId, Map>, map: &MapId, x: i16, y: i16) -> Result<(), BootError> {
    match assess_boot_point(project, maps, map, x, y)?.problem() {
        None => Ok(()),
        Some(problem) => Err(BootError(problem)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauros_model::LayoutId;

    fn project(tag: &str, makefile: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("tauros-preflight-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("data/layouts/Room")).unwrap();
        fs::write(dir.join("Makefile"), makefile).unwrap();
        fs::create_dir_all(dir.join("data/maps")).unwrap();
        fs::write(
            dir.join(MAP_GROUPS_FILE),
            "{\"group_order\":[\"gMapGroup_A\"],\"gMapGroup_A\":[\"MAP_A\",\"MAP_K\",\"MAP_ROOM\",\"MAP_KANTO\"]}",
        )
        .unwrap();
        let layout = |id: &str, version: &str| {
            format!(
                "{{\"id\":\"{id}\",\"name\":\"{id}\",\"width\":3,\"height\":2,\"primary_tileset\":\"a\",\"secondary_tileset\":\"b\",\"border_filepath\":\"data/layouts/Room/border.bin\",\"blockdata_filepath\":\"data/layouts/Room/map.bin\"{version}}}"
            )
        };
        fs::write(
            dir.join(LAYOUTS_FILE),
            format!(
                "{{\"layouts_table_label\":\"gMapLayouts\",\"layouts\":[{},{}]}}",
                layout("LAYOUT_ROOM", ""),
                layout("LAYOUT_KANTO", ",\"layout_version\":\"frlg\"")
            ),
        )
        .unwrap();
        // Row 0: floor, wall, floor; row 1: wall, floor, water.
        let words: [u16; 6] = [3 << 12, (1 << 10) | (3 << 12), 3 << 12, (1 << 10) | (3 << 12), 3 << 12, 1 << 12];
        fs::write(dir.join("data/layouts/Room/map.bin"), crate::maps::write_words(&words)).unwrap();
        dir
    }

    fn map(id: &str, layout: &str, region: Option<&str>) -> Map {
        let mut properties = BTreeMap::new();
        if let Some(r) = region {
            properties.insert("region".to_owned(), Value::Text(r.to_owned()));
        }
        Map {
            id: MapId::from(id),
            name: id.into(),
            geometry: MapGeometry::Grid { layout: LayoutId(layout.into()), width: 3, height: 2 },
            events: vec![],
            connections: vec![],
            properties,
        }
    }

    fn snapshot(maps: Vec<Map>) -> BTreeMap<MapId, Map> {
        maps.into_iter().map(|m| (m.id.clone(), m)).collect()
    }

    #[test]
    fn an_emerald_build_leaves_out_other_regions_and_foreign_layouts() {
        let dir = project("rules", "MAP_VERSION  ?= emerald\n");
        let rules = MapRules::read(&dir).unwrap();
        assert_eq!(rules.refusal(&map("MAP_A", "LAYOUT_ROOM", None)), None);
        assert_eq!(rules.refusal(&map("MAP_A", "LAYOUT_ROOM", Some("REGION_HOENN"))), None);
        assert!(rules.refusal(&map("MAP_K", "LAYOUT_ROOM", Some("REGION_KANTO"))).unwrap().contains("REGION_KANTO"));
        assert!(rules.refusal(&map("MAP_K", "LAYOUT_KANTO", None)).unwrap().contains("frlg layout"));
        assert!(rules.refusal(&map("MAP_K", "LAYOUT_NOPE", None)).unwrap().contains("LAYOUT_NOPE"));
        assert!(rules.refusal(&map("MAP_ORPHAN", "LAYOUT_ROOM", None)).unwrap().contains("no group"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_firered_build_keeps_the_opposite_maps() {
        let dir = project("firered", "MAP_VERSION  ?= firered\n");
        let rules = MapRules::read(&dir).unwrap();
        assert_eq!(rules.refusal(&map("MAP_A", "LAYOUT_ROOM", None)), None, "no region means Kanto here");
        assert!(rules.refusal(&map("MAP_A", "LAYOUT_ROOM", Some("REGION_HOENN"))).is_some());
        assert_eq!(rules.refusal(&map("MAP_K", "LAYOUT_KANTO", Some("REGION_KANTO"))), None);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn boot_points_are_refused_before_the_boot_with_the_nearest_tile() {
        let dir = project("check", "MAP_VERSION  ?= emerald\n");
        let snap = snapshot(vec![map("MAP_ROOM", "LAYOUT_ROOM", None), map("MAP_KANTO", "LAYOUT_KANTO", None)]);
        check_boot_point(&dir, &snap, &MapId::from("MAP_ROOM"), 0, 0).unwrap();
        let wall = check_boot_point(&dir, &snap, &MapId::from("MAP_ROOM"), 1, 0).unwrap_err().0;
        assert!(wall.contains("solid") && wall.contains("nearest walkable tile is (0, 0)"), "{wall}");
        let water = check_boot_point(&dir, &snap, &MapId::from("MAP_ROOM"), 2, 1).unwrap_err().0;
        assert!(water.contains("water"), "{water}");
        assert!(check_boot_point(&dir, &snap, &MapId::from("MAP_ROOM"), 9, 9).unwrap_err().0.contains("outside the map"));
        let foreign = check_boot_point(&dir, &snap, &MapId::from("MAP_KANTO"), 0, 0).unwrap_err().0;
        assert!(foreign.contains("cannot boot this map") && foreign.contains("frlg"), "{foreign}");
        assert_eq!(
            assess_boot_point(&dir, &snap, &MapId::from("MAP_ROOM"), 1, 0).unwrap(),
            PointAssessment::Blocked { reason: Blocked::Solid, nearest: Some((0, 0)) }
        );
        assert!(check_boot_point(&dir, &snap, &MapId::from("MAP_NOPE"), 0, 0).is_err());

        let entries = map_usability(&dir, &snap).unwrap();
        let by = |id: &str| entries.iter().find(|e| e.0 .0 == id).unwrap().3.clone();
        assert_eq!(by("MAP_ROOM"), MapUsability { default_point: (1, 1), refusal: None });
        assert!(by("MAP_KANTO").refusal.is_some());
        let _ = fs::remove_dir_all(&dir);
    }
}
