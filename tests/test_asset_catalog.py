"""Tests for scripts/asset_catalog.py using only synthetic fixtures (no real assets)."""
import json
import struct
import sys
import tempfile
import unittest
import zlib
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "scripts"))
import asset_catalog as ac  # noqa: E402


def indexed_png(path, w, h, depth, indices, palette):
    """Write a minimal indexed PNG (filter 0) from a flat list of indices."""
    per = 8 // depth
    stride = (w * depth + 7) // 8
    raw = b""
    for y in range(h):
        row = bytearray(stride)
        for x in range(w):
            v = indices[y * w + x]
            row[x // per] |= v << (8 - depth * (x % per + 1))
        raw += b"\x00" + bytes(row)

    def chunk(t, b):
        return struct.pack(">I", len(b)) + t + b + struct.pack(">I", zlib.crc32(t + b))

    plte = b"".join(bytes(c) for c in palette)
    Path(path).write_bytes(ac.PNG_MAGIC + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, depth, 3, 0, 0, 0))
                           + chunk(b"PLTE", plte) + chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b""))


def pal_file(path, colours=16):
    lines = ["JASC-PAL", "0100", str(colours)] + [f"{(i * 8) % 256} 0 0" for i in range(colours)]
    Path(path).write_text("\r\n".join(lines) + "\r\n")


