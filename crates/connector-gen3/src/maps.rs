//! Porymap-format maps, layouts and metatiles (G3-003), reimplemented without
//! embedding Porymap.
//!
//! - `data/maps/<Map>/map.json` ⇄ [`Map`]: header fields, connections and the four
//!   event lists. Fields the model does not name ride in `properties` or the event's
//!   `extensions` under their JSON key, with the JSON type kept (`Text`, `Int`,
//!   `Bool`).
//! - `data/layouts/layouts.json` ⇄ [`Layout`].
//! - `map.bin`, `border.bin`, `metatiles.bin`, `metatile_attributes.bin` ⇄ little-endian
//!   words ([`Blockdata`], [`Metatiles`], [`MetatileAttributes`]).
//!
//! Writes go through [`json::Document`], so an unchanged map renders byte-identical
//! and an edit rewrites only the values that changed. Events are aligned old to new
//! with a longest common subsequence, so inserting or removing one event leaves the
//! others' text alone.

use crate::json::{align, Array, Document, Item, Node, Object, Step};
use std::collections::BTreeMap;
use tauros_model::{Connection, Direction, Event, EventKind, Extensions, FlagId, LayoutId, Map, MapGeometry, MapId, Position, ScriptLabel, Value};

pub const MAPS_DIR: &str = "data/maps";
pub const LAYOUTS_FILE: &str = "data/layouts/layouts.json";
pub const MAP_GROUPS_FILE: &str = "data/maps/map_groups.json";
/// Defines `WARP_ID_*` used by symbolic warp destinations.
pub const WARP_IDS_HEADER: &str = "include/constants/maps.h";

/// The four event arrays, in file order, with the model kind each holds.
pub const EVENT_ARRAYS: [(&str, EventKind); 4] = [
    ("object_events", EventKind::Npc),
    ("warp_events", EventKind::Warp),
    ("coord_events", EventKind::Trigger),
    ("bg_events", EventKind::Background),
];

/// Map header keys in Porymap's order; other keys follow in file order.
const MAP_KEYS: [&str; 4] = ["id", "name", "layout", "connections"];

const NPC_ORDER: [&str; 15] = [
    "local_id",
    "type",
    "graphics_id",
    "x",
    "y",
    "elevation",
    "movement_type",
    "movement_range_x",
    "movement_range_y",
    "trainer_type",
    "trainer_sight_or_berry_tree_id",
    "script",
    "flag",
    "target_local_id",
    "target_map",
];
const WARP_ORDER: [&str; 5] = ["x", "y", "elevation", "dest_map", "dest_warp_id"];
const COORD_ORDER: [&str; 8] = ["type", "x", "y", "elevation", "var", "var_value", "script", "weather"];
const BG_ORDER: [&str; 11] =
    ["type", "x", "y", "elevation", "player_facing_dir", "script", "item", "flag", "quantity", "underfoot", "secret_base_id"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapFormatError(pub String);

fn bad(message: impl Into<String>) -> MapFormatError {
    MapFormatError(message.into())
}

/// Symbolic warp ids (`WARP_ID_DYNAMIC` = 0x7F) read from the project.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WarpIds(pub BTreeMap<String, u16>);

impl WarpIds {
    pub fn parse(header: &str) -> Self {
        let mut ids = BTreeMap::new();
        for line in header.lines() {
            let mut words = line.split_whitespace();
            if let (Some("#define"), Some(name), Some(value)) = (words.next(), words.next(), words.next()) {
                if name.starts_with("WARP_ID_") {
                    if let Some(v) = parse_c_int(value) {
                        ids.insert(name.to_owned(), v as u16);
                    }
                }
            }
        }
        Self(ids)
    }

    fn resolve(&self, text: &str) -> Option<u16> {
        text.parse().ok().or_else(|| self.0.get(text).copied())
    }
}

pub fn parse_c_int(text: &str) -> Option<i64> {
    let t = text.trim();
    match t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        Some(hex) => i64::from_str_radix(hex, 16).ok(),
        None => t.parse().ok(),
    }
}

fn value_of(node: &Node) -> Option<Value> {
    match node {
        Node::Scalar(s) => {
            if let Some(text) = s.as_str() {
                Some(Value::Text(text))
            } else if let Some(b) = s.as_bool() {
                Some(Value::Bool(b))
            } else {
                s.as_i64().map(Value::Int)
            }
        }
        Node::Array(a) => a.items.iter().map(|i| value_of(&i.node)).collect::<Option<Vec<_>>>().map(Value::List),
        Node::Object(_) => None,
    }
}

