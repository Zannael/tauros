//! The decomp connector contract (layer 4). Every game family implements it.

use crate::change::{ChangeSet, Severity, Violation};
use crate::game::{GameInfo, ProjectSnapshot};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;

/// Obligations, checked by [`crate::contract::run_all`]:
///
/// - `load` is deterministic and reflects the project files exactly.
/// - `apply` is atomic and lossless: it calls `validate` first and refuses any
///   change set with an [`Severity::Error`] violation without touching files;
///   re-applying an unchanged entity leaves every byte as it was; files and parts of
///   files no change refers to are never rewritten.
/// - Writes go through a snapshot so they can be undone (G3-001 for git projects),
///   and record the change set's provenance label.
/// - `fingerprint` changes if and only if the connector's source files change.
pub trait Connector {
    fn info(&self) -> GameInfo;
    fn load(&self) -> Result<ProjectSnapshot, ConnectorError>;
    /// Preflight the cumulative final state, including connector engine limits.
    /// Errors must prevent writes; warnings (such as uncertain palette pressure)
    /// remain visible to callers. Build implementations also preflight source data.
    fn validate(&self, changes: &ChangeSet) -> Vec<Violation>;
    fn apply(&mut self, changes: &ChangeSet) -> Result<Applied, ConnectorError>;
    fn build(&mut self) -> Result<BuildOutput, ConnectorError>;
    /// Runs the project's headless test suite, optionally limited to tests matching `filter`.
    fn run_tests(&mut self, _filter: Option<&str>) -> Result<TestOutput, ConnectorError> {
        Err(ConnectorError::NotFound("test runner is not supported by this connector".into()))
    }
    fn fingerprint(&self) -> Result<String, ConnectorError>;
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Applied {
    /// The commit recording the change, when the connector versions the project.
    pub commit: Option<String>,
    /// Project-relative paths that were rewritten.
    pub touched: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    pub file: Option<String>,
    pub line: Option<u32>,
    pub message: String,
    /// Whether the toolchain reported an error or a warning. Older payloads
    /// without it are read as errors.
    #[serde(default = "error_severity")]
    pub severity: Severity,
}

fn error_severity() -> Severity {
    Severity::Error
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildOutput {
    pub rom: PathBuf,
    pub symbols: Option<PathBuf>,
    pub rom_sha256: String,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TestCaseOutput {
    pub name: String,
    pub result: String,
    pub passed: bool,
    pub location: Option<(String, u32)>,
    pub output: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TestOutput {
    pub success: bool,
    pub cases: Vec<TestCaseOutput>,
    pub totals: BTreeMap<String, u32>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConnectorError {
    /// Validation found errors; nothing was written.
    Rejected(Vec<Violation>),
    NotFound(String),
    Format { file: String, line: Option<u32>, message: String },
    Io(String),
    /// The pre-write snapshot failed; nothing was written.
    Snapshot(String),
    Build(Vec<Diagnostic>),
}

impl fmt::Display for ConnectorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Rejected(v) => {
                let errors: Vec<_> = v.iter().filter(|v| v.severity == Severity::Error).collect();
                write!(f, "change set rejected ({} errors)", errors.len())?;
                for e in errors {
                    write!(f, "; {}: {}", e.rule, e.message)?;
                }
                Ok(())
            }
            Self::NotFound(what) => write!(f, "not found: {what}"),
            Self::Format { file, line, message } => match line {
                Some(line) => write!(f, "{file}:{line}: {message}"),
                None => write!(f, "{file}: {message}"),
            },
            Self::Io(message) => write!(f, "I/O error: {message}"),
            Self::Snapshot(message) => write!(f, "snapshot failed, nothing written: {message}"),
            Self::Build(d) => write!(f, "build failed with {} diagnostics", d.len()),
        }
    }
}

impl std::error::Error for ConnectorError {}
