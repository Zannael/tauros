"""GFX-004: catalog upgrade entries apply, revert, validate and preview on a synthetic project.

Everything is generated here; no external or Nintendo-derived asset is used.
"""
import hashlib
import json
import os
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "scripts"))
sys.path.insert(0, str(Path(__file__).resolve().parent))
import gfx_upgrades as gu  # noqa: E402
from test_asset_catalog import indexed_png  # noqa: E402

PAL_A = "JASC-PAL\n0100\n16\n" + "\n".join(f"{8 * i} 0 0" for i in range(16)) + "\n"
PAL_B = "JASC-PAL\n0100\n16\n" + "\n".join(f"0 {8 * i} 0" for i in range(16)) + "\n"


def sha(text):
    return hashlib.sha256(text.encode()).hexdigest()


def write_tileset(directory, pal, words=None, attributes=2):
    directory.mkdir(parents=True)
    indexed_png(directory / "tiles.png", 16, 8, 8, [i % 16 for i in range(128)], [(i, i, i) for i in range(16)])
    words = words or [1, 0, 0, 0, 0, 0, 0, 0]
    (directory / "metatiles.bin").write_bytes(b"".join(w.to_bytes(2, "little") for w in words))
    (directory / "metatile_attributes.bin").write_bytes(bytes(attributes))
    (directory / "palettes").mkdir()
    for i in range(16):
        (directory / "palettes" / f"{i:02d}.pal").write_text(pal)


class UpgradeTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        data = Path(self.tmp.name).resolve()
        self.old_env = os.environ.get("TAUROS_DATA")
        os.environ["TAUROS_DATA"] = str(data)
        self.addCleanup(self.restore_env)
        self.project = data / "projects" / "demo"
        (self.project / "include").mkdir(parents=True)
        (self.project / "include/fieldmap.h").write_text("", encoding="utf-8")
        write_tileset(self.project / "data/tilesets/primary/general", PAL_A)
        write_tileset(self.project / "data/tilesets/secondary/town", PAL_A)
        (self.project / "data/layouts/Town").mkdir(parents=True)
        (self.project / "data/layouts/Town/map.bin").write_bytes(bytes([0, 0, 1, 0]))
        (self.project / "data/layouts/layouts.json").write_text(
            json.dumps({"layouts": [{"id": "LAYOUT_TOWN", "width": 2, "height": 1,
                                     "primary_tileset": "gTileset_General",
                                     "blockdata_filepath": "data/layouts/Town/map.bin"}]}, indent=2) + "\n",
            encoding="utf-8")
        (self.project / "src").mkdir()
        (self.project / "src/weather.c").write_text("int count = 16;\nint speed = 64;\n", encoding="utf-8")
        src = data / "assets/catalog/sources/pack"
        src.mkdir(parents=True)
        (src / "01.pal").write_text(PAL_B, encoding="utf-8")
        (data / "assets/catalog/upgrades").mkdir(parents=True)
        self.entry = {
            "id": "demo-snow", "kind": "tileset", "title": "Demo", "status": "unverified",
            "status_basis": "synthetic",
            "sources": [{"id": "pal01", "path": "pack/01.pal", "sha256": sha(PAL_B), "pin": "rev1",
                         "credits": "Author", "terms": "credit"}],
            "pairing": {"role": "primary"}, "prerequisites": {"layers": 2},
            "operations": [
                {"op": "copy_tree", "from": "data/tilesets/primary/general", "to": "data/tilesets/primary/snow"},
                {"op": "copy_source", "source": "pal01", "to": "data/tilesets/primary/snow/palettes/01.pal",
                 "replace": True},
                {"op": "replace_text", "file": "data/layouts/layouts.json",
                 "old": '"primary_tileset": "gTileset_General"', "new": '"primary_tileset": "gTileset_Snow"'}],
            "protect": ["data/tilesets/primary/general", "data/tilesets/secondary/town", "data/layouts/Town"],
            "validate": [{"path": "data/tilesets/primary/snow", "role": "primary"}],
            "preview": {"layout": "LAYOUT_TOWN", "before": ["data/tilesets/primary/general", "data/tilesets/secondary/town"],
                        "after": ["data/tilesets/primary/snow", "data/tilesets/secondary/town"]},
        }

    def restore_env(self):
        if self.old_env is None:
            os.environ.pop("TAUROS_DATA", None)
        else:
            os.environ["TAUROS_DATA"] = self.old_env

    def snapshot(self):
        return {str(p.relative_to(self.project)): p.read_bytes()
                for p in sorted(self.project.rglob("*")) if p.is_file()}

    def test_apply_is_idempotent_and_revert_restores_every_byte(self):
        before = self.snapshot()
        first = gu.apply_entry(self.project, self.entry)
        self.assertEqual(first["result"], "applied")
        self.assertIn("gTileset_Snow", (self.project / "data/layouts/layouts.json").read_text())
        self.assertEqual((self.project / "data/tilesets/primary/snow/palettes/01.pal").read_text(), PAL_B)
        self.assertEqual((self.project / "data/tilesets/primary/snow/palettes/00.pal").read_text(), PAL_A)
        self.assertEqual(gu.apply_entry(self.project, self.entry)["result"], "already-applied")
        self.assertEqual(gu.revert_entry(self.project, "demo-snow")["result"], "reverted")
        self.assertEqual(self.snapshot(), before)
        self.assertEqual(gu.revert_entry(self.project, "demo-snow")["result"], "not-applied")

    def test_shared_tileset_collisions_and_events_stay_untouched(self):
        shared = {k: v for k, v in self.snapshot().items()
                  if k.startswith(("data/tilesets/primary/general", "data/layouts/Town"))}
        gu.apply_entry(self.project, self.entry)
        after = self.snapshot()
        for name, content in shared.items():
            self.assertEqual(after[name], content, name)

    def test_protect_violation_rolls_back(self):
        self.entry["operations"].append(
            {"op": "replace_text", "file": "src/weather.c", "old": "speed = 64", "new": "speed = 80"})
        self.entry["protect"].append("src/weather.c")
        before = self.snapshot()
        with self.assertRaisesRegex(gu.UpgradeError, "rolled back.*weather.c changed"):
            gu.apply_entry(self.project, self.entry)
        self.assertEqual(self.snapshot(), before)

    def test_validator_error_rolls_back(self):
        self.entry["operations"][0]["from"] = "data/tilesets/primary/bad"
        write_tileset(self.project / "data/tilesets/primary/bad", PAL_A, words=[1] * 12, attributes=0)
        # 24-byte metatiles: the triple-layer package the pinned engine cannot read
        (self.project / "data/tilesets/primary/bad/metatiles.bin").write_bytes(bytes(24))
        before = self.snapshot()
        with self.assertRaisesRegex(gu.UpgradeError, "rolled back"):
            gu.apply_entry(self.project, self.entry)
        self.assertEqual(self.snapshot(), before)

    def test_anchor_must_occur_exactly_once(self):
        self.entry["operations"] = [{"op": "replace_text", "file": "src/weather.c", "old": "= ", "new": "= 1 + "}]
        before = self.snapshot()
        with self.assertRaisesRegex(gu.UpgradeError, "exactly one occurrence"):
            gu.apply_entry(self.project, self.entry)
        self.assertEqual(self.snapshot(), before)

    def test_changed_source_is_refused(self):
        (Path(os.environ["TAUROS_DATA"]) / "assets/catalog/sources/pack/01.pal").write_text(PAL_A)
        with self.assertRaisesRegex(gu.UpgradeError, "sha256 differs"):
            gu.apply_entry(self.project, self.entry)

    def test_revert_refuses_when_a_file_changed_afterwards(self):
        gu.apply_entry(self.project, self.entry)
        layouts = self.project / "data/layouts/layouts.json"
        layouts.write_text(layouts.read_text() + " ")
        with self.assertRaisesRegex(gu.UpgradeError, "changed afterwards"):
            gu.revert_entry(self.project, "demo-snow")
        with self.assertRaisesRegex(gu.UpgradeError, "changed afterwards"):
            gu.apply_entry(self.project, self.entry)

    def test_conversion_required_and_unsafe_paths_are_refused(self):
        self.entry["status"] = "conversion-required"
        with self.assertRaisesRegex(gu.UpgradeError, "conversion-required"):
            gu.apply_entry(self.project, self.entry)
        self.entry["status"] = "unverified"
        self.entry["operations"] = [{"op": "replace_text", "file": "../outside.c", "old": "a", "new": "b"}]
        with self.assertRaisesRegex(gu.UpgradeError, "unsafe project path"):
            gu.apply_entry(self.project, self.entry)

    def test_project_outside_the_data_directory_is_refused(self):
        with tempfile.TemporaryDirectory() as other:
            (Path(other) / "include").mkdir()
            (Path(other) / "include/fieldmap.h").write_text("")
            with self.assertRaises(SystemExit):
                gu.apply_entry(Path(other), self.entry)

    def test_entry_model_requires_credits_pin_status_and_pairing(self):
        self.assertEqual(gu.check_entry(self.entry), [])
        broken = json.loads(json.dumps(self.entry))
        broken["sources"][0].pop("credits")
        broken["sources"][0].pop("pin")
        broken["status"] = "great"
        broken.pop("pairing")
        problems = " | ".join(gu.check_entry(broken))
        for text in ("missing credits", "without pin", "unknown status", "need pairing"):
            self.assertIn(text, problems)

    def test_two_entries_revert_only_in_reverse_order(self):
        second = {"id": "demo-weather", "kind": "weather", "title": "W", "status": "unverified",
                  "status_basis": "synthetic",
                  "sources": [{"id": "p", "sha256": sha("patch"), "credits": "Tauros", "terms": "repo"}],
                  "prerequisites": {}, "preview": {"manual": "emulator"},
                  "operations": [{"op": "replace_text", "file": "data/layouts/layouts.json",
                                  "old": '"width": 2', "new": '"width": 2, "x": 1'}]}
        gu.apply_entry(self.project, self.entry)
        gu.apply_entry(self.project, second)
        with self.assertRaisesRegex(gu.UpgradeError, "changed afterwards"):
            gu.revert_entry(self.project, "demo-snow")
        gu.revert_entry(self.project, "demo-weather")
        gu.revert_entry(self.project, "demo-snow")

    def test_preview_renders_before_and_after_with_the_new_palette(self):
        gu.apply_entry(self.project, self.entry)
        out = gu.preview_entry(self.project, self.entry)
        image = gu.ac.read_png(out)
        self.assertEqual((image.width, image.height), (2 * 16 + 8 + 2 * 16, 16))
        self.assertGreater(out.stat().st_size, 0)

    def test_weather_entry_has_no_rendered_preview(self):
        self.entry["preview"] = {"manual": "boot the ROM"}
        gu.apply_entry(self.project, self.entry)
        with self.assertRaisesRegex(gu.UpgradeError, "boot the ROM"):
            gu.preview_entry(self.project, self.entry)


if __name__ == "__main__":
    unittest.main()