fn node_of(value: &Value) -> Node {
    match value {
        Value::Bool(b) => Node::bool(*b),
        Value::Int(n) => Node::int(*n),
        Value::Text(t) => Node::string(t),
        Value::List(items) => Node::array(items.iter().map(node_of).collect(), ""),
    }
}

fn text(obj: &Object, key: &str) -> Option<String> {
    obj.get(key).and_then(Node::as_str)
}

fn int(obj: &Object, key: &str) -> Option<i64> {
    obj.get(key).and_then(Node::as_i64)
}

fn position(obj: &Object) -> Result<Position, MapFormatError> {
    let coord = |k| int(obj, k).ok_or_else(|| bad(format!("event without integer '{k}'")));
    Ok(Position {
        x: coord("x")? as i32,
        y: coord("y")? as i32,
        elevation: int(obj, "elevation").map(|e| e as u8),
    })
}

/// Collects the keys the model does not name into extensions.
fn extensions(obj: &Object, named: &[&str]) -> Result<Extensions, MapFormatError> {
    let mut ext = Extensions::new();
    for m in &obj.members {
        if named.contains(&m.key.as_str()) {
            continue;
        }
        let value = value_of(&m.node).ok_or_else(|| bad(format!("unsupported value for '{}'", m.key)))?;
        ext.insert(m.key.clone(), value);
    }
    Ok(ext)
}

/// "0" and "0x0" mean "none" for flags and scripts in Porymap files.
fn optional(value: Option<String>) -> Option<String> {
    value.filter(|v| !matches!(v.as_str(), "0" | "0x0" | "NULL" | ""))
}

fn event_from_json(kind: EventKind, obj: &Object, warp_ids: &WarpIds) -> Result<Event, MapFormatError> {
    let pos = position(obj)?;
    Ok(match kind {
        EventKind::Npc => Event::Npc {
            local_id: text(obj, "local_id"),
            graphics: text(obj, "graphics_id").ok_or_else(|| bad("object event without graphics_id"))?,
            position: pos,
            movement: text(obj, "movement_type"),
            script: optional(text(obj, "script")).map(ScriptLabel),
            visibility_flag: optional(text(obj, "flag")).map(FlagId),
            extensions: extensions(obj, &["local_id", "graphics_id", "x", "y", "elevation", "movement_type", "script", "flag"])?,
        },
        EventKind::Warp => {
            let dest = text(obj, "dest_warp_id").or_else(|| int(obj, "dest_warp_id").map(|n| n.to_string()));
            let dest = dest.ok_or_else(|| bad("warp without dest_warp_id"))?;
            Event::Warp {
                position: pos,
                destination_map: MapId(text(obj, "dest_map").ok_or_else(|| bad("warp without dest_map"))?),
                destination_warp: warp_ids.resolve(&dest).ok_or_else(|| bad(format!("unknown warp id {dest}")))?,
                extensions: extensions(obj, &["x", "y", "elevation", "dest_map", "dest_warp_id"])?,
            }
        }
        EventKind::Trigger => Event::Trigger {
            position: pos,
            variable: text(obj, "var"),
            value: text(obj, "var_value"),
            script: ScriptLabel(text(obj, "script").unwrap_or_default()),
            extensions: extensions(obj, &["x", "y", "elevation", "var", "var_value", "script"])?,
        },
        EventKind::Background => Event::Background {
            background: text(obj, "type").ok_or_else(|| bad("bg event without type"))?,
            position: pos,
            script: text(obj, "script").map(ScriptLabel),
            extensions: extensions(obj, &["type", "x", "y", "elevation", "script"])?,
        },
    })
}

fn ordered(mut pairs: BTreeMap<String, Node>, order: &[&str]) -> Vec<(String, Node)> {
    let mut out = Vec::new();
    for key in order {
        if let Some(node) = pairs.remove(*key) {
            out.push(((*key).to_owned(), node));
        }
    }
    out.extend(pairs);
    out
}

