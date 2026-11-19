//! Layer 4: the Gen 3 connector for pokeemerald-expansion projects. It reads and
//! writes project files losslessly into `tauros-model` types (G3-002..G3-005) and
//! depends only on the model. All writes go through [`snapshot::GitGuard`].
//! [`boot`] (EMU-003) also drives an emulator through the `tauros-emu` trait, to
//! start a built ROM at a chosen map with chosen flags.

pub mod boot;
pub mod flags;
pub mod json;
pub mod maps;
pub mod limits;
pub mod names;
pub mod party;
pub mod project;
pub mod scripts;
pub mod snapshot;
pub mod toolchain;
pub mod trainers;

pub use project::Gen3Connector;
