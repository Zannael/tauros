//! Snapshot-before-write guard for game projects (G3-001).
//!
//! Every connector write runs inside [`GitGuard::write`]:
//! 1. **snapshot**: pending work in the project is committed (or HEAD already is the
//!    snapshot). If that fails, the write is never attempted;
//! 2. **write**: the connector modifies files. If it fails, the tree is restored to
//!    the snapshot;
//! 3. **record**: the change is committed with `Tauros-Provenance` and
//!    `Tauros-Snapshot` trailers (DEC-003).
//!
//! The guard never contacts a remote. Taking a project over disables every remote's
//! push URL and installs a `pre-push` hook that refuses, so nothing reaches upstream
//! (pret does not accept AI-generated code).

use std::cell::RefCell;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;
use tauros_model::Provenance;

/// Placeholder push URL; any push attempt fails on it.
pub const DISABLED_PUSH_URL: &str = "DISABLED-by-tauros";

const PRE_PUSH_HOOK: &str = "#!/bin/sh\n\
# Installed by Tauros: game projects are never pushed (see Tauros README).\n\
echo 'Tauros: pushing this game project is disabled.' >&2\n\
exit 1\n";

/// Git subcommands that contact a remote; the guard must never run them.
pub const NETWORK_SUBCOMMANDS: &[&str] = &["push", "fetch", "pull", "clone", "ls-remote", "submodule"];

#[derive(Debug, PartialEq, Eq)]
pub enum GuardError {
    NotADirectory(PathBuf),
    /// Snapshot could not be taken; nothing was written.
    Snapshot(String),
    /// The write failed; the project was restored to the snapshot.
    Write { message: String, restored_to: String },
    Git(String),
}

impl fmt::Display for GuardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotADirectory(p) => write!(f, "not a directory: {}", p.display()),
            Self::Snapshot(m) => write!(f, "snapshot failed, nothing written: {m}"),
            Self::Write { message, restored_to } => write!(f, "write failed ({message}); restored to {restored_to}"),
            Self::Git(m) => write!(f, "git: {m}"),
        }
    }
}

impl std::error::Error for GuardError {}

#[derive(Debug, PartialEq, Eq)]
pub struct WriteReceipt {
    /// Commit holding the state before the write.
    pub snapshot: String,
    /// Commit recording the write; `None` when the write changed nothing.
    pub commit: Option<String>,
}

pub struct GitGuard {
    root: PathBuf,
    /// Every git argument list run, for auditing (tests check no network use).
    log: RefCell<Vec<Vec<String>>>,
}

impl GitGuard {
    /// Takes a game project over: initialises git if needed, disables push for every
    /// remote and installs the refusing `pre-push` hook unless one already exists.
    pub fn take_over(root: impl Into<PathBuf>) -> Result<Self, GuardError> {
        let root = root.into();
        if !root.is_dir() {
            return Err(GuardError::NotADirectory(root));
        }
        let guard = Self { root, log: RefCell::new(Vec::new()) };
        if guard.git(&["rev-parse", "--git-dir"]).is_err() {
            guard.git(&["init", "-q"])?;
        }
        let remotes = guard.git(&["remote"])?;
        for remote in remotes.lines().map(str::trim).filter(|r| !r.is_empty()) {
            guard.git(&["remote", "set-url", "--push", remote, DISABLED_PUSH_URL])?;
        }
        let hooks = PathBuf::from(guard.git(&["rev-parse", "--git-path", "hooks"])?.trim());
        let hooks = if hooks.is_absolute() { hooks } else { guard.root.join(hooks) };
        let hook = hooks.join("pre-push");
        if !hook.exists() {
            std::fs::create_dir_all(&hooks).map_err(|e| GuardError::Git(e.to_string()))?;
            std::fs::write(&hook, PRE_PUSH_HOOK).map_err(|e| GuardError::Git(e.to_string()))?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755))
                    .map_err(|e| GuardError::Git(e.to_string()))?;
            }
        }
        Ok(guard)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Commits pending work so the current state is recoverable; returns that commit.
    pub fn snapshot(&self, summary: &str) -> Result<String, GuardError> {
        let snap = |e: GuardError| GuardError::Snapshot(e.to_string());
        self.git(&["add", "-A"]).map_err(snap)?;
        let has_head = self.git(&["rev-parse", "--verify", "-q", "HEAD"]).is_ok();
        let dirty = !self.git(&["status", "--porcelain"]).map_err(snap)?.trim().is_empty();
        if dirty || !has_head {
            let message = format!("tauros: snapshot before: {summary}");
            let mut args = vec!["commit", "-q", "-m", message.as_str()];
            if !has_head {
                args.push("--allow-empty");
            }
            self.git(&args).map_err(snap)?;
        }
        Ok(self.git(&["rev-parse", "HEAD"]).map_err(snap)?.trim().to_owned())
    }

    /// Snapshot, run `write` on the project root, then commit the result with its
    /// provenance. A failed snapshot means `write` never runs; a failed write
    /// restores the snapshot.
    pub fn write<E: fmt::Display>(
        &self,
        summary: &str,
        provenance: Provenance,
        write: impl FnOnce(&Path) -> Result<(), E>,
    ) -> Result<WriteReceipt, GuardError> {
        let snapshot = self.snapshot(summary)?;
        if let Err(e) = write(&self.root) {
            self.restore(&snapshot)?;
            return Err(GuardError::Write { message: e.to_string(), restored_to: snapshot });
        }
        self.git(&["add", "-A"])?;
        if self.git(&["status", "--porcelain"])?.trim().is_empty() {
            return Ok(WriteReceipt { snapshot, commit: None });
        }
        let message = format!(
            "{summary}\n\nTauros-Provenance: {}\nTauros-Snapshot: {snapshot}",
            provenance.label()
        );
        if let Err(e) = self.git(&["commit", "-q", "-m", &message]) {
            self.restore(&snapshot)?;
            return Err(GuardError::Write { message: format!("recording commit failed: {e}"), restored_to: snapshot });
        }
        let commit = self.git(&["rev-parse", "HEAD"])?.trim().to_owned();
        Ok(WriteReceipt { snapshot, commit: Some(commit) })
    }

    /// Returns the tree to `commit`, dropping changes made since (ignored files stay).
    fn restore(&self, commit: &str) -> Result<(), GuardError> {
        self.git(&["reset", "-q", "--hard", commit])?;
        self.git(&["clean", "-q", "-f", "-d"])?;
        Ok(())
    }

    /// The git argument lists run so far.
    pub fn commands(&self) -> Vec<Vec<String>> {
        self.log.borrow().clone()
    }

    fn git(&self, args: &[&str]) -> Result<String, GuardError> {
        if let Some(sub) = args.first() {
            debug_assert!(!NETWORK_SUBCOMMANDS.contains(sub), "network git command {sub}");
        }
        self.log.borrow_mut().push(args.iter().map(|a| a.to_string()).collect());
        // Snapshots and restores must be byte-exact whatever the user's git config:
        // line-ending conversion would rewrite files (e.g. CRLF palettes) on restore.
        let output = Command::new("git")
            .arg("-C")
            .arg(&self.root)
            .args(["-c", "core.autocrlf=false", "-c", "core.eol=lf", "-c", "core.safecrlf=false"])
            .args(args)
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
            .map_err(|e| GuardError::Git(e.to_string()))?;
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).into_owned())
        } else {
            Err(GuardError::Git(format!(
                "git {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr).trim()
            )))
        }
    }
}