/// The JSON members an event is written as, in Porymap's order.
pub fn event_to_json(event: &Event) -> Vec<(String, Node)> {
    let mut pairs: BTreeMap<String, Node> = BTreeMap::new();
    let ext = match event {
        Event::Npc { extensions, .. }
        | Event::Warp { extensions, .. }
        | Event::Trigger { extensions, .. }
        | Event::Background { extensions, .. } => extensions,
    };
    for (key, value) in ext {
        pairs.insert(key.clone(), node_of(value));
    }
    let mut put = |key: &str, node: Node| {
        pairs.insert(key.to_owned(), node);
    };
    let pos = match event {
        Event::Npc { position, .. }
        | Event::Warp { position, .. }
        | Event::Trigger { position, .. }
        | Event::Background { position, .. } => position,
    };
    put("x", Node::int(pos.x as i64));
    put("y", Node::int(pos.y as i64));
    if let Some(e) = pos.elevation {
        put("elevation", Node::int(e as i64));
    }
    let order: &[&str] = match event {
        Event::Npc { local_id, graphics, movement, script, visibility_flag, extensions, .. } => {
            let clone = matches!(extensions.get("type"), Some(Value::Text(t)) if t == "clone");
            if let Some(id) = local_id {
                put("local_id", Node::string(id));
            }
            put("graphics_id", Node::string(graphics));
            if let Some(m) = movement {
                put("movement_type", Node::string(m));
            }
            if !clone {
                if !extensions.contains_key("movement_range_x") {
                    put("movement_range_x", Node::int(0));
                }
                if !extensions.contains_key("movement_range_y") {
                    put("movement_range_y", Node::int(0));
                }
                if !extensions.contains_key("trainer_type") {
                    put("trainer_type", Node::string("TRAINER_TYPE_NONE"));
                }
                if !extensions.contains_key("trainer_sight_or_berry_tree_id") {
                    put("trainer_sight_or_berry_tree_id", Node::string("0"));
                }
                put("script", Node::string(script.as_ref().map_or("0x0", |s| s.0.as_str())));
                put("flag", Node::string(visibility_flag.as_ref().map_or("0", |f| f.0.as_str())));
            }
            &NPC_ORDER
        }
        Event::Warp { destination_map, destination_warp, .. } => {
            put("dest_map", Node::string(&destination_map.0));
            put("dest_warp_id", Node::string(&destination_warp.to_string()));
            &WARP_ORDER
        }
        Event::Trigger { variable, value, script, extensions, .. } => {
            if !extensions.contains_key("type") {
                put("type", Node::string("trigger"));
            }
            if let Some(v) = variable {
                put("var", Node::string(v));
            }
            if let Some(v) = value {
                put("var_value", Node::string(v));
            }
            if !script.0.is_empty() {
                put("script", Node::string(&script.0));
            }
            &COORD_ORDER
        }
        Event::Background { background, script, .. } => {
            put("type", Node::string(background));
            if let Some(s) = script {
                put("script", Node::string(&s.0));
            }
            &BG_ORDER
        }
    };
    ordered(pairs, order)
}

/// Desired members for `new`, reusing the original node of every member whose
/// model-level value did not change (so `"NULL"` is not rewritten as `"0x0"`).
fn keep_unchanged(original: &Object, old: Vec<(String, Node)>, new: Vec<(String, Node)>) -> Vec<(String, Node)> {
    new.into_iter()
        .map(|(key, node)| {
            let unchanged = old.iter().any(|(k, n)| *k == key && n.same(&node));
            match original.get(&key) {
                Some(orig) if unchanged => (key, orig.clone()),
                _ => (key, node),
            }
        })
        .collect()
}

/// Rebuilds an array of objects from `old` to `new`, keeping the text of every
/// unchanged element and editing changed ones in place.
fn rebuild_array<T: PartialEq>(
    original: Option<&Node>,
    old: &[T],
    new: &[T],
    to_json: impl Fn(&T) -> Vec<(String, Node)>,
    own_indent: &str,
) -> Node {
    let empty = Array { items: Vec::new(), tail: String::new() };
    let source = original.and_then(Node::as_array).unwrap_or(&empty);
    let inner = source.item_indent(own_indent);
    let steps = align(old, new);
    // A moved element is a delete plus an insert of an equal value: reuse its text.
    let mut moved: Vec<usize> = steps.iter().filter_map(|s| if let Step::Delete(i) = s { Some(*i) } else { None }).collect();
    let mut items = Vec::new();
    for step in steps {
        match step {
            Step::Keep(i, _) => items.push(source.items[i].clone()),
            Step::Change(i, j) => {
                let mut item = source.items[i].clone();
                match item.node.as_object_mut() {
                    Some(obj) => {
                        let desired = keep_unchanged(obj, to_json(&old[i]), to_json(&new[j]));
                        obj.merge(desired, &inner, &[]);
                    }
                    None => item.node = Node::object(to_json(&new[j]), &inner),
                }
                items.push(item);
            }
            Step::Insert(j) => match moved.iter().position(|&i| old[i] == new[j]) {
                Some(at) => {
                    let i = moved.remove(at);
                    items.push(Item { lead: format!("\n{inner}"), node: source.items[i].node.clone(), trail: String::new() });
                }
                None => items.push(Item {
                    lead: format!("\n{inner}"),
                    node: Node::object(to_json(&new[j]), &inner),
                    trail: String::new(),
                }),
            },
            Step::Delete(_) => {}
        }
    }
    let tail = if items.is_empty() {
        String::new()
    } else if source.items.is_empty() {
        format!("\n{own_indent}")
    } else {
        source.tail.clone()
    };
    Node::Array(Array { items, tail })
}

