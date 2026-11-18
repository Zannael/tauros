//! Shared test helpers: locate the pinned expansion and make disposable copies.
//!
//! Nothing from the expansion is committed. Tests read the local pristine clone
//! (never modified) and work on `git archive` copies in a temporary directory; when
//! the clone is missing they skip with a message saying how to get it.

#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

pub const ENGINE_REF: &str = "expansion/1.16.4";
pub const ENGINE_COMMIT: &str = "3796c72237d2ebf0361c6f4a1b4be551d8858904";

pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

/// The pristine pinned clone, or `None` (after printing why) when it is missing.
pub fn engine() -> Option<PathBuf> {
    let manifest = fs::read_to_string(repo_root().join("toolchain/sources.json")).unwrap();
    assert!(manifest.contains(ENGINE_COMMIT) && manifest.contains(ENGINE_REF), "test pin differs from toolchain/sources.json");
    let data = std::env::var("TAUROS_DATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(std::env::var("HOME").unwrap()).join(".local/share/tauros"));
    let dir = data.join("engines").join(format!("pokeemerald-expansion-{}", ENGINE_REF.replace('/', "-")));
    let head = Command::new("git").arg("-C").arg(&dir).args(["rev-parse", "HEAD"]).output();
    match head {
        Ok(out) if out.status.success() && String::from_utf8_lossy(&out.stdout).trim() == ENGINE_COMMIT => Some(dir),
        _ => {
            eprintln!(
                "SKIPPED: pinned pokeemerald-expansion {ENGINE_REF} not found at {}; run `python3 scripts/setup_toolchain.py`",
                dir.display()
            );
            None
        }
    }
}

pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new(tag: &str) -> Self {
        static N: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "tauros-{tag}-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        if std::env::var_os("TAUROS_KEEP_FIXTURES").is_none() {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}

fn git(dir: &Path, args: &[&str]) {
    let ok = Command::new("git").arg("-C").arg(dir).args(args).stdout(Stdio::null()).status().unwrap().success();
    assert!(ok, "git {args:?} failed in {}", dir.display());
}

/// A `git archive` copy of the pinned engine (only `paths`, or all of it when
/// empty), with `.histignore` so the expansion builds without history, initialised
/// as a git repository with a local test identity and one baseline commit.
pub fn fixture(engine: &Path, tag: &str, paths: &[&str]) -> TempDir {
    let dir = TempDir::new(tag);
    let mut archive = Command::new("git")
        .arg("-C")
        .arg(engine)
        .args(["archive", "--format=tar", ENGINE_COMMIT])
        .args(paths)
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let ok = Command::new("tar")
        .arg("-x")
        .arg("-C")
        .arg(dir.path())
        .stdin(archive.stdout.take().unwrap())
        .status()
        .unwrap()
        .success();
    assert!(ok && archive.wait().unwrap().success(), "git archive failed");
    fs::write(dir.path().join(".histignore"), "").unwrap();
    init_repository(dir.path());
    dir
}

/// A copy of the FND-002 base build (sources plus build tree, timestamps kept), so
/// `make` rebuilds only what a test edits. `None` (after printing why) when the base
/// build is missing or not for the pinned commit.
pub fn built_fixture(engine: &Path, tag: &str) -> Option<TempDir> {
    let base = engine.with_file_name(format!("{}-base", engine.file_name().unwrap().to_string_lossy()));
    let record = fs::read_to_string(base.join("tauros-base-build.json")).unwrap_or_default();
    if !record.contains(ENGINE_COMMIT) {
        eprintln!("SKIPPED: base build missing at {}; run `python3 scripts/setup_toolchain.py`", base.display());
        return None;
    }
    let dir = TempDir::new(tag);
    let ok = Command::new("cp").arg("-a").arg(base.join(".")).arg(dir.path()).status().unwrap().success();
    assert!(ok, "copying the base build failed");
    fs::remove_file(dir.path().join("tauros-base-build.json")).unwrap();
    // Build outputs are ignored by the expansion's .gitignore, so only sources are committed.
    init_repository(dir.path());
    Some(dir)
}

fn init_repository(dir: &Path) {
    git(dir, &["init", "-q"]);
    for (k, v) in [
        ("user.name", "Tauros Test"),
        ("user.email", "test@tauros.invalid"),
        ("commit.gpgsign", "false"),
        ("core.hooksPath", ".git/hooks"),
        ("core.autocrlf", "false"),
    ] {
        git(dir, &["config", k, v]);
    }
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", "fixture: pinned expansion"]);
}

/// Files under `root/dir` (recursive) whose name satisfies `keep`, sorted.
pub fn files(root: &Path, dir: &str, keep: impl Fn(&str) -> bool + Copy) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.join(dir)];
    while let Some(d) = stack.pop() {
        for entry in fs::read_dir(&d).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else if keep(&path.file_name().unwrap().to_string_lossy()) {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}
