//! Identifiers: the game's own constant names, wrapped so they cannot be mixed up.

use serde::{Deserialize, Serialize};
use std::fmt;

macro_rules! id {
    ($($(#[$doc:meta])* $name:ident;)*) => {$(
        $(#[$doc])*
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub String);

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self(value.to_owned())
            }
        }
    )*};
}

id! {
    /// A trainer constant, e.g. `TRAINER_ROXANNE_1` (Gen 3), `GIOVANNI` party index
    /// names (Gen 1-2) or the trainer file stem (DS).
    TrainerId;
    /// A species constant, e.g. `SPECIES_GEODUDE`.
    SpeciesId;
    /// A move constant, e.g. `MOVE_ROCK_TOMB`.
    MoveId;
    /// An item constant, e.g. `ITEM_ORAN_BERRY`.
    ItemId;
    /// A map constant, e.g. `MAP_ROUTE101`.
    MapId;
    /// A map layout identifier, e.g. `LAYOUT_ROUTE101`.
    LayoutId;
    /// An event flag constant, e.g. `FLAG_DEFEATED_RIVAL`.
    FlagId;
    /// A script label, e.g. `Route101_EventScript_Youngster`.
    ScriptLabel;
}
