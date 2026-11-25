"""GFX-001: the validator catches each violation class on synthetic fixtures.

All art is generated here; nothing external or Nintendo-derived is committed.
Set TAUROS_GFX_PROJECT to a scratch project under $TAUROS_DATA to also run the
container pipeline (Porytiles, no network, then the game build).
"""
import importlib.util
import json
import os
from pathlib import Path
import shutil
import struct
import tempfile
import unittest
import zlib

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("gfx_pipeline", ROOT / "scripts/gfx_pipeline.py")
gfx = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(gfx)
png = gfx.png

BLANK = (0, 0, 0, 0)
# RGB555-exact ramps (multiples of 8); the tile generator uses only these.
RAMP = [(8 * i + 8, 8 * (7 - i) + 8, 16 * (i % 4) + 8, 255) for i in range(8)]
RAMP_A = [(248, 248, 248, 255), (200, 200, 200, 255)]


def blank(width, height):
    return [[BLANK] * width for _ in range(height)]


def paint_tile(img, tx, ty, n):
    """Tile number n: one marker pixel in the corner plus one placed by n, so no two tiles are
    equal under any flip."""
    pos = n % 63 + 1
    bx, by = pos % 8, pos // 8
    img[ty * 8][tx * 8] = RAMP_A[(n // 504) % 2]
    img[ty * 8 + by][tx * 8 + bx] = RAMP[(n // 63) % 8]


def paint_cell(img, mx, my, first):
    for i, (dx, dy) in enumerate(((0, 0), (1, 0), (0, 1), (1, 1))):
        paint_tile(img, mx * 2 + dx, my * 2 + dy, first + i)


def save(path, img):
    png.write_png(path, len(img[0]), len(img), [bytes(c for px in row for c in px) for row in img])


def save_indexed(path, width, height, palette_size):
    """8-bit indexed PNG whose PLTE has palette_size entries."""
    plte = b"".join(bytes((8 * i % 256, 0, 8)) for i in range(palette_size))
    raw = b"".join(b"\x00" + bytes(i % palette_size for i in range(width)) for _ in range(height))

    def chunk(t, b):
        return struct.pack(">I", len(b)) + t + b + struct.pack(">I", zlib.crc32(t + b))

    data = png.PNG_MAGIC + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 3, 0, 0, 0))
    data += chunk(b"PLTE", plte) + chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b"")
    Path(path).write_bytes(data)


def valid_layers(width=128, height=32):
    """Bottom has cells 0-7, middle 8-11 and top 4-5/12-15: two layers at most per cell."""
    layers = {name: blank(width, height) for name in gfx.LAYERS}
    cols = width // 16
    n = 0
    for cell in range(cols * (height // 16)):
        mx, my = cell % cols, cell // cols
        target = "bottom" if cell < 8 else "middle" if cell < 12 else "top"
        paint_cell(layers[target], mx, my, n)
        n += 4
    paint_cell(layers["top"], 4, 0, n)         # cell 4 gets a second layer
    return layers


def write_package(root, layers, **extra):
    root.mkdir(parents=True, exist_ok=True)
    for name, img in layers.items():
        save(root / f"{name}.png", img)
    return root


class Fixture(unittest.TestCase):
    def setUp(self):
        self._temp = tempfile.TemporaryDirectory()
        self.addCleanup(self._temp.cleanup)
        self.tmp = Path(self._temp.name)

    def check(self, layers, role="primary"):
        return gfx.validate_layers(write_package(self.tmp / "pkg", layers), role)


class SourceValidation(Fixture):
    def test_valid_tileset_has_no_violation(self):
        report = self.check(valid_layers())
        self.assertEqual(report.errors, 0, report.render())
        self.assertEqual(report.totals(), {})
        self.assertEqual(report.usage["metatiles"], 16)

    def test_dimensions_not_multiple_of_8(self):
        layers = {n: blank(20, 16) for n in gfx.LAYERS}
        report = self.check(layers)
        self.assertIn("dimensions.not_multiple_of_8", report.codes())
        self.assertTrue(any(v.x == 20 and v.y == 16 for v in report.items))

    def test_dimensions_not_multiple_of_16(self):
        report = self.check({n: blank(24, 16) for n in gfx.LAYERS})
        self.assertIn("dimensions.not_multiple_of_16", report.codes())

    def test_loose_converted_png_needs_multiple_of_16(self):
        path = self.tmp / "art.png"
        save(path, blank(24, 16))
        self.assertIn("dimensions.not_multiple_of_16", gfx.validate_png(path).codes())
        self.assertNotIn("dimensions.not_multiple_of_16", gfx.validate_png(path, converted=False).codes())

    def test_layer_size_mismatch(self):
        layers = valid_layers()
        layers["top"] = blank(128, 16)
        self.assertIn("layers.size_mismatch", self.check(layers).codes())

    def test_missing_layer(self):
        layers = valid_layers()
        del layers["top"]
        self.assertIn("package.missing_layer", self.check(layers).codes())

    def test_too_many_colours_in_a_tile_with_coordinates(self):
        layers = valid_layers()
        for i in range(16):                     # 16 distinct opaque colours in tile (3, 1) of bottom
            layers["bottom"][8 + i // 4][24 + i % 4] = (8 * i + 8, 16, 32, 255)
        report = self.check(layers)
        hit = [v for v in report.items if v.code == "palette.too_many_colours"]
        self.assertEqual([(v.file, v.tile) for v in hit], [("bottom.png", (3, 1))])

    def test_indexed_png_with_more_than_16_entries(self):
        path = self.tmp / "indexed.png"
        save_indexed(path, 16, 16, 17)
        self.assertIn("palette.indexed_entries", gfx.validate_png(path).codes())
        save_indexed(path, 16, 16, 16)
        self.assertNotIn("palette.indexed_entries", gfx.validate_png(path).codes())

    def test_non_rgb555_colour_reports_pixel(self):
        layers = valid_layers()
        layers["bottom"][0][0] = (1, 2, 3, 255)
        report = self.check(layers)
        hit = [v for v in report.items if v.code == "color.not_rgb555"]
        self.assertEqual([(v.x, v.y, v.tile) for v in hit], [(0, 0, (0, 0))])

    def test_semi_transparent_pixel(self):
        layers = valid_layers()
        layers["middle"][16][40] = (8, 8, 8, 128)
        report = self.check(layers)
        hit = [v for v in report.items if v.code == "alpha.semi_transparent"]
        self.assertEqual([(v.file, v.x, v.y) for v in hit], [("middle.png", 40, 16)])

    def test_duplicate_and_flipped_duplicate_tiles(self):
        layers = valid_layers()
        for y in range(8):
            layers["bottom"][y][16:24] = [BLANK] * 8
        paint_tile(layers["bottom"], 2, 0, 0)           # same as tile (0, 0) of cell 0 (n = 0)
        flipped = [row[:8][::-1] for row in layers["bottom"][0:8]]
        for y in range(8):
            layers["middle"][y][0:8] = flipped[y]       # horizontal mirror of the same tile
        report = self.check(layers)
        found = {(v.file, v.tile) for v in report.items if v.code == "tile.duplicate"}
        self.assertIn(("bottom.png", (2, 0)), found)
        self.assertIn(("middle.png", (0, 0)), found)
        self.assertEqual(report.errors, 0)              # a duplicate is a warning

    def test_triple_layer_metatile_is_rejected(self):
        layers = valid_layers()
        paint_cell(layers["bottom"], 7, 1, 100)
        paint_cell(layers["middle"], 7, 1, 200)
        paint_cell(layers["top"], 7, 1, 300)
        report = self.check(layers)
        hit = [v for v in report.items if v.code == "metatile.triple_layer"]
        self.assertEqual([(v.metatile, v.x, v.y) for v in hit], [(15, 112, 16)])

    def test_animation_is_reported_unsupported_not_processed(self):
        root = write_package(self.tmp / "pkg", valid_layers())
        (root / "anim").mkdir()
        (root / "anim/anim.json").write_text("{}")
        report = gfx.validate_layers(root)
        self.assertIn("animation.unsupported", report.codes())

    def test_unreadable_png(self):
        root = write_package(self.tmp / "pkg", valid_layers())
        (root / "top.png").write_bytes(b"not a png")
        self.assertIn("png.unreadable", gfx.validate_layers(root).codes())

    def test_metatile_budget_estimate_is_reported(self):
        layers = {n: blank(128, 16 * 65) for n in gfx.LAYERS}     # 8 x 65 = 520 cells
        for cell in range(520):
            paint_cell(layers["bottom"], cell % 8, cell // 8, cell * 4 % 1000)
        report = self.check(layers)
        self.assertEqual(report.usage["metatiles"], 520)
        self.assertEqual(report.totals().get("budget.metatiles"), 1)
        self.assertEqual(report.errors, 0)              # estimate: warning, compiled stage is exact

    def test_tile_budget_estimate_counts_flip_aware_unique_tiles(self):
        layers = {n: blank(128, 16 * 17) for n in gfx.LAYERS}     # 136 cells -> 544 tile slots
        for cell in range(130):
            paint_cell(layers["bottom"], cell % 8, cell // 8, cell * 4)
        report = self.check(layers)
        self.assertEqual(report.usage["tiles_estimate"], 521)     # 520 unique + the blank tile
        self.assertEqual(report.totals().get("budget.tiles"), 1)

    def test_palette_budget_estimate(self):
        layers = {n: blank(128, 16) for n in gfx.LAYERS}
        for t in range(7):                              # 7 tiles with 15 private colours each
            for i in range(15):
                layers["bottom"][i // 4][t * 8 + i % 4] = (8 * (t * 15 + i) % 248 + 8, 8 * t + 8, 8 * i + 8, 255)
        report = self.check(layers)
        self.assertEqual(report.usage["palettes_estimate"], 7)
        self.assertEqual(report.totals().get("budget.palettes"), 1)      # over the primary 6
        self.assertNotIn("budget.palettes", gfx.validate_layers(self.tmp / "pkg", "secondary").totals())

    def test_cli_exit_codes_and_json(self):
        write_package(self.tmp / "ok", valid_layers())
        bad = valid_layers()
        bad["bottom"][0][0] = (1, 2, 3, 255)
        write_package(self.tmp / "bad", bad)
        self.assertEqual(gfx.main(["validate", str(self.tmp / "ok")]), 0)
        self.assertEqual(gfx.main(["validate", str(self.tmp / "bad"), "--json"]), 1)


class CompiledValidation(Fixture):
    def make(self, words, tiles=8, attr_bytes=None, role="primary", name="bin", palettes=None):
        d = self.tmp / name
        (d / "palettes").mkdir(parents=True, exist_ok=True)
        (d / "metatiles.bin").write_bytes(b"".join(struct.pack("<H", w) for w in words))
        count = len(words) // 8
        (d / "metatile_attributes.bin").write_bytes(b"\0" * (attr_bytes if attr_bytes is not None else count * 2))
        save(d / "tiles.png", blank(8, tiles * 8))
        for slot, colour in (palettes or {}).items():
            (d / "palettes" / f"{slot:02d}.pal").write_text(f"JASC-PAL\n0100\n1\n{colour} 0 0\n")
        return d

    @staticmethod
    def meta(tile, pal):
        return tile | pal << 12

    def test_valid_compiled_primary(self):
        d = self.make([self.meta(i % 4, i % 6) for i in range(16)])
        report = gfx.validate_compiled(d)
        self.assertEqual(report.errors, 0, report.render())
        self.assertEqual(report.usage["metatiles"], 2)

    def test_triple_layer_binary_package_rejected(self):
        d = self.tmp / "tri"
        d.mkdir()
        (d / "metatiles.bin").write_bytes(b"\0" * 24 * 4)
        (d / "metatile_attributes.bin").write_bytes(b"\0" * 8)
        save(d / "tiles.png", blank(8, 8))
        report = gfx.validate_compiled(d)
        self.assertEqual(report.codes(), {"package.metatile_format"})
        self.assertIn("triple-layer", report.items[0].message)

    def test_four_byte_attributes_rejected(self):
        d = self.make([0] * 16, attr_bytes=2 * 4)
        self.assertIn("package.metatile_format", gfx.validate_compiled(d).codes())

    def test_tile_id_out_of_range_with_metatile_index(self):
        d = self.make([self.meta(1, 0)] * 8 + [self.meta(9, 0)] + [self.meta(0, 0)] * 7, tiles=8)
        hit = [v for v in gfx.validate_compiled(d).items if v.code == "tile.out_of_range"]
        self.assertEqual([v.metatile for v in hit], [1])

    def test_primary_may_not_use_secondary_palettes(self):
        d = self.make([self.meta(0, 7)] * 8)
        self.assertIn("palette.primary_overflow", gfx.validate_compiled(d).codes())

    def test_palette_slot_beyond_map_palettes(self):
        d = self.make([self.meta(0, 14)] * 8)
        self.assertIn("palette.slot_out_of_range", gfx.validate_compiled(d, "secondary").codes())

    def test_secondary_sharing_primary_palette_is_reported_and_checked_against_primary(self):
        d = self.make([self.meta(512, 2)] * 8, role="secondary", name="sec", palettes={2: 10})
        report = gfx.validate_compiled(d, "secondary")
        self.assertEqual(report.totals().get("palette.shared_with_primary"), 1)   # per metatile
        self.assertEqual(report.errors, 0)
        same = self.make([0] * 8, name="same", palettes={2: 10})
        self.assertEqual(gfx.validate_compiled(d, "secondary", same).errors, 0)
        other = self.make([0] * 8, name="other", palettes={2: 99})
        self.assertIn("palette.primary_mismatch", gfx.validate_compiled(d, "secondary", other).codes())

    def test_secondary_tile_ids_below_512_belong_to_primary(self):
        d = self.make([self.meta(5, 7)] * 8 + [self.meta(512 + 20, 7)] * 8, tiles=8, role="secondary")
        hit = [v for v in gfx.validate_compiled(d, "secondary").items if v.code == "tile.out_of_range"]
        self.assertEqual([v.metatile for v in hit], [1])

    def test_tile_and_metatile_budgets_are_exact_errors(self):
        d = self.make([self.meta(0, 0)] * (513 * 8), tiles=513)
        totals = gfx.validate_compiled(d).totals()
        self.assertEqual((totals.get("budget.metatiles"), totals.get("budget.tiles")), (1, 1))
        self.assertGreater(gfx.validate_compiled(d).errors, 0)


class PipelineGuards(Fixture):
    def test_build_refuses_the_repository_and_paths_outside_the_data_directory(self):
        for path in (ROOT, ROOT / "scripts", self.tmp):
            with self.assertRaises(SystemExit):
                gfx.scratch_project(path)


@unittest.skipUnless(os.environ.get("TAUROS_GFX_PROJECT"), "needs a scratch project under $TAUROS_DATA")
class ContainerPipeline(Fixture):
    def test_valid_tileset_compiles_and_builds(self):
        write_package(self.tmp / "src", valid_layers())
        project = Path(os.environ["TAUROS_GFX_PROJECT"])
        rc = gfx.main(["build", "--project", str(project), "--source", str(self.tmp / "src"),
                       "--name", "gTileset_GfxTest", "--make"])
        self.assertEqual(rc, 0)
        self.assertTrue((project / "pokeemerald.gba").is_file())


if __name__ == "__main__":
    unittest.main()
