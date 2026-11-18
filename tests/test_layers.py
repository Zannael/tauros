"""The crate graph follows the specification's layers: no dependency points upward."""
import json
from pathlib import Path
import shutil
import subprocess
import unittest

ROOT = Path(__file__).resolve().parents[1]

# Allowed internal dependencies per crate (plan "Layered architecture"; FND-001).
# Layer 1 app wires everything; layer 2 mcp sees the model and the emulator interface;
# connectors (layer 4) see only the model; model, emulator and agent bridge stand alone.
ALLOWED = {
    "tauros-app": {"tauros-model", "tauros-connector-gen3", "tauros-emu", "tauros-mcp", "tauros-agent-bridge"},
    "tauros-mcp": {"tauros-model", "tauros-emu"},
    "tauros-agent-bridge": set(),
    "tauros-connector-gen3": {"tauros-model"},
    "tauros-emu": {"tauros-mgba-sys"},
    "tauros-mgba-sys": set(),
    "tauros-model": set(),
}


@unittest.skipIf(shutil.which("cargo") is None, "cargo not installed")
class Layers(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        out = subprocess.run(["cargo", "metadata", "--format-version", "1", "--no-deps", "--offline"],
                             cwd=ROOT, check=True, capture_output=True, text=True).stdout
        cls.packages = {p["name"]: p for p in json.loads(out)["packages"]}

    def test_every_workspace_crate_is_classified(self):
        self.assertEqual(set(self.packages) - set(ALLOWED), set())

    def test_no_upward_or_sideways_dependencies(self):
        for name, package in self.packages.items():
            internal = {d["name"] for d in package["dependencies"] if d["name"].startswith("tauros-")}
            with self.subTest(crate=name):
                self.assertLessEqual(internal, ALLOWED[name])

    def test_nothing_depends_on_the_app(self):
        for name, package in self.packages.items():
            with self.subTest(crate=name):
                self.assertNotIn("tauros-app", {d["name"] for d in package["dependencies"]})

    def test_crates_are_not_publishable(self):
        for name, package in self.packages.items():
            with self.subTest(crate=name):
                self.assertEqual(package["publish"], [])


if __name__ == "__main__":
    unittest.main()
