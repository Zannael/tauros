//! Layer 3: the common semantic model shared by every game, and the contract that
//! layer-4 connectors implement (MOD-001).
//!
//! The model is a *view*: connectors keep the project's own files as the source of
//! truth and apply [`ChangeSet`]s to them losslessly, so anything the model does not
//! express survives untouched. Game-specific fields travel in [`Extensions`].
//! Identifiers are the game's own constants (`TRAINER_ROXANNE_1`, `SPECIES_GEODUDE`,
//! `MAP_ROUTE101`); connectors translate display names to constants.
//!
//! How each type maps onto Gen 3, Gen 1-2 and DS sources is recorded in
//! `roadmap/docs/master.md` ("Semantic model mapping (MOD-001)").

pub mod change;
pub mod connector;
pub mod contract;
pub mod entities;
pub mod game;
pub mod ids;
pub mod memory;
pub mod provenance;
pub mod validate;

pub use change::{Change, ChangeSet, Severity, Violation};
pub use connector::{
    Applied, BuildOutput, Connector, ConnectorError, Diagnostic, TestCaseOutput, TestOutput,
};
pub use entities::*;
pub use game::{Capabilities, GameFamily, GameInfo, ProjectSnapshot};
pub use ids::*;
pub use provenance::Provenance;
