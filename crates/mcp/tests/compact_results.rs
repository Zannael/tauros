//! MCP-006: build and test results stay bounded however noisy the toolchain is.
use std::collections::BTreeMap;
use std::path::PathBuf;

use tauros_mcp::domain::{self, RunTestRequest, MAX_CASES, MAX_ERRORS, MAX_MESSAGE_CHARS, MAX_WARNINGS};
use tauros_model::memory::MemoryConnector;
use tauros_model::{
    Applied, BuildOutput, Capabilities, ChangeSet, Connector, ConnectorError, Diagnostic, GameFamily, GameInfo,
    ProjectSnapshot, Severity, TestCaseOutput, TestOutput, Violation,
};

fn diagnostic(severity: Severity, message: String) -> Diagnostic {
    Diagnostic { file: Some("src/x.c".into()), line: Some(1), message, severity }
}

/// Like the real Emerald build: hundreds of repeated linker and compiler warnings.
fn noisy(errors: usize) -> Vec<Diagnostic> {
    let mut all: Vec<Diagnostic> = (0..413)
        .map(|i| diagnostic(Severity::Warning, format!("warning: arm-none-eabi-ld: warning: note {}", i % 37)))
        .collect();
    all.push(diagnostic(Severity::Warning, format!("warning: {}", "x".repeat(5000))));
    all.extend((0..errors).map(|i| diagnostic(Severity::Error, format!("error: undefined reference {i}"))));
    all
}

struct Noisy {
    inner: MemoryConnector,
    fail: bool,
}

impl Connector for Noisy {
    fn info(&self) -> GameInfo { self.inner.info() }
    fn load(&self) -> Result<ProjectSnapshot, ConnectorError> { self.inner.load() }
    fn validate(&self, changes: &ChangeSet) -> Vec<Violation> { self.inner.validate(changes) }
    fn apply(&mut self, changes: &ChangeSet) -> Result<Applied, ConnectorError> { self.inner.apply(changes) }
    fn fingerprint(&self) -> Result<String, ConnectorError> { self.inner.fingerprint() }
    fn build(&mut self) -> Result<BuildOutput, ConnectorError> {
        if self.fail {
            return Err(ConnectorError::Build(noisy(120)));
        }
        Ok(BuildOutput {
            rom: PathBuf::from("pokeemerald.gba"),
            symbols: Some(PathBuf::from("pokeemerald.sym")),
            rom_sha256: "ab".repeat(32),
            diagnostics: noisy(0),
        })
    }
    fn run_tests(&mut self, _filter: Option<&str>) -> Result<TestOutput, ConnectorError> {
        let cases = (0..400).map(|i| TestCaseOutput {
            name: format!("case {i}"),
            result: if i % 100 == 99 { "FAIL" } else { "PASS" }.into(),
            passed: i % 100 != 99,
            location: None,
            output: "o".repeat(20_000),
        }).collect();
        Ok(TestOutput { success: false, cases, totals: BTreeMap::from([("TOTAL".into(), 400)]), diagnostics: noisy(2) })
    }
}

fn connector(fail: bool) -> Noisy {
    let game = GameInfo {
        family: GameFamily::Gen3,
        engine: "original-synthetic-fixture".into(),
        capabilities: Capabilities { max_party_size: 6, max_moves: 4, min_level: 1, max_level: 100, held_items: true, abilities: true },
    };
    let snapshot = ProjectSnapshot {
        game, trainers: BTreeMap::new(), maps: BTreeMap::new(), scenes: BTreeMap::new(), species: BTreeMap::new(), flags: BTreeMap::new(),
    };
    Noisy { inner: MemoryConnector::new(snapshot), fail }
}

#[test]
fn successful_noisy_build_is_a_few_kilobytes_with_full_counts() {
    let result = domain::build(&mut connector(false)).unwrap();
    let size = serde_json::to_string(&result).unwrap().len();
    assert!(size < 8 * 1024, "build result is {size} bytes");
    assert_eq!((result.counts.errors, result.counts.warnings), (0, 414));
    assert_eq!(result.diagnostics.len(), MAX_WARNINGS);
    assert_eq!(result.counts.omitted, 414 - MAX_WARNINGS);
    // Distinct warnings only, and every message shortened.
    let mut messages: Vec<_> = result.diagnostics.iter().map(|d| d.message.clone()).collect();
    messages.dedup();
    assert_eq!(messages.len(), MAX_WARNINGS);
    assert!(result.diagnostics.iter().all(|d| d.message.chars().count() <= MAX_MESSAGE_CHARS + 32));
}

#[test]
fn failed_build_keeps_errors_first_and_counts_the_rest() {
    let error = domain::build(&mut connector(true)).unwrap_err();
    let size = serde_json::to_string(&error).unwrap().len();
    assert!(size < 12 * 1024, "error payload is {size} bytes");
    assert_eq!(error.kind, "build_failed");
    assert!(error.message.contains("120 errors, 414 warnings"), "{}", error.message);
    let errors = error.diagnostics.iter().take_while(|d| d.severity == Severity::Error).count();
    assert_eq!(errors, MAX_ERRORS);
    assert_eq!(error.diagnostics.len(), MAX_ERRORS + MAX_WARNINGS);
}

#[test]
fn small_builds_lose_nothing() {
    let (kept, counts) = domain::compact_diagnostics(vec![
        diagnostic(Severity::Warning, "warning: a".into()),
        diagnostic(Severity::Error, "error: b".into()),
    ]);
    assert_eq!(kept.iter().map(|d| d.message.as_str()).collect::<Vec<_>>(), ["error: b", "warning: a"]);
    assert_eq!((counts.errors, counts.warnings, counts.omitted), (1, 1, 0));
}

#[test]
fn test_runs_list_failures_first_with_short_output() {
    let result = domain::run_test(&mut connector(false), RunTestRequest { filter: None }).unwrap();
    let size = serde_json::to_string(&result).unwrap().len();
    assert!(size < 16 * 1024, "run_test result is {size} bytes");
    assert_eq!(result.cases.len(), MAX_CASES);
    assert_eq!(result.omitted_cases, 400 - MAX_CASES);
    assert!(result.cases[..4].iter().all(|c| !c.passed) && result.cases[4..].iter().all(|c| c.passed));
    assert!(result.cases[..4].iter().all(|c| c.output.starts_with("ooo") && c.output.len() < 1100));
    assert!(result.cases[4..].iter().all(|c| c.output.is_empty()));
    assert_eq!(result.totals["TOTAL"], 400);
    assert_eq!((result.counts.errors, result.counts.warnings), (2, 414));
}
