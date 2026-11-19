//! APP-004 tests: change history listing with origin/provenance and exact,
//! non-destructive undo over G3-001 snapshot commits.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use tauros_app::history::{list_history, undo_to, ChangeOrigin};
use tauros_connector_gen3::snapshot::GitGuard;
use tauros_model::Provenance;

struct TempRepo(PathBuf);

impl TempRepo {
    fn new(tag: &str) -> Self {
        static N: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "tauros-history-{tag}-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        git(&dir, &["init", "-q"]);
        git(&dir, &["config", "user.name", "Tauros Test"]);
        git(&dir, &["config", "user.email", "test@users.noreply.github.com"]);
        git(&dir, &["config", "core.autocrlf", "false"]);
        Self(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempRepo {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn lists_agent_and_manual_changes_and_restores_previous_snapshot_exactly() {
    let repo = TempRepo::new("restore");
    fs::write(repo.path().join(".gitignore"), "build/\n").unwrap();
    fs::create_dir_all(repo.path().join("build")).unwrap();
    fs::write(repo.path().join("build/cache.o"), b"ignored-build-output").unwrap();
    // Include CRLF bytes to verify core.autocrlf=false byte-exact restore.
    let initial_palette = b"JASC-PAL\r\n0100\r\n16\r\n";
    fs::write(repo.path().join("palette.pal"), initial_palette).unwrap();
    fs::write(repo.path().join("trainers.party"), "=== TRAINER_A ===\nLevel: 10\n").unwrap();
    git(repo.path(), &["add", "-A"]);
    git(repo.path(), &["commit", "-q", "-m", "initial import"]);

    let guard = GitGuard::take_over(repo.path()).unwrap();

    // 1. Manual change (Human)
    let r1 = guard
        .write("edit trainer level manually", Provenance::Human, |root| {
            fs::write(root.join("trainers.party"), "=== TRAINER_A ===\nLevel: 12\n")
        })
        .unwrap();
    let human_commit = r1.commit.unwrap();

    // 2. Agent change (AiGenerated): modifies trainers.party, edits palette, adds new scene file
    let r2 = guard
        .write("add director scene", Provenance::AiGenerated, |root| {
            fs::write(root.join("trainers.party"), "=== TRAINER_A ===\nLevel: 50\n")?;
            fs::write(root.join("palette.pal"), b"CHANGED\r\n")?;
            fs::write(root.join("director.pory"), "script Director { end }\n")
        })
        .unwrap();
    let agent_commit = r2.commit.unwrap();

    // 3. Uncommitted dirty edit right before undo (must be snapshotted, never lost)
    fs::write(repo.path().join("scratch.txt"), "pending manual work").unwrap();

    // Verify history listing reports origins accurately
    let entries = list_history(repo.path(), 20).unwrap();
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[0].commit, agent_commit);
    assert_eq!(entries[0].origin, ChangeOrigin::AgentGenerated);
    assert_eq!(entries[0].provenance, Some(Provenance::AiGenerated));
    assert_eq!(entries[0].snapshot_commit.as_deref(), Some(human_commit.as_str()));

    assert_eq!(entries[1].commit, human_commit);
    assert_eq!(entries[1].origin, ChangeOrigin::Human);
    assert_eq!(entries[1].provenance, Some(Provenance::Human));

    assert_eq!(entries[2].origin, ChangeOrigin::External);

    // Undo back to the Human snapshot
    let receipt = undo_to(repo.path(), &human_commit).unwrap();
    assert_eq!(receipt.target_commit, human_commit);
    assert!(receipt.undo_commit.is_some());

    // Working tree must match human_commit byte-for-byte:
    // - trainers.party restored to Level: 12
    // - palette.pal restored with exact CRLF bytes
    // - director.pory and scratch.txt removed from working tree
    // - ignored build/cache.o preserved untouched
    assert_eq!(
        fs::read_to_string(repo.path().join("trainers.party")).unwrap(),
        "=== TRAINER_A ===\nLevel: 12\n"
    );
    assert_eq!(fs::read(repo.path().join("palette.pal")).unwrap(), initial_palette);
    assert!(!repo.path().join("director.pory").exists());
    assert!(!repo.path().join("scratch.txt").exists());
    assert_eq!(
        fs::read(repo.path().join("build/cache.o")).unwrap(),
        b"ignored-build-output"
    );

    // Non-destructive: history now has the pre-undo snapshot (preserving scratch.txt) + Undo commit
    let after = list_history(repo.path(), 20).unwrap();
    assert_eq!(after[0].origin, ChangeOrigin::Undo);
    assert_eq!(after[0].undo_target.as_deref(), Some(human_commit.as_str()));
    assert_eq!(after[1].origin, ChangeOrigin::Snapshot);
    assert_eq!(after[1].commit, receipt.pre_undo_snapshot);

    // Undo is reversible: restoring pre_undo_snapshot brings back both director.pory and scratch.txt!
    let redo = undo_to(repo.path(), &receipt.pre_undo_snapshot).unwrap();
    assert!(redo.undo_commit.is_some());
    assert!(repo.path().join("director.pory").exists());
    assert_eq!(
        fs::read_to_string(repo.path().join("scratch.txt")).unwrap(),
        "pending manual work"
    );

    // No-op undo to current tree creates no extra commit
    let noop = undo_to(repo.path(), &receipt.pre_undo_snapshot).unwrap();
    assert!(noop.undo_commit.is_none());
}

#[test]
fn undo_rejects_unknown_commit_without_modifying_tree() {
    let repo = TempRepo::new("invalid");
    fs::write(repo.path().join("file.txt"), "hello").unwrap();
    git(repo.path(), &["add", "-A"]);
    git(repo.path(), &["commit", "-q", "-m", "init"]);

    let err = undo_to(repo.path(), "0000000000000000000000000000000000000000").unwrap_err();
    assert!(err.contains("unknown target commit"), "{err}");
    assert_eq!(fs::read_to_string(repo.path().join("file.txt")).unwrap(), "hello");
    assert_eq!(list_history(repo.path(), 10).unwrap().len(), 1);
}
