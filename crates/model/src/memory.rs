//! An in-memory reference connector. It defines the contract's expected behaviour
//! and lets the contract suite be tested without a real game project.

use crate::change::{ChangeSet, Severity, Violation};
use crate::connector::{Applied, BuildOutput, Connector, ConnectorError};
use crate::game::{GameInfo, ProjectSnapshot};
use crate::validate;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

#[derive(Clone, Debug)]
pub struct MemoryConnector {
    state: ProjectSnapshot,
    /// Recorded (summary, provenance label) per applied change set, like commits.
    pub history: Vec<(String, String)>,
}

impl MemoryConnector {
    pub fn new(state: ProjectSnapshot) -> Self {
        Self { state, history: Vec::new() }
    }
}

impl Connector for MemoryConnector {
    fn info(&self) -> GameInfo {
        self.state.game.clone()
    }

    fn load(&self) -> Result<ProjectSnapshot, ConnectorError> {
        Ok(self.state.clone())
    }

    fn validate(&self, changes: &ChangeSet) -> Vec<Violation> {
        validate::check(&self.state, changes)
    }

    fn apply(&mut self, changes: &ChangeSet) -> Result<Applied, ConnectorError> {
        let next = validate::simulate(&self.state, changes).map_err(ConnectorError::Rejected)?;
        if next != self.state {
            self.state = next;
            self.history.push((changes.summary.clone(), changes.provenance.label().to_owned()));
        }
        Ok(Applied { commit: None, touched: Vec::new() })
    }

    fn build(&mut self) -> Result<BuildOutput, ConnectorError> {
        Err(ConnectorError::Build(Vec::new()))
    }

    fn fingerprint(&self) -> Result<String, ConnectorError> {
        // BTreeMaps make the debug rendering deterministic within one build.
        let mut hasher = DefaultHasher::new();
        format!("{:?}", self.state).hash(&mut hasher);
        Ok(format!("{:016x}", hasher.finish()))
    }
}

/// True when any violation is an error.
pub fn has_errors(violations: &[Violation]) -> bool {
    violations.iter().any(|v| v.severity == Severity::Error)
}
