//! Change history and non-destructive snapshot undo (`APP-004`).
//!
//! Reads the `G3-001` snapshot commits and their `Tauros-Provenance` /
//! `Tauros-Snapshot` trailers, lists agent and manual changes with their origin,
//! and restores any previous snapshot without rewriting git history.

use std::path::Path;
use std::process::Command;

use serde::{Deserialize, Serialize};
use tauros_connector_gen3::snapshot::GitGuard;
use tauros_model::Provenance;

/// High-level origin shown in the change-history view.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ChangeOrigin {
    /// Change recorded with `Tauros-Provenance: ai-generated`.
    AgentGenerated,
    /// Change recorded with `Tauros-Provenance: ai-assisted`.
    AgentAssisted,
    /// Manual change recorded with `Tauros-Provenance: human`.
    Human,
    /// Automatic pre-write safety snapshot created by `GitGuard`.
    Snapshot,
    /// Non-destructive restore recorded by [`undo_to`].
    Undo,
    /// Commit created outside Tauros (no `Tauros-*` trailers).
    External,
}

/// One commit in the project's change history.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub commit: String,
    pub short_commit: String,
    pub timestamp: String,
    pub summary: String,
    pub origin: ChangeOrigin,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provenance: Option<Provenance>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot_commit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub undo_target: Option<String>,
}

/// Result returned by [`undo_to`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UndoReceipt {
    /// Full hash of the restored target commit.
    pub target_commit: String,
    /// Snapshot commit taken immediately before restoring (preserves pending work).
    pub pre_undo_snapshot: String,
    /// New commit recording the restored tree (`None` when the tree already matched).
    pub undo_commit: Option<String>,
}

fn run_git(project_root: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .current_dir(project_root)
        .args(["-c", "core.autocrlf=false"])
        .args(args)
        .output()
        .map_err(|e| format!("failed to run git: {e}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_owned();
        let stdout = String::from_utf8_lossy(&out.stdout).trim().to_owned();
        let detail = if stderr.is_empty() { stdout } else { stderr };
        return Err(format!("git {} failed: {detail}", args.join(" ")));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn parse_provenance(value: &str) -> Option<Provenance> {
    match value.trim() {
        "human" => Some(Provenance::Human),
        "ai-assisted" => Some(Provenance::AiAssisted),
        "ai-generated" => Some(Provenance::AiGenerated),
        _ => None,
    }
}

/// Lists up to `limit` recent commits from `project_root` with their Tauros
/// provenance, snapshot link and origin classification.
pub fn list_history(project_root: &Path, limit: usize) -> Result<Vec<HistoryEntry>, String> {
    if !project_root.is_dir() {
        return Err(format!("not a directory: {}", project_root.display()));
    }
    if run_git(project_root, &["rev-parse", "--verify", "-q", "HEAD"]).is_err() {
        return Ok(Vec::new());
    }
    let limit_arg = format!("-n{}", limit.max(1));
    // Record separator 0x1e, field separator 0x1f.
    let raw = run_git(
        project_root,
        &["log", &limit_arg, "--format=%H%x1f%h%x1f%cI%x1f%s%x1f%b%x1e"],
    )?;

    let mut entries = Vec::new();
    for record in raw.split('\x1e') {
        let trimmed = record.trim_matches(|c| c == '\n' || c == '\r');
        if trimmed.is_empty() {
            continue;
        }
        let mut fields = trimmed.split('\x1f');
        let commit = fields.next().unwrap_or("").trim().to_owned();
        let short_commit = fields.next().unwrap_or("").trim().to_owned();
        let timestamp = fields.next().unwrap_or("").trim().to_owned();
        let summary = fields.next().unwrap_or("").trim().to_owned();
        let body = fields.next().unwrap_or("");

        let mut provenance = None;
        let mut snapshot_commit = None;
        let mut undo_target = None;
        for line in body.lines().map(str::trim) {
            if let Some(val) = line.strip_prefix("Tauros-Provenance:") {
                provenance = parse_provenance(val);
            } else if let Some(val) = line.strip_prefix("Tauros-Snapshot:") {
                snapshot_commit = Some(val.trim().to_owned());
            } else if let Some(val) = line.strip_prefix("Tauros-Undo-Target:") {
                undo_target = Some(val.trim().to_owned());
            }
        }

        let origin = if undo_target.is_some() {
            ChangeOrigin::Undo
        } else if let Some(p) = provenance {
            match p {
                Provenance::AiGenerated => ChangeOrigin::AgentGenerated,
                Provenance::AiAssisted => ChangeOrigin::AgentAssisted,
                Provenance::Human => ChangeOrigin::Human,
            }
        } else if summary.starts_with("tauros: snapshot before:") {
            ChangeOrigin::Snapshot
        } else {
            ChangeOrigin::External
        };

        entries.push(HistoryEntry {
            commit,
            short_commit,
            timestamp,
            summary,
            origin,
            provenance,
            snapshot_commit,
            undo_target,
        });
    }
    Ok(entries)
}

/// Restores `project_root` to the exact tracked tree of `target_commit` without
/// rewriting git history. Any pending edits are snapshotted first via `GitGuard`,
/// and the restore is recorded as a new commit referencing `Tauros-Undo-Target`.
pub fn undo_to(project_root: &Path, target_commit: &str) -> Result<UndoReceipt, String> {
    let target_spec = format!("{target_commit}^{{commit}}");
    let full_target = run_git(project_root, &["rev-parse", "--verify", "-q", &target_spec])
        .map_err(|_| format!("unknown target commit '{target_commit}'"))?
        .trim()
        .to_owned();

    let guard = GitGuard::take_over(project_root).map_err(|e| e.to_string())?;
    let short_target: String = full_target.chars().take(8).collect();
    let pre_undo_snapshot = guard
        .snapshot(&format!("undo to {short_target}"))
        .map_err(|e| e.to_string())?;

    // Check whether HEAD's tree already matches target_commit's tree.
    let head_tree = run_git(project_root, &["rev-parse", "HEAD^{tree}"])?
        .trim()
        .to_owned();
    let target_tree = run_git(project_root, &["rev-parse", &format!("{full_target}^{{tree}}")])?
        .trim()
        .to_owned();
    if head_tree == target_tree {
        return Ok(UndoReceipt {
            target_commit: full_target,
            pre_undo_snapshot,
            undo_commit: None,
        });
    }

    // Update index and working tree to match target_commit byte-for-byte while
    // keeping ignored build files untouched.
    run_git(project_root, &["read-tree", "--reset", "-u", &full_target])?;

    let message = format!(
        "tauros: undo to {short_target}\n\nTauros-Provenance: human\nTauros-Snapshot: {pre_undo_snapshot}\nTauros-Undo-Target: {full_target}\n"
    );
    run_git(project_root, &["commit", "-q", "-m", &message])?;
    let undo_commit = run_git(project_root, &["rev-parse", "HEAD"])?
        .trim()
        .to_owned();

    Ok(UndoReceipt {
        target_commit: full_target,
        pre_undo_snapshot,
        undo_commit: Some(undo_commit),
    })
}
