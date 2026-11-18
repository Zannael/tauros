"""Privacy guard: private data and game binaries are rejected before commit."""
import importlib.util
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("check_private", ROOT / "scripts/check_private.py")
privacy = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(privacy)


class Scan(unittest.TestCase):
    def test_clean_text_passes(self):
        self.assertEqual(privacy.scan("src/lib.rs", b"fn main() {}\n", []), [])

    def test_game_data_and_credential_files_rejected(self):
        for name in ("rom/emerald.gba", "a.sav", "state.ss1", ".env", "config/.env.local", "id_ed25519", "k.pem"):
            with self.subTest(name=name):
                self.assertTrue(privacy.scan(name, b"x", []))

    def test_secrets_and_home_paths_rejected(self):
        # Built at runtime so this file never contains a matching literal.
        samples = ["-----BEGIN OPENSSH " + "PRIVATE KEY-----", "sk-ant-" + "a1" * 10, "ghp_" + "A" * 36,
                   "github_pat_" + "B" * 40, "AKIA" + "ABCDEFGHIJKLMNOP", "/ho" + "me/someone/project"]
        for sample in samples:
            with self.subTest(sample=sample[:12]):
                self.assertTrue(privacy.scan("notes.md", sample.encode(), []))

    def test_local_patterns_are_case_insensitive_and_cover_names(self):
        self.assertTrue(privacy.scan("a.md", b"mail Me@Example.org", ["me@example.org"]))
        self.assertTrue(privacy.scan("docs/hostname-box.md", b"", ["hostname-box"]))

    def test_oversized_file_rejected(self):
        self.assertTrue(privacy.scan("big.dat", b"0" * (privacy.MAX_BYTES + 1), []))

    def test_noreply_email(self):
        self.assertTrue(privacy.NOREPLY.match("123+user@users.noreply.github.com"))
        self.assertFalse(privacy.NOREPLY.match("someone@example.org"))


class Repository(unittest.TestCase):
    def test_tracked_files_are_clean(self):
        if subprocess.run(["git", "rev-parse"], cwd=ROOT, capture_output=True).returncode:
            self.skipTest("not a git repository")
        names = subprocess.run(["git", "ls-files", "-z"], cwd=ROOT, check=True,
                               capture_output=True).stdout.decode().split("\0")
        for name in filter(None, names):
            with self.subTest(name=name):
                self.assertEqual(privacy.scan(name, (ROOT / name).read_bytes(), []), [])

    def test_untracked_pattern_file_is_read_from_git_dir(self):
        with tempfile.TemporaryDirectory() as temp:
            subprocess.run(["git", "init", "-q", temp], check=True)
            info = Path(temp) / ".git/info"
            info.mkdir(exist_ok=True)
            (info / "private-patterns").write_text("# comment\nsecret-name\n\n", encoding="utf-8")
            self.assertEqual(privacy.local_patterns(Path(temp)), ["secret-name"])


if __name__ == "__main__":
    unittest.main()
