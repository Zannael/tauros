//! G3-001: snapshot before write, restore on failure, provenance trailers, no push.

use std::cell::Cell;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use tauros_connector_gen3::snapshot::{GitGuard, GuardError, DISABLED_PUSH_URL, NETWORK_SUBCOMMANDS};
use tauros_model::Provenance;

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static N: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "tauros-g3-001-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn git(dir: &Path, args: &[&str]) -> (bool, String) {
    let out = Command::new("git").arg("-C").arg(dir).args(args).output().unwrap();
    (out.status.success(), String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

/// A project directory with local identity, no signing and no inherited hooks.
fn project() -> TempDir {
    let dir = TempDir::new();
    git(&dir.0, &["init", "-q"]);
    for (k, v) in [
        ("user.name", "Tauros Test"),
        ("user.email", "test@tauros.invalid"),
        ("commit.gpgsign", "false"),
        ("core.hooksPath", ".git/hooks"),
    ] {
        git(&dir.0, &["config", k, v]);
    }
    dir
}

fn read(dir: &Path, file: &str) -> String {
    fs::read_to_string(dir.join(file)).unwrap()
}

#[test]
fn dirty_work_is_snapshotted_before_the_write_and_change_carries_provenance() {
    let dir = project();
    fs::write(dir.0.join("trainers.party"), "old").unwrap();
    let guard = GitGuard::take_over(&dir.0).unwrap();
    let receipt = guard
        .write("raise Roxanne's levels", Provenance::AiGenerated, |root| fs::write(root.join("trainers.party"), "new"))
        .unwrap();

    assert_eq!(git(&dir.0, &["show", &format!("{}:trainers.party", receipt.snapshot)]).1, "old");
    let commit = receipt.commit.expect("change commit");
    assert_eq!(git(&dir.0, &["rev-parse", &format!("{commit}^")]).1, receipt.snapshot);
    let message = git(&dir.0, &["log", "-1", "--format=%B", &commit]).1;
    assert!(message.starts_with("raise Roxanne's levels"));
    assert!(message.contains("Tauros-Provenance: ai-generated"));
    assert!(message.contains(&format!("Tauros-Snapshot: {}", receipt.snapshot)));
    assert_eq!(read(&dir.0, "trainers.party"), "new");
    assert_eq!(git(&dir.0, &["status", "--porcelain"]).1, "");
}

#[test]
fn clean_tree_uses_head_as_the_snapshot() {
    let dir = project();
    fs::write(dir.0.join("a.txt"), "1").unwrap();
    git(&dir.0, &["add", "-A"]);
    git(&dir.0, &["commit", "-q", "-m", "init"]);
    let head = git(&dir.0, &["rev-parse", "HEAD"]).1;
    let guard = GitGuard::take_over(&dir.0).unwrap();
    let receipt = guard.write("edit", Provenance::Human, |root| fs::write(root.join("a.txt"), "2")).unwrap();
    assert_eq!(receipt.snapshot, head);
    assert!(git(&dir.0, &["log", "-1", "--format=%B"]).1.contains("Tauros-Provenance: human"));
}

#[test]
fn failed_snapshot_aborts_before_any_write() {
    let dir = project();
    fs::write(dir.0.join("a.txt"), "old").unwrap();
    let guard = GitGuard::take_over(&dir.0).unwrap();
    let hook = dir.0.join(".git/hooks/pre-commit");
    fs::write(&hook, "#!/bin/sh\nexit 1\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let ran = Cell::new(false);
    let result = guard.write("edit", Provenance::AiAssisted, |root| {
        ran.set(true);
        fs::write(root.join("a.txt"), "new")
    });
    assert!(matches!(result, Err(GuardError::Snapshot(_))), "{result:?}");
    assert!(!ran.get(), "write ran without a snapshot");
    assert_eq!(read(&dir.0, "a.txt"), "old");
    assert!(!git(&dir.0, &["rev-parse", "--verify", "-q", "HEAD"]).0, "no commit may exist");
}

#[test]
fn failed_write_restores_the_snapshot() {
    let dir = project();
    fs::write(dir.0.join("a.txt"), "old").unwrap();
    fs::write(dir.0.join(".gitignore"), "build/\n").unwrap();
    fs::create_dir_all(dir.0.join("build")).unwrap();
    fs::write(dir.0.join("build/keep.o"), "artifact").unwrap();
    let guard = GitGuard::take_over(&dir.0).unwrap();
    let result = guard.write("edit", Provenance::AiGenerated, |root| -> Result<(), String> {
        fs::write(root.join("a.txt"), "half-written").unwrap();
        fs::write(root.join("new.txt"), "partial").unwrap();
        Err("parser exploded".into())
    });
    let Err(GuardError::Write { restored_to, .. }) = result else { panic!("{result:?}") };
    assert_eq!(read(&dir.0, "a.txt"), "old");
    assert!(!dir.0.join("new.txt").exists());
    assert_eq!(read(&dir.0, "build/keep.o"), "artifact", "ignored files must survive a restore");
    assert_eq!(git(&dir.0, &["rev-parse", "HEAD"]).1, restored_to);
}

#[test]
fn a_write_that_changes_nothing_records_no_commit() {
    let dir = project();
    fs::write(dir.0.join("a.txt"), "same").unwrap();
    let guard = GitGuard::take_over(&dir.0).unwrap();
    let receipt = guard.write("noop", Provenance::Human, |_| Ok::<(), String>(())).unwrap();
    assert_eq!(receipt.commit, None);
    assert_eq!(git(&dir.0, &["rev-parse", "HEAD"]).1, receipt.snapshot);
}

#[test]
fn taken_over_projects_cannot_push_and_the_guard_never_touches_remotes() {
    let dir = project();
    let upstream = TempDir::new();
    git(&upstream.0, &["init", "-q", "--bare"]);
    let upstream_url = upstream.0.to_string_lossy().into_owned();
    git(&dir.0, &["remote", "add", "origin", &upstream_url]);
    git(&dir.0, &["remote", "add", "broken", "/nonexistent/tauros/remote.git"]);
    fs::write(dir.0.join("a.txt"), "x").unwrap();

    let guard = GitGuard::take_over(&dir.0).unwrap();
    guard.write("edit", Provenance::AiAssisted, |root| fs::write(root.join("a.txt"), "y")).unwrap();

    for remote in ["origin", "broken"] {
        assert_eq!(git(&dir.0, &["remote", "get-url", "--push", remote]).1, DISABLED_PUSH_URL);
    }
    assert!(!git(&dir.0, &["push", "-q", "origin", "HEAD:refs/heads/main"]).0, "push via remote succeeded");
    assert!(!git(&dir.0, &["push", "-q", &upstream_url, "HEAD:refs/heads/main"]).0, "push via URL bypassed the hook");
    assert_eq!(git(&upstream.0, &["for-each-ref"]).1, "", "upstream received refs");

    let network: Vec<_> = guard
        .commands()
        .into_iter()
        .filter(|args| NETWORK_SUBCOMMANDS.contains(&args[0].as_str()))
        .collect();
    assert!(network.is_empty(), "guard ran network commands: {network:?}");
}

#[test]
fn an_existing_pre_push_hook_is_preserved() {
    let dir = project();
    let hook = dir.0.join(".git/hooks/pre-push");
    fs::create_dir_all(hook.parent().unwrap()).unwrap();
    fs::write(&hook, "#!/bin/sh\n# user's own hook\nexit 1\n").unwrap();
    GitGuard::take_over(&dir.0).unwrap();
    assert!(read(&dir.0, ".git/hooks/pre-push").contains("user's own hook"));
}

#[test]
fn take_over_requires_a_directory() {
    let missing = std::env::temp_dir().join("tauros-g3-001-does-not-exist");
    assert!(matches!(GitGuard::take_over(&missing), Err(GuardError::NotADirectory(_))));
}

#[test]
fn snapshots_and_restores_are_byte_exact_whatever_the_line_ending_config() {
    let dir = project();
    // A user config that converts line endings must not leak into snapshots.
    git(&dir.0, &["config", "core.autocrlf", "input"]);
    let crlf = b"JASC-PAL\r\n0100\r\n".to_vec();
    fs::write(dir.0.join("00.pal"), &crlf).unwrap();
    let guard = GitGuard::take_over(&dir.0).unwrap();
    let result = guard.write("fail", Provenance::Human, |root| {
        fs::write(root.join("00.pal"), b"changed")?;
        Err(std::io::Error::other("boom"))
    });
    assert!(matches!(result, Err(GuardError::Write { .. })), "{result:?}");
    assert_eq!(fs::read(dir.0.join("00.pal")).unwrap(), crlf, "restore rewrote line endings");
    let snapshot = git(&dir.0, &["rev-parse", "HEAD"]).1;
    let stored = Command::new("git").arg("-C").arg(&dir.0).args(["show", &format!("{snapshot}:00.pal")]).output().unwrap().stdout;
    assert_eq!(stored, crlf, "snapshot normalised line endings");
}