fn connection_to_json(c: &Connection) -> Vec<(String, Node)> {
    vec![
        ("map".into(), Node::string(&c.map.0)),
        ("offset".into(), Node::int(c.offset as i64)),
        ("direction".into(), Node::string(direction_name(c.direction))),
    ]
}

fn direction_name(d: Direction) -> &'static str {
    match d {
        Direction::Up => "up",
        Direction::Down => "down",
        Direction::Left => "left",
        Direction::Right => "right",
        Direction::Dive => "dive",
        Direction::Emerge => "emerge",
    }
}

fn parse_direction(text: &str) -> Option<Direction> {
    Some(match text {
        "up" => Direction::Up,
        "down" => Direction::Down,
        "left" => Direction::Left,
        "right" => Direction::Right,
        "dive" => Direction::Dive,
        "emerge" => Direction::Emerge,
        _ => return None,
    })
}

/// One `map.json`, parsed and kept for lossless writing.
#[derive(Clone, Debug)]
pub struct MapFile {
    pub doc: Document,
}

impl MapFile {
    pub fn parse(text: &str) -> Result<Self, MapFormatError> {
        let doc = Document::parse(text).map_err(|e| bad(e.to_string()))?;
        if doc.root.as_object().is_none() {
            return Err(bad("map.json root is not an object"));
        }
        Ok(Self { doc })
    }

    pub fn render(&self) -> String {
        self.doc.render()
    }

    fn root(&self) -> &Object {
        self.doc.root.as_object().expect("checked in parse")
    }

    /// True when the map borrows another map's events (`shared_events_map`).
    pub fn shares_events(&self) -> bool {
        self.root().get("shared_events_map").is_some()
    }

    pub fn to_model(&self, layouts: &Layouts, warp_ids: &WarpIds) -> Result<Map, MapFormatError> {
        let root = self.root();
        let id = text(root, "id").ok_or_else(|| bad("map without id"))?;
        let layout = text(root, "layout").ok_or_else(|| bad(format!("{id}: no layout")))?;
        let (width, height) = layouts
            .get(&layout)
            .map(|l| (l.width, l.height))
            .ok_or_else(|| bad(format!("{id}: unknown layout {layout}")))?;
        let mut connections = Vec::new();
        if let Some(Node::Array(list)) = root.get("connections") {
            for item in &list.items {
                let obj = item.node.as_object().ok_or_else(|| bad(format!("{id}: connection is not an object")))?;
                let direction = text(obj, "direction").and_then(|d| parse_direction(&d));
                connections.push(Connection {
                    direction: direction.ok_or_else(|| bad(format!("{id}: bad connection direction")))?,
                    offset: int(obj, "offset").ok_or_else(|| bad(format!("{id}: connection without offset")))? as i32,
                    map: MapId(text(obj, "map").ok_or_else(|| bad(format!("{id}: connection without map")))?),
                });
            }
        }
        let mut events = Vec::new();
        for (key, kind) in EVENT_ARRAYS {
            if let Some(Node::Array(list)) = root.get(key) {
                for item in &list.items {
                    let obj = item.node.as_object().ok_or_else(|| bad(format!("{id}: {key} item is not an object")))?;
                    events.push(event_from_json(kind, obj, warp_ids).map_err(|e| bad(format!("{id}: {key}: {}", e.0)))?);
                }
            }
        }
        let mut properties = Extensions::new();
        for m in &root.members {
            if MAP_KEYS.contains(&m.key.as_str()) || EVENT_ARRAYS.iter().any(|(k, _)| *k == m.key) {
                continue;
            }
            if m.node.as_scalar().is_some_and(|s| s.is_null()) {
                continue;
            }
            let value = value_of(&m.node).ok_or_else(|| bad(format!("{id}: unsupported value for '{}'", m.key)))?;
            properties.insert(m.key.clone(), value);
        }
        Ok(Map {
            id: MapId(id),
            name: text(root, "name").unwrap_or_default(),
            geometry: MapGeometry::Grid { layout: LayoutId(layout), width, height },
            events,
            connections,
            properties,
        })
    }

