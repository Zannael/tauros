//! Change sets: the only way to modify a project through a connector.

use crate::entities::{Event, EventKind, Extensions, Map, Scene, Trainer};
use crate::ids::*;
use crate::provenance::Provenance;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Change {
    UpsertTrainer { trainer: Trainer },
    RemoveTrainer { id: TrainerId },
    /// Creates the map when its id is new. Its grid must name a layout that exists
    /// (or one created earlier in the same change set) with the same size.
    UpsertMap { map: Map },
    /// Creates a layout (terrain grid) of `width` x `height`; game-specific fields
    /// (tilesets, name, fill block) travel in `properties`.
    CreateLayout {
        layout: LayoutId,
        width: u16,
        height: u16,
        #[serde(default)]
        properties: Extensions,
    },
    /// Resizes a layout, keeping its top-left corner: growth adds blocks at the
    /// right and bottom, shrinking drops them. Every map on the layout follows.
    ResizeLayout {
        layout: LayoutId,
        width: u16,
        height: u16,
        #[serde(default)]
        properties: Extensions,
    },
    AddEvent { map: MapId, event: Event },
    RemoveEvent { map: MapId, kind: EventKind, index: usize },
    UpsertScene { scene: Scene },
    RemoveScene { label: ScriptLabel },
    /// Reserve a new flag; the connector chooses a free numeric value.
    AllocateFlag { id: FlagId },
}

/// A unit of work: applied atomically and recorded as one commit with its label.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangeSet {
    /// One line for the commit message and history view.
    pub summary: String,
    pub provenance: Provenance,
    pub changes: Vec<Change>,
}

impl ChangeSet {
    pub fn new(summary: impl Into<String>, provenance: Provenance) -> Self {
        Self { summary: summary.into(), provenance, changes: Vec::new() }
    }

    pub fn with(mut self, change: Change) -> Self {
        self.changes.push(change);
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// Allowed, but the user should know (e.g. palette pressure).
    Warning,
    /// The change set must not be applied.
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Violation {
    /// Index into `ChangeSet::changes`, when one change is responsible.
    pub change: Option<usize>,
    /// Stable rule name, e.g. `party.max_size`, `map.size_ceiling`.
    pub rule: String,
    pub severity: Severity,
    pub message: String,
}

impl Violation {
    pub fn error(change: Option<usize>, rule: &str, message: impl Into<String>) -> Self {
        Self { change, rule: rule.to_owned(), severity: Severity::Error, message: message.into() }
    }

    pub fn warning(change: Option<usize>, rule: &str, message: impl Into<String>) -> Self {
        Self { change, rule: rule.to_owned(), severity: Severity::Warning, message: message.into() }
    }
}