def make_tileset(d: Path, layers=2, tiles_h=64, n_meta=16):
    d.mkdir(parents=True)
    indexed_png(d / "tiles.png", 128, tiles_h, 4, [i % 16 for i in range(128 * tiles_h)], [(i * 8, 0, 0) for i in range(16)])
    (d / "palettes").mkdir()
    for i in range(13):
        pal_file(d / "palettes" / f"{i:02d}.pal")
    per = 16 if layers == 2 else 24
    (d / "metatiles.bin").write_bytes(struct.pack(f"<{per // 2 * n_meta}H", *([0x0001] * (per // 2 * n_meta))))
    (d / "metatile_attributes.bin").write_bytes(b"\x00\x00" * n_meta)


class PngTests(unittest.TestCase):
    def test_roundtrip_and_crop(self):
        with tempfile.TemporaryDirectory() as t:
            rows = [bytes([i, 0, 0, 255] * 4) for i in range(4)]
            ac.write_png(Path(t) / "a.png", 4, 4, rows)
            png = ac.read_png(Path(t) / "a.png")
            self.assertEqual((png.width, png.height), (4, 4))
            self.assertEqual(list(png.rgba_rows()), rows)
            self.assertEqual(ac.crop_rows(rows, 1, 1, 2, 2)[0], rows[1][4:12])

    def test_indexed_4bit(self):
        with tempfile.TemporaryDirectory() as t:
            idx = [x % 16 for x in range(16)]
            indexed_png(Path(t) / "i.png", 16, 1, 4, idx, [(i * 16, 0, 0) for i in range(16)])
            png = ac.read_png(Path(t) / "i.png")
            self.assertEqual(list(png.index_rows())[0], bytes(idx))
            self.assertEqual(next(png.rgba_rows())[4 * 5], 80)

    def test_rejects_garbage(self):
        with tempfile.TemporaryDirectory() as t:
            (Path(t) / "x.png").write_bytes(b"not a png at all")
            with self.assertRaises(ac.PngError):
                ac.read_png(Path(t) / "x.png")

    def test_upscale_and_segments(self):
        small = [bytes([255, 0, 0, 255, 0, 0, 0, 0]), bytes([0, 0, 0, 0, 0, 255, 0, 255])]
        big = ac.scale_rows(small, 2)
        self.assertEqual(ac.detect_upscale(big, 4, 4), 2)
        self.assertEqual(ac.detect_upscale(small, 2, 2), 1)
        rows = [bytes(32 * 4) for _ in range(32)]
        rows[0] = bytes([1, 1, 1, 255]) * 8 + bytes(24 * 4)
        rows[20] = bytes(16 * 4) + bytes([1, 1, 1, 255]) * 8 + bytes(8 * 4)
        self.assertEqual(ac.segment(rows, 32, 32, 8, bridge=0), [(0, 0, 8, 8), (16, 16, 8, 8)])


class TilesetTests(unittest.TestCase):
    def test_two_layer_secondary_is_verified_statically(self):
        with tempfile.TemporaryDirectory() as t:
            d = Path(t) / "Test Secondary"
            make_tileset(d)
            facts, compat = ac.decomp_tileset_facts(d, d.name)
            self.assertEqual(compat["status"], "verified", compat["reasons"])
            self.assertEqual(facts["metatile_layers"], 2)

    def test_triple_layer_needs_conversion(self):
        with tempfile.TemporaryDirectory() as t:
            d = Path(t) / "Triple Secondary"
            make_tileset(d, layers=3)
            facts, compat = ac.decomp_tileset_facts(d, d.name)
            self.assertEqual(facts["metatile_layers"], 3)
            self.assertEqual(compat["status"], "conversion-required")
            self.assertEqual(compat["checks"]["engine_two_layer_metatiles"], "fail")

    def test_unknown_role_is_never_verified(self):
        with tempfile.TemporaryDirectory() as t:
            d = Path(t) / "Mystery"
            make_tileset(d)
            _, compat = ac.decomp_tileset_facts(d, d.name)
            self.assertEqual(compat["status"], "unverified")

    def test_tile_budget_and_missing_attributes(self):
        with tempfile.TemporaryDirectory() as t:
            d = Path(t) / "Big Primary"
            make_tileset(d, tiles_h=272)  # 34 tile rows * 16 = 544 tiles > 512
            _, compat = ac.decomp_tileset_facts(d, d.name)
            self.assertEqual(compat["checks"]["tile_budget"], "fail")
            (d / "metatile_attributes.bin").unlink()
            _, compat = ac.decomp_tileset_facts(d, d.name)
            self.assertNotEqual(compat["status"], "verified")

    def test_raw_sheet_is_never_ready(self):
        facts = {"mode": "rgba", "opaque_colours": 40, "tiles": {"analysed": True, "max_opaque_colours_per_tile": 9,
                                                              "grid_aligned": True}}
        compat = ac.raw_sheet_compat(facts)
        self.assertEqual(compat["status"], "conversion-required")
        self.assertEqual(compat["checks"]["metatile_data"], "fail")


def sample_catalog(root: Path):
    def item(iid, role, biome, kind="composite", contains=(), tags=(), status="conversion-required"):
        return {"id": iid, "source": "s", "kind": kind, "form": "raw-sheet", "role": role, "contains": list(contains),
                "title": iid.split("/")[-1].replace("-", " "), "description": f"{role} in {biome}.", "biome": biome,
                "setting": "exterior", "style": "x", "tags": list(tags), "path": "p.png", "region": None, "size": [8, 8],
                "compat": {"status": status, "scope": "s", "checks": {"metatile_data": "fail"}, "reasons": []},
                "credits": "c", "confidence": "medium", "preview": None, "meta_basis": {}}
    items = [item("s/a/ground", "ground", "snow", tags=["snowy"]), item("s/a/pine", "tree", "snow", tags=["pine", "snowy"]),
             item("s/a/house", "building", "snow", contains=["roof", "door"], tags=["house"]),
             item("s/a/cliff", "edge", "snow"), item("s/b/palm", "tree", "beach", tags=["palm"])]
    group = {"id": "grp/snow", "title": "Snow town kit", "description": "snow kit", "archetype": "town", "biome": "snow",
             "style": "x", "sources": ["s"], "members": [{"id": i["id"], "role": i["role"], "kind": i["kind"]} for i in items[:3]],
             "coverage": {"ground": "component", "path": "missing", "edge": "missing", "tree": "composite", "roof": "inside-composite-only",
                          "building": "composite", "door": "inside-composite-only", "fence": "missing", "water": "missing",
                          "prop": "missing", "vegetation": "missing"},
             "required_roles": list(ac.ARCHETYPES["town"]), "missing_roles": ["edge"], "inside_composite_only": ["roof", "door"],
             "compat_statuses": ["conversion-required"], "emerald_ready": "no", "notes": [], "preview": None}
    (root / "items.jsonl").write_text("".join(json.dumps(i) + "\n" for i in items))
    (root / "groups.jsonl").write_text(json.dumps(group) + "\n")


class SearchTests(unittest.TestCase):
    def test_interpretation_handles_italian(self):
        qi = ac.interpret("città innevata")
        self.assertEqual(qi["biomes"], ["snow"])
        self.assertEqual(qi["archetype"], "town")

    def test_group_reports_missing_components(self):
        with tempfile.TemporaryDirectory() as t:
            sample_catalog(Path(t))
            res = ac.search(Path(t), "snowy town")
            text = ac.render(res, Path(t))
            self.assertIn("grp/snow", text)
            self.assertIn("MISSING: edge", text)
            self.assertIn("not separable: roof, door", text)
            self.assertIn("Emerald-ready: no", text)

    def test_role_query_returns_pieces_not_kits(self):
        with tempfile.TemporaryDirectory() as t:
            sample_catalog(Path(t))
            res = ac.search(Path(t), "tree")
            ids = [i["id"] for _, i in res["items"]]
            self.assertIn("s/a/pine", ids)
            self.assertEqual(res["groups"], [])

    def test_biome_filter_and_empty_result(self):
        with tempfile.TemporaryDirectory() as t:
            sample_catalog(Path(t))
            ids = [i["id"] for _, i in ac.search(Path(t), "snow tree")["items"]]
            self.assertNotIn("s/b/palm", ids)
            text = ac.render(ac.search(Path(t), "cyberpunk skyline"), Path(t))
            self.assertIn("No relevant entry", text)


class ValidatorTests(unittest.TestCase):
    def test_validator_flags_bad_catalog(self):
        with tempfile.TemporaryDirectory() as t:
            root = Path(t)
            (root / "sources").mkdir()
            (root / "sources.json").write_text(json.dumps([{
                "id": "s", "name": "n", "page_url": "u", "fetch": {"kind": "none"}, "retrieved": "d", "version": "v",
                "authors": "a", "terms": {}, "credit_requirement": "c", "local_dir": "sources"}]))
            sample_catalog(root)
            items = [json.loads(l) for l in (root / "items.jsonl").read_text().splitlines()]
            items[0]["compat"]["status"] = "verified"            # verified without passing checks
            items[1]["tags"] = ["not-a-tag"]
            items[2]["description"] = "Emerald-ready house."
            items.append(dict(items[3]))                          # duplicate id
            (root / "items.jsonl").write_text("".join(json.dumps(i) + "\n" for i in items))
            (root / "p.png").write_bytes(b"")
            errs, _ = ac.validate(root)
            text = "\n".join(errs)
            self.assertIn("verified without passing", text)
            self.assertIn("tags outside vocabulary", text)
            self.assertIn("claims readiness", text)
            self.assertIn("duplicate id", text)


if __name__ == "__main__":
    unittest.main()