    /// Rewrites the file from `old` (what it holds now) to `new`, touching only
    /// values that differ. The layout's size is not written here.
    pub fn apply(&mut self, old: &Map, new: &Map) -> Result<(), MapFormatError> {
        if old == new {
            return Ok(());
        }
        let layout = match &new.geometry {
            MapGeometry::Grid { layout, .. } => layout.0.clone(),
            MapGeometry::External { .. } => return Err(bad("Gen 3 maps are grids")),
        };
        let root = self.doc.root.as_object_mut().expect("checked in parse");
        let indent = "";
        let member_indent = root.member_indent(indent);
        let mut desired: Vec<(String, Node)> = Vec::new();
        let mut exact: Vec<&str> = Vec::new();
        let mut old_pairs: Vec<(String, Node)> = vec![
            ("id".into(), Node::string(&old.id.0)),
            ("name".into(), Node::string(&old.name)),
        ];
        if let MapGeometry::Grid { layout, .. } = &old.geometry {
            old_pairs.push(("layout".into(), Node::string(&layout.0)));
        }
        old_pairs.extend(old.properties.iter().map(|(k, v)| (k.clone(), node_of(v))));
        let mut new_pairs: Vec<(String, Node)> =
            vec![("id".into(), Node::string(&new.id.0)), ("name".into(), Node::string(&new.name)), ("layout".into(), Node::string(&layout))];
        new_pairs.extend(new.properties.iter().map(|(k, v)| (k.clone(), node_of(v))));
        let scalars = keep_unchanged(root, old_pairs, new_pairs);
        // Walk the existing members in order so array keys stay in place.
        let mut scalars: BTreeMap<String, Node> = scalars.into_iter().collect();
        for m in &root.members {
            let key = m.key.as_str();
            if key == "connections" {
                let node = if old.connections == new.connections {
                    m.node.clone()
                } else {
                    rebuild_array(Some(&m.node), &old.connections, &new.connections, connection_to_json, &member_indent)
                };
                desired.push(("connections".into(), node));
                exact.push("connections");
            } else if let Some((_, kind)) = EVENT_ARRAYS.iter().find(|(k, _)| *k == key) {
                let olds: Vec<&Event> = old.events.iter().filter(|e| e.kind() == *kind).collect();
                let news: Vec<&Event> = new.events.iter().filter(|e| e.kind() == *kind).collect();
                let node = if olds == news {
                    m.node.clone()
                } else {
                    rebuild_array(Some(&m.node), &olds, &news, |e| event_to_json(e), &member_indent)
                };
                desired.push((key.to_owned(), node));
                exact.push(EVENT_ARRAYS.iter().find(|(k, _)| *k == key).unwrap().0);
            } else if let Some(node) = scalars.remove(key) {
                desired.push((key.to_owned(), node));
            } else if m.node.as_scalar().is_some_and(|s| s.is_null()) {
                desired.push((key.to_owned(), m.node.clone()));
            }
        }
        // New header fields, then connections and event arrays the file lacked.
        desired.extend(scalars);
        if root.get("connections").is_none() && !new.connections.is_empty() {
            desired.push(("connections".into(), rebuild_array(None, &[], &new.connections, connection_to_json, &member_indent)));
            exact.push("connections");
        }
        for (key, kind) in EVENT_ARRAYS {
            let news: Vec<&Event> = new.events.iter().filter(|e| e.kind() == kind).collect();
            if root.get(key).is_none() && !news.is_empty() {
                if root.get("shared_events_map").is_some() {
                    return Err(bad(format!("{} uses shared_events_map; edit that map's events", new.id)));
                }
                desired.push((key.to_owned(), rebuild_array(None, &[], &news, |e| event_to_json(e), &member_indent)));
                exact.push(key);
            }
        }
        root.merge(desired, indent, &exact);
        Ok(())
    }
}

