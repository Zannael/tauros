//! What a connector's game supports, and a loaded project.

use crate::entities::{Flag, Map, Scene, Species, Trainer};
use crate::ids::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GameFamily {
    Gen1,
    Gen2,
    Gen3,
    Gen4,
}

/// Generic limits every connector declares; game-specific limits (map size, palette
/// slots) are enforced by the connector's own validation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capabilities {
    pub max_party_size: u8,
    pub max_moves: u8,
    pub min_level: u8,
    pub max_level: u8,
    pub held_items: bool,
    pub abilities: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameInfo {
    pub family: GameFamily,
    /// The decomp base and pinned revision, e.g. `pokeemerald-expansion 1.16.4`.
    pub engine: String,
    pub capabilities: Capabilities,
}

/// The model view of a project at one point in time.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectSnapshot {
    pub game: GameInfo,
    pub trainers: BTreeMap<TrainerId, Trainer>,
    pub maps: BTreeMap<MapId, Map>,
    pub scenes: BTreeMap<ScriptLabel, Scene>,
    pub species: BTreeMap<SpeciesId, Species>,
    pub flags: BTreeMap<FlagId, Flag>,
}
