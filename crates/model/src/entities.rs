//! Entities shared by every game: trainer, party, map, event, scene, species, flag.

use crate::ids::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A game-specific value the common model does not name.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Value {
    Bool(bool),
    Int(i64),
    Text(String),
    List(Vec<Value>),
}

/// Game-specific fields keyed by the source's own field name, e.g. `iv_scale` (DS),
/// `Mugshot` (expansion) or `requires_flash` (Gen 3 map). Sorted for determinism.
pub type Extensions = BTreeMap<String, Value>;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Trainer {
    pub id: TrainerId,
    pub name: String,
    /// Trainer class as written by the source (`Leader`, `TRAINER_CLASS_ACE_TRAINER_MALE`).
    pub class: Option<String>,
    pub items: Vec<ItemId>,
    pub double_battle: bool,
    pub ai_flags: Vec<String>,
    pub party: Party,
    #[serde(default)]
    pub extensions: Extensions,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Party {
    pub members: Vec<PartyMember>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartyMember {
    pub species: SpeciesId,
    pub level: u8,
    /// `None` when the game has no held items (Gen 1) or none is set.
    pub held_item: Option<ItemId>,
    /// Empty means "the game's default moveset for this level".
    pub moves: Vec<MoveId>,
    pub nickname: Option<String>,
    pub ability: Option<String>,
    #[serde(default)]
    pub extensions: Extensions,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Map {
    pub id: MapId,
    pub name: String,
    pub geometry: MapGeometry,
    pub events: Vec<Event>,
    pub connections: Vec<Connection>,
    /// Weather, music, map type and similar header fields, by source name.
    #[serde(default)]
    pub properties: Extensions,
}

/// Where a map's terrain lives. Tile grids are shared by Gen 1-3; DS 3D maps stay
/// outside the shared model and are edited by a dedicated editor.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MapGeometry {
    Grid { layout: LayoutId, width: u16, height: u16 },
    External { format: String, reference: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Position {
    pub x: i32,
    pub y: i32,
    /// Gen 3+ elevation layer; `None` where the game has none.
    pub elevation: Option<u8>,
}

/// Map events. Their order within a map matters (it is how scripts refer to them),
/// so they are addressed by kind and index.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Event {
    Npc {
        local_id: Option<String>,
        graphics: String,
        position: Position,
        movement: Option<String>,
        script: Option<ScriptLabel>,
        /// Flag that hides the NPC when set.
        visibility_flag: Option<FlagId>,
        #[serde(default)]
        extensions: Extensions,
    },
    Warp {
        position: Position,
        destination_map: MapId,
        destination_warp: u16,
        #[serde(default)]
        extensions: Extensions,
    },
    Trigger {
        position: Position,
        variable: Option<String>,
        value: Option<String>,
        script: ScriptLabel,
        #[serde(default)]
        extensions: Extensions,
    },
    /// Signs, hidden items and other background events (`kind` is the source's type).
    Background {
        background: String,
        position: Position,
        script: Option<ScriptLabel>,
        #[serde(default)]
        extensions: Extensions,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    Npc,
    Warp,
    Trigger,
    Background,
}

impl Event {
    pub fn kind(&self) -> EventKind {
        match self {
            Event::Npc { .. } => EventKind::Npc,
            Event::Warp { .. } => EventKind::Warp,
            Event::Trigger { .. } => EventKind::Trigger,
            Event::Background { .. } => EventKind::Background,
        }
    }

    pub fn position(&self) -> &Position {
        match self {
            Event::Npc { position, .. }
            | Event::Warp { position, .. }
            | Event::Trigger { position, .. }
            | Event::Background { position, .. } => position,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
    Dive,
    Emerge,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Connection {
    pub direction: Direction,
    pub offset: i32,
    pub map: MapId,
}

/// A scripted scene: one labelled script in the game's scripting language.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scene {
    pub label: ScriptLabel,
    pub map: Option<MapId>,
    pub language: ScriptLanguage,
    pub source: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScriptLanguage {
    /// Poryscript (`.pory`), compiled by the Poryscript CLI (Gen 3).
    Poryscript,
    /// The decomp's own assembly macros (`.inc` in Gen 3, `.asm` in Gen 1-2).
    Assembly,
    /// Anything else, named by the connector (e.g. a DS script format).
    Other(String),
}

/// Species are read-only reference data for validation and pickers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Species {
    pub id: SpeciesId,
    pub name: String,
    pub national_dex: Option<u16>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Flag {
    pub id: FlagId,
    /// Numeric value when the source defines one.
    pub value: Option<u32>,
    /// Whether any script or event refers to it (allocation must avoid these).
    pub in_use: bool,
}
