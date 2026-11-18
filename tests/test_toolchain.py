"""Toolchain manifest, Dockerfile pin agreement and hardened container options."""
import copy
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("setup_toolchain", ROOT / "scripts/setup_toolchain.py")
toolchain = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(toolchain)


class Manifest(unittest.TestCase):
    def setUp(self):
        self.manifest = toolchain.load_manifest()

    def check_invalid(self, mutate):
        bad = copy.deepcopy(self.manifest)
        mutate(bad)
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "sources.json"
            path.write_text(json.dumps(bad), encoding="utf-8")
            with self.assertRaises(toolchain.SetupError):
                toolchain.load_manifest(path)

    def test_repository_manifest_is_valid_and_pins_everything(self):
        self.assertTrue(self.manifest["downloads"])
        for engine in self.manifest["engines"]:
            self.assertRegex(engine["commit"], r"^[0-9a-f]{40}$")

    def test_invalid_manifests_rejected(self):
        self.check_invalid(lambda m: m["downloads"][0].update(sha256="abc"))
        self.check_invalid(lambda m: m["downloads"][0].update(url="http://example.org/x.zip"))
        self.check_invalid(lambda m: m["engines"][0].update(commit="main"))
        self.check_invalid(lambda m: m["image"].update(base_digest="latest"))
        self.check_invalid(lambda m: m.update(engines=[]))

    def test_dockerfile_matches_manifest_pins(self):
        dockerfile = (ROOT / self.manifest["image"]["dockerfile"]).read_text(encoding="utf-8")
        image = self.manifest["image"]
        self.assertIn(f"FROM {image['base']}@{image['base_digest']}", dockerfile)
        for item in self.manifest["downloads"]:
            with self.subTest(download=item["name"]):
                self.assertIn(f"--checksum=sha256:{item['sha256']}", dockerfile)
                self.assertIn(item["url"], dockerfile)
        self.assertEqual(dockerfile.count("ADD "), len(self.manifest["downloads"]))

    def test_image_tag_follows_dockerfile_content(self):
        tag = toolchain.image_tag(self.manifest)
        self.assertRegex(tag, r"^tauros-gba-toolchain:[0-9a-f]{12}$")
        self.assertEqual(tag, toolchain.image_tag(self.manifest))


class Hardening(unittest.TestCase):
    def test_container_runs_isolated_and_unprivileged(self):
        args = toolchain.hardened_run("img:tag", "make", Path("/data/project"))
        joined = " ".join(args)
        for option in ("--rm", "--network none", "--cap-drop ALL", "--security-opt no-new-privileges",
                       "--read-only", "--tmpfs /tmp"):
            with self.subTest(option=option):
                self.assertIn(option, joined)
        self.assertRegex(joined, r"--user \d+:\d+")
        self.assertNotIn("--privileged", joined)
        volumes = [args[i + 1] for i, arg in enumerate(args) if arg == "--volume"]
        self.assertEqual(volumes, ["/data/project:/work:rw"])

    def test_no_volume_without_workdir(self):
        self.assertNotIn("--volume", toolchain.hardened_run("img:tag", "true"))


class Layout(unittest.TestCase):
    def test_engine_dirs_live_under_data_engines(self):
        engine = toolchain.load_manifest()["engines"][0]
        clone, base = toolchain.engine_dirs(Path("/d"), engine)
        self.assertEqual(clone.parent, Path("/d/engines"))
        self.assertEqual(base.name, clone.name + "-base")
        self.assertNotIn("/", clone.name)

    def test_data_dir_respects_override_and_default(self):
        self.assertEqual(toolchain.data_dir("/x/y"), Path("/x/y"))
        self.assertTrue(str(toolchain.data_dir()).endswith((".local/share/tauros",)) or "TAUROS_DATA" in __import__("os").environ)

    def test_refuses_to_replace_foreign_directory(self):
        engine = toolchain.load_manifest()["engines"][0]
        with tempfile.TemporaryDirectory() as temp:
            base = Path(temp) / "base"
            base.mkdir()
            (base / "user-file.txt").write_text("keep me", encoding="utf-8")
            with self.assertRaisesRegex(toolchain.SetupError, "did not create"):
                toolchain.build_base(engine, Path(temp), base, "img:tag", install=True)
            self.assertTrue((base / "user-file.txt").exists())


if __name__ == "__main__":
    unittest.main()