/// One entry of `layouts.json`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layout {
    pub id: String,
    pub name: String,
    pub width: u16,
    pub height: u16,
    /// Absent in files that use the default 2×2 border.
    pub border_width: Option<u16>,
    pub border_height: Option<u16>,
    pub primary_tileset: String,
    pub secondary_tileset: String,
    pub border_filepath: String,
    pub blockdata_filepath: String,
}

impl Layout {
    pub fn border_size(&self) -> (u16, u16) {
        (self.border_width.unwrap_or(2), self.border_height.unwrap_or(2))
    }
}

/// `layouts.json`, parsed and kept for lossless writing.
#[derive(Clone, Debug)]
pub struct Layouts {
    pub doc: Document,
    list: Vec<Layout>,
}

impl Layouts {
    pub fn parse(source: &str) -> Result<Self, MapFormatError> {
        let doc = Document::parse(source).map_err(|e| bad(e.to_string()))?;
        let mut list = Vec::new();
        let items = doc
            .root
            .as_object()
            .and_then(|o| o.get("layouts"))
            .and_then(Node::as_array)
            .ok_or_else(|| bad("layouts.json has no 'layouts' array"))?;
        for item in &items.items {
            let Some(obj) = item.node.as_object() else { continue };
            if obj.members.is_empty() {
                continue;
            }
            let field = |k: &str| text(obj, k).ok_or_else(|| bad(format!("layout without '{k}'")));
            let size = |k: &str| int(obj, k).map(|n| n as u16);
            list.push(Layout {
                id: field("id")?,
                name: field("name")?,
                width: size("width").ok_or_else(|| bad("layout without width"))?,
                height: size("height").ok_or_else(|| bad("layout without height"))?,
                border_width: size("border_width"),
                border_height: size("border_height"),
                primary_tileset: field("primary_tileset")?,
                secondary_tileset: field("secondary_tileset")?,
                border_filepath: field("border_filepath")?,
                blockdata_filepath: field("blockdata_filepath")?,
            });
        }
        Ok(Self { doc, list })
    }

    pub fn render(&self) -> String {
        self.doc.render()
    }

    pub fn all(&self) -> &[Layout] {
        &self.list
    }

    pub fn get(&self, id: &str) -> Option<&Layout> {
        self.list.iter().find(|l| l.id == id)
    }

    /// Updates an existing layout's fields in place (unknown keys are kept).
    pub fn update(&mut self, layout: &Layout) -> Result<(), MapFormatError> {
        let index = self.list.iter().position(|l| l.id == layout.id).ok_or_else(|| bad(format!("no layout {}", layout.id)))?;
        if self.list[index] == *layout {
            return Ok(());
        }
        let root = self.doc.root.as_object_mut().ok_or_else(|| bad("layouts.json root"))?;
        let indent = root.member_indent("");
        let Some(Node::Array(array)) = root.get_mut("layouts") else { return Err(bad("no layouts array")) };
        let item_indent = array.item_indent(&indent);
        let obj = array
            .items
            .iter_mut()
            .filter_map(|i| i.node.as_object_mut())
            .find(|o| text(o, "id").as_deref() == Some(layout.id.as_str()))
            .ok_or_else(|| bad(format!("no layout {}", layout.id)))?;
        let mut fields = vec![
            ("name", Node::string(&layout.name)),
            ("width", Node::int(layout.width as i64)),
            ("height", Node::int(layout.height as i64)),
            ("primary_tileset", Node::string(&layout.primary_tileset)),
            ("secondary_tileset", Node::string(&layout.secondary_tileset)),
            ("border_filepath", Node::string(&layout.border_filepath)),
            ("blockdata_filepath", Node::string(&layout.blockdata_filepath)),
        ];
        if let Some(w) = layout.border_width {
            fields.push(("border_width", Node::int(w as i64)));
        }
        if let Some(h) = layout.border_height {
            fields.push(("border_height", Node::int(h as i64)));
        }
        for (key, node) in fields {
            obj.set(key, node, &item_indent);
        }
        self.list[index] = layout.clone();
        Ok(())
    }
}

/// `map.bin` / `border.bin`: one little-endian word per block.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Blockdata(pub Vec<u16>);

/// One map block, with the masks of include/global.fieldmap.h.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Block {
    pub metatile: u16,
    pub collision: u8,
    pub elevation: u8,
}

impl Block {
    pub fn unpack(word: u16) -> Self {
        Self { metatile: word & 0x03FF, collision: ((word & 0x0C00) >> 10) as u8, elevation: (word >> 12) as u8 }
    }

    pub fn pack(self) -> u16 {
        (self.metatile & 0x03FF) | ((self.collision as u16 & 0x3) << 10) | ((self.elevation as u16 & 0xF) << 12)
    }
}

pub fn read_words(bytes: &[u8]) -> Result<Vec<u16>, MapFormatError> {
    if !bytes.len().is_multiple_of(2) {
        return Err(bad("odd byte count in a 16-bit binary file"));
    }
    Ok(bytes.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect())
}

pub fn write_words(words: &[u16]) -> Vec<u8> {
    words.iter().flat_map(|w| w.to_le_bytes()).collect()
}

impl Blockdata {
    pub fn parse(bytes: &[u8]) -> Result<Self, MapFormatError> {
        read_words(bytes).map(Self)
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        write_words(&self.0)
    }

    pub fn block(&self, index: usize) -> Option<Block> {
        self.0.get(index).map(|w| Block::unpack(*w))
    }
}

/// `metatiles.bin`: 8 tile references per metatile (`NUM_TILES_PER_METATILE`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Metatiles(pub Vec<u16>);

impl Metatiles {
    pub const TILES_PER_METATILE: usize = 8;

    pub fn parse(bytes: &[u8]) -> Result<Self, MapFormatError> {
        let words = read_words(bytes)?;
        if words.len() % Self::TILES_PER_METATILE != 0 {
            return Err(bad("metatiles.bin is not a whole number of metatiles"));
        }
        Ok(Self(words))
    }

    pub fn count(&self) -> usize {
        self.0.len() / Self::TILES_PER_METATILE
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        write_words(&self.0)
    }
}

/// `metatile_attributes.bin`: 16-bit per metatile in Emerald layouts, 32-bit in
/// FRLG layouts (include/global.fieldmap.h `METATILE_ATTR_*_FRLG`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MetatileAttributes {
    pub bytes_per_metatile: usize,
    pub values: Vec<u32>,
}

impl MetatileAttributes {
    pub fn parse(bytes: &[u8], metatiles: usize) -> Result<Self, MapFormatError> {
        let width = match (metatiles, bytes.len()) {
            (0, 0) => 2,
            (n, len) if len == n * 2 => 2,
            (n, len) if len == n * 4 => 4,
            (n, len) => return Err(bad(format!("{len} attribute bytes for {n} metatiles"))),
        };
        let values = bytes
            .chunks_exact(width)
            .map(|c| if width == 2 { u16::from_le_bytes([c[0], c[1]]) as u32 } else { u32::from_le_bytes([c[0], c[1], c[2], c[3]]) })
            .collect();
        Ok(Self { bytes_per_metatile: width, values })
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        self.values
            .iter()
            .flat_map(|v| if self.bytes_per_metatile == 2 { (*v as u16).to_le_bytes().to_vec() } else { v.to_le_bytes().to_vec() })
            .collect()
    }

    /// Behaviour id (`METATILE_ATTR_BEHAVIOR_MASK`, or the FRLG mask).
    pub fn behavior(&self, index: usize) -> Option<u32> {
        let mask = if self.bytes_per_metatile == 2 { 0x00FF } else { 0x01FF };
        self.values.get(index).map(|v| v & mask)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LAYOUTS: &str = "{\n  \"layouts_table_label\": \"gMapLayouts\",\n  \"layouts\": [\n    {\n      \"id\": \"LAYOUT_A\",\n      \"name\": \"A_Layout\",\n      \"width\": 20,\n      \"height\": 10,\n      \"primary_tileset\": \"gTileset_General\",\n      \"secondary_tileset\": \"gTileset_Petalburg\",\n      \"border_filepath\": \"data/layouts/A/border.bin\",\n      \"blockdata_filepath\": \"data/layouts/A/map.bin\",\n      \"layout_version\": \"emerald\"\n    }\n  ]\n}\n";

    const MAP: &str = "{\n  \"id\": \"MAP_A\",\n  \"name\": \"A\",\n  \"layout\": \"LAYOUT_A\",\n  \"music\": \"MUS_A\",\n  \"requires_flash\": false,\n  \"connections\": null,\n  \"object_events\": [\n    {\n      \"graphics_id\": \"OBJ_EVENT_GFX_BOY_1\",\n      \"x\": 1,\n      \"y\": 2,\n      \"elevation\": 3,\n      \"movement_type\": \"MOVEMENT_TYPE_NONE\",\n      \"movement_range_x\": 0,\n      \"movement_range_y\": 0,\n      \"trainer_type\": \"TRAINER_TYPE_NONE\",\n      \"trainer_sight_or_berry_tree_id\": \"0\",\n      \"script\": \"A_EventScript_Boy\",\n      \"flag\": \"0\"\n    }\n  ],\n  \"warp_events\": [\n    {\n      \"x\": 4,\n      \"y\": 5,\n      \"elevation\": 0,\n      \"dest_map\": \"MAP_A\",\n      \"dest_warp_id\": \"WARP_ID_DYNAMIC\"\n    }\n  ],\n  \"coord_events\": [\n\n  ],\n  \"bg_events\": []\n}\n";

    fn load() -> (MapFile, Map, Layouts, WarpIds) {
        let layouts = Layouts::parse(LAYOUTS).unwrap();
        let warps = WarpIds::parse("#define WARP_ID_DYNAMIC     0x7F\n");
        let file = MapFile::parse(MAP).unwrap();
        let map = file.to_model(&layouts, &warps).unwrap();
        (file, map, layouts, warps)
    }

    #[test]
    fn map_roundtrips_and_maps_onto_the_model() {
        let (mut file, map, layouts, _) = load();
        assert_eq!(file.render(), MAP);
        assert_eq!(layouts.render(), LAYOUTS);
        assert_eq!(map.geometry, MapGeometry::Grid { layout: LayoutId("LAYOUT_A".into()), width: 20, height: 10 });
        assert_eq!(map.events.len(), 2);
        assert!(matches!(&map.events[1], Event::Warp { destination_warp: 0x7F, .. }));
        assert_eq!(map.properties["requires_flash"], Value::Bool(false));
        file.apply(&map, &map.clone()).unwrap();
        assert_eq!(file.render(), MAP);
    }

    #[test]
    fn adding_an_event_only_adds_its_lines() {
        let (mut file, map, layouts, warps) = load();
        let mut edited = map.clone();
        edited.events.push(Event::Background {
            background: "sign".into(),
            position: Position { x: 7, y: 8, elevation: Some(0) },
            script: Some(ScriptLabel("A_EventScript_Sign".into())),
            extensions: [("player_facing_dir".to_owned(), Value::Text("BG_EVENT_PLAYER_FACING_ANY".into()))].into(),
        });
        edited.properties.insert("music".into(), Value::Text("MUS_B".into()));
        file.apply(&map, &edited).unwrap();
        let out = file.render();
        assert!(out.contains("  \"music\": \"MUS_B\",\n"));
        assert!(out.ends_with("  \"bg_events\": [\n    {\n      \"type\": \"sign\",\n      \"x\": 7,\n      \"y\": 8,\n      \"elevation\": 0,\n      \"player_facing_dir\": \"BG_EVENT_PLAYER_FACING_ANY\",\n      \"script\": \"A_EventScript_Sign\"\n    }\n  ]\n}\n"));
        let reread = MapFile::parse(&out).unwrap().to_model(&layouts, &warps).unwrap();
        assert_eq!(reread, edited);
        // The warp keeps its symbolic destination.
        assert!(out.contains("\"dest_warp_id\": \"WARP_ID_DYNAMIC\""));
    }

    #[test]
    fn removing_an_event_leaves_the_others() {
        let (mut file, map, layouts, warps) = load();
        let mut edited = map.clone();
        edited.events.remove(0);
        file.apply(&map, &edited).unwrap();
        let out = file.render();
        assert!(out.contains("  \"object_events\": [],\n  \"warp_events\": [\n    {\n      \"x\": 4,"));
        assert_eq!(MapFile::parse(&out).unwrap().to_model(&layouts, &warps).unwrap(), edited);
    }

    #[test]
    fn binaries_roundtrip() {
        let bytes = [0x01, 0x30, 0xff, 0x0f];
        let data = Blockdata::parse(&bytes).unwrap();
        assert_eq!(data.block(0), Some(Block { metatile: 1, collision: 0, elevation: 3 }));
        assert_eq!(data.block(1).unwrap().pack(), 0x0fff);
        assert_eq!(data.to_bytes(), bytes);
        let attrs = MetatileAttributes::parse(&[1, 0, 0, 0x60], 1).unwrap();
        assert_eq!((attrs.bytes_per_metatile, attrs.behavior(0)), (4, Some(1)));
        assert_eq!(attrs.to_bytes(), [1, 0, 0, 0x60]);
    }
}
