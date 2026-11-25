"""GFX-009 map generator: deterministic parts and the CLI against local fake peers (no paid calls)."""
import base64
import json
import os
import re
import subprocess
import sys
import tempfile
import threading
import unittest
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "scripts"))
import asset_catalog as png  # noqa: E402
import mapgen  # noqa: E402

KEY = "sk-test-local-fixture-key-0123456789"
MAGENTA, INK = bytes([255, 0, 255, 255]), bytes([40, 90, 60, 255])


def picture(width, height, boxes, border=0, bg=MAGENTA):
    """Canvas (magenta or `bg`) with filled boxes (x, y, w, h); `border` keeps a frame, rest filled."""
    rows = []
    for y in range(height):
        row = bytearray(bg * width)
        for bx, by, bw, bh in boxes:
            if by <= y < by + bh:
                row[bx * 4:(bx + bw) * 4] = INK * bw
        if border and border <= y < height - border:
            row[border * 4:(width - border) * 4] = INK * (width - 2 * border)
        rows.append(bytes(row))
    return rows


def png_bytes(width, height, boxes=(), border=0, bg=MAGENTA):
    with tempfile.TemporaryDirectory() as tmp:
        path = Path(tmp) / "x.png"
        png.write_png(path, width, height, picture(width, height, boxes, border, bg))
        return path.read_bytes()


class FakeImages(BaseHTTPRequestHandler):
    calls = []
    fail_first = 0

    def do_POST(self):  # noqa: N802
        body = self.rfile.read(int(self.headers["Content-Length"]))
        FakeImages.calls.append({"auth": self.headers["Authorization"], "body": body})
        if FakeImages.fail_first:
            FakeImages.fail_first -= 1
            self.send_response(429)
            self.end_headers()
            self.wfile.write(json.dumps({"error": {"message": f"slow down {KEY}"}}).encode())
            return
        width, height = map(int, re.search(rb'name="size"\r\n\r\n(\d+)x(\d+)', body).groups())
        if b"tileset sheet" in body:
            boxes = [(64 + 160 * i, 64, 128, 128) for i in range(5)]
            soft = bytes([0, 0, 0, 30]) if b'name="background"\r\n\r\ntransparent' in body else MAGENTA
            image = png_bytes(width, height, boxes, bg=soft)
        else:
            image = png_bytes(width, height, border=width // 38)
        usage = {"input_tokens": 3100, "input_tokens_details": {"image_tokens": 3072, "text_tokens": 28},
                 "output_tokens": 2000}
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.end_headers()
        self.wfile.write(json.dumps({"data": [{"b64_json": base64.b64encode(image).decode()}],
                                     "usage": usage}).encode())

    def log_message(self, *args):
        pass


FAKE_CLAUDE = r'''#!/usr/bin/env python3
import json, sys
args = sys.argv
schema = args[args.index("--json-schema") + 1]
message = json.loads(sys.stdin.readline())
images = [c for c in message["message"]["content"] if c["type"] == "image"]
if '"orientation"' in schema:
    out = {"orientation": "landscape", "width": 72, "height": 52, "layout": "Castle north, plaza centre.",
           "assets": ["stone paving", "cottage"]}
elif '"uncertainties"' in schema:
    assert images, "inventory needs the layout image"
    pieces = [{"id": f"SUR_00{i}", "name": f"Stone {n}", "section": 1, "width_metatiles": 1, "height_metatiles": 1,
               "palette_family": "stone", "notes": "lavender bricks"} for i, n in enumerate(["interior", "edge", "corner"], 1)]
    out = {"assets": pieces + [
        {"id": "BLD_001", "name": "Cottage", "section": 4, "width_metatiles": 5, "height_metatiles": 5, "notes": "snowy roof"},
        {"id": "BLD_002", "name": "Hall", "section": 4, "width_metatiles": 7, "height_metatiles": 8},
        {"id": "OBJ_001", "name": "Lantern", "section": 6, "width_metatiles": 1, "height_metatiles": 2}]}
else:
    out = {"present": [1, 2], "missing": [], "cropped": [], "extra": [], "background_ok": True}
print(json.dumps({"type": "system"}))
print(json.dumps({"type": "result", "is_error": False, "structured_output": out, "total_cost_usd": 0.01}))
'''


class Deterministic(unittest.TestCase):
    def test_verbatim_templates_carry_only_the_documented_placeholders(self):
        layout = mapgen.verbatim_template(mapgen.prompt_text("01_map_layout.md"))
        sheet = mapgen.verbatim_template(mapgen.prompt_text("03_tileset_sheet_generation.md"))
        filled = mapgen.fill(layout, IMAGE_SIZE="1824×1344", CELL=24, SIZE="72 × 52", BORDER=48, LAYOUT="x")
        self.assertIn("exactly one 24×24 pixel square", filled)
        for name in ("{ASSET_LIST}", "{SHEET_SIZE}", "{BACKGROUND}"):
            self.assertIn(name, sheet)
        with self.assertRaises(mapgen.MapgenError):
            mapgen.fill(layout, SIZE="72 × 52")

    def test_layout_images_are_sized_exactly_to_the_map_grid(self):
        for w, h, cell in ((72, 52, 24), (20, 15, 48), (40, 60, 32), (100, 70, 16)):
            geometry = mapgen.layout_geometry({"width": w, "height": h})
            width, height = geometry["size"]
            self.assertEqual((geometry["cell"], width, height), (cell, (w + 4) * cell, (h + 4) * cell))
            self.assertEqual((width % 16, height % 16), (0, 0))
            self.assertLessEqual(max(width, height), 2560)
            self.assertLessEqual(min(width, height), 1440)

    def test_usage_cost_and_binary_alpha(self):
        cost = mapgen.usage_cost({"input_tokens": 1582, "input_tokens_details": {"image_tokens": 1536,
                                  "text_tokens": 46}, "output_tokens": 626}, "m")
        self.assertAlmostEqual(cost["cost_usd"], (46 * 5 + 1536 * 8 + 626 * 30) / 1e6, places=4)
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "a.png"
            png.write_png(path, 2, 1, [bytes([9, 9, 9, 253, 9, 9, 9, 40])])
            mapgen.harden_alpha(path)
            self.assertEqual(next(png.read_png(path).rgba_rows()), bytes([9, 9, 9, 255, 0, 0, 0, 0]))

    def test_plan_limits_follow_image_and_engine_bounds(self):
        ok = {"orientation": "landscape", "width": 72, "height": 52, "layout": "x"}
        self.assertEqual(mapgen.plan_limits(ok), [])
        self.assertTrue(mapgen.plan_limits({**ok, "width": 157}))
        self.assertTrue(mapgen.plan_limits({**ok, "width": 150, "height": 80}), "engine limit")
        self.assertTrue(mapgen.plan_limits({**ok, "width": 120, "height": 10}), "aspect over 3:1")
        self.assertTrue(mapgen.plan_limits({**ok, "layout": " "}))

    def test_batches_group_autotiles_keep_sections_apart_and_fit_the_sheet(self):
        assets = [{"id": f"S{i}", "name": f"piece {i}", "section": 1, "width_metatiles": 1, "height_metatiles": 1,
                   "palette_family": "stone" if i < 8 else "wood"} for i in range(12)]
        assets += [{"id": f"M{i:02d}", "name": f"module {i}", "section": 5, "width_metatiles": 4,
                    "height_metatiles": 3} for i in range(30)]
        assets += [{"id": "BIG", "name": "gate", "section": 5, "width_metatiles": 36, "height_metatiles": 20}]
        batches = mapgen.plan_batches({"assets": assets})
        ids = [x for b in batches for item in b["items"] for x in item["ids"]]
        self.assertEqual(sorted(ids), sorted(a["id"] for a in assets))
        self.assertEqual(len(batches[0]["items"]), 2, "one autotile set per material")
        for b in batches:
            self.assertLessEqual(len(b["items"]), mapgen.MAX_BATCH)
            self.assertTrue(mapgen.fits(b["items"]))
            self.assertEqual({i["section"] for i in b["items"]}, {b["section"]})
        self.assertEqual([b["n"] for b in batches], list(range(1, len(batches) + 1)))
        self.assertTrue(mapgen.validate_inventory({"assets": [{**assets[-1], "width_metatiles": 41}]}))

    def test_sheet_checks_count_separated_assets_and_magenta(self):
        small = picture(384, 256, [(10, 10, 40, 40), (80, 10, 40, 40), (150, 10, 60, 30)])
        size = mapgen.SHEET_SIZE
        good = mapgen.sheet_checks(size, small, 3)
        self.assertTrue(good["pass"], good)
        self.assertEqual(good["components"], 3)
        clear = picture(384, 256, [(10, 10, 40, 40), (80, 10, 40, 40), (150, 10, 60, 30)], bg=bytes(4))
        self.assertEqual(mapgen.sheet_checks(size, clear, 3)["components"], 3, "transparent background")
        self.assertFalse(mapgen.sheet_checks(size, small, 10)["pass"])
        self.assertFalse(mapgen.sheet_checks((1536, 1024), small, 3)["pass"])
        full = picture(384, 256, [(0, 0, 384, 256)])
        self.assertIn("background", " ".join(mapgen.sheet_checks(size, full, 1)["reasons"]))


class WithFakePeers(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        base = Path(self.tmp.name)
        self.data = base / "data"
        (self.data / "mapgen").mkdir(parents=True)
        self.env_file = base / "openai.env"
        self.env_file.write_text(f"OTHER=1\nOPENAI_API_KEY={KEY}\n")
        png.write_png(self.data / "mapgen/style_reference.png", 64, 32, picture(64, 32, [(4, 4, 8, 8)]))
        self.claude = base / "claude"
        self.claude.write_text(FAKE_CLAUDE)
        self.claude.chmod(0o755)
        FakeImages.calls, FakeImages.fail_first = [], 0
        self.server = HTTPServer(("127.0.0.1", 0), FakeImages)
        threading.Thread(target=self.server.serve_forever, daemon=True).start()
        self.env = {**os.environ, "TAUROS_DATA": str(self.data), "TAUROS_MAPGEN_ENV": str(self.env_file),
                    "TAUROS_MAPGEN_CLAUDE": str(self.claude),
                    "TAUROS_MAPGEN_OPENAI_BASE": f"http://127.0.0.1:{self.server.server_port}/v1"}

    def tearDown(self):
        self.server.shutdown()
        self.server.server_close()
        self.tmp.cleanup()

    def cli(self, *args):
        done = subprocess.run([sys.executable, str(ROOT / "scripts/mapgen.py"), *args], env=self.env,
                              capture_output=True, text=True, timeout=300)
        events = [json.loads(line) for line in done.stdout.splitlines()]
        self.assertNotIn(KEY, done.stdout + done.stderr)
        return done.returncode, events

    def test_image_requests_retry_busy_api_and_redact_errors(self):
        os.environ["TAUROS_MAPGEN_ENV"] = str(self.env_file)
        old = mapgen.OPENAI_BASE
        mapgen.OPENAI_BASE = self.env["TAUROS_MAPGEN_OPENAI_BASE"]
        try:
            FakeImages.fail_first = 1
            out = Path(self.tmp.name) / "out.png"
            settings = dict(mapgen.DEFAULT_SETTINGS)
            usage = mapgen.openai_edit("layout", [self.data / "mapgen/style_reference.png"], (1824, 1344), settings,
                                       out, model="gpt-image-2.6-flare", sleep=lambda _: None)
            self.assertEqual(png.png_size(out)[:2], (1824, 1344))
            self.assertEqual(usage["image_out"], 2000)
            self.assertEqual(len(FakeImages.calls), 2)
            self.assertEqual(FakeImages.calls[1]["auth"], f"Bearer {KEY}")
            self.assertIn(b'name="image[]"', FakeImages.calls[1]["body"])
            self.assertIn(b'name="size"\r\n\r\n1824x1344', FakeImages.calls[1]["body"])
            self.assertIn(b'name="complexity"\r\n\r\nhigh', FakeImages.calls[1]["body"])
            self.assertNotIn(b'name="background"', FakeImages.calls[1]["body"])
            FakeImages.fail_first = 5
            with self.assertRaises(mapgen.MapgenError) as error:
                mapgen.openai_edit("x", [self.data / "mapgen/style_reference.png"], (1536, 1024), settings, out,
                                   model="m", attempts=2, sleep=lambda _: None)
            self.assertNotIn(KEY, str(error.exception))
            self.assertIn("[redacted]", str(error.exception))
        finally:
            mapgen.OPENAI_BASE = old
            os.environ.pop("TAUROS_MAPGEN_ENV", None)

    def test_full_run_from_scene_to_reviewed_sheet_is_resumable(self):
        code, events = self.cli("create", "--scene", "A snowy treetop city")
        self.assertEqual(code, 0, events)
        run_id = events[-1]["run"]
        code, events = self.cli("inventory", run_id)
        self.assertEqual((code, events[-1]["message"]), (1, "accept a version first"))
        code, events = self.cli("layout", run_id)
        self.assertEqual(code, 0, events)
        state = [e for e in events if e["event"] == "state"][-1]["state"]
        self.assertEqual(state["layout"]["status"], "review")
        self.assertEqual(state["layout"]["plan"]["width"], 72)
        layout = state["layout"]["versions"][0]
        self.assertTrue(layout["checks"]["pass"], layout)
        self.assertEqual(layout["geometry"], {"cell": 24, "border": 48})
        self.assertIn(b'name="model"\r\n\r\ngpt-image-2.6-flare', FakeImages.calls[0]["body"])
        self.assertEqual(self.cli("accept", run_id, "layout")[0], 0)
        code, events = self.cli("inventory", run_id)
        self.assertEqual(code, 0, events)
        code, events = self.cli("state", run_id)
        state = events[-1]["state"]
        self.assertEqual(state["inventory"]["count"], 6)
        self.assertEqual([b["title"] for b in state["batches"]], ["Surfaces", "Buildings", "Small objects"])
        self.assertIn("autotile set", state["batches"][0]["lines"][0])
        code, events = self.cli("sheet", run_id, "2")
        self.assertEqual(code, 0, events)
        code, events = self.cli("sheet", run_id, "2", "--correction", "make the roof whiter")
        self.assertEqual(code, 0, events)
        first_sheet, correction = FakeImages.calls[-2]["body"], FakeImages.calls[-1]["body"]
        self.assertIn(b"make the roof whiter", correction)
        for body in (first_sheet, correction):
            self.assertIn(b'name="model"\r\n\r\ngpt-image-2.6-sunburst', body)
            self.assertIn(b'name="background"\r\n\r\ntransparent', body)
            self.assertIn(b'name="size"\r\n\r\n2560x1408', body)
        self.assertIn(b'name="complexity"\r\n\r\nhigh', first_sheet)
        self.assertIn(b'name="complexity"\r\n\r\nlow', correction)
        self.assertEqual(FakeImages.calls[-1]["body"].count(b'name="image[]"'), 3, "previous sheet + layout + style")
        self.assertEqual(self.cli("accept", run_id, "batch", "--batch", "2", "--version", "1")[0], 0)
        state = self.cli("state", run_id)[1][-1]["state"]
        batch = state["batches"][1]
        self.assertEqual((batch["status"], batch["accepted"], len(batch["versions"])), ("accepted", 0, 2))
        self.assertEqual(batch["versions"][0]["checks"]["verify"]["missing"], [])
        self.assertEqual(state["usage"]["image_calls"], 3)
        self.assertAlmostEqual(state["usage"]["image_cost_usd"], 3 * (28 * 5 + 3072 * 8 + 2000 * 30) / 1e6, places=3)
        self.assertTrue(batch["versions"][0]["checks"]["pass"], batch["versions"][0]["checks"])
        self.assertEqual(state["usage"]["claude_calls"], 4)
        run_dir = Path(state["dir"])
        sheet = png.read_png(run_dir / "sheet_02_v01.png")
        self.assertEqual(set(next(sheet.rgba_rows())[3::4]), {0}, "soft background alpha made transparent")
        for name in ("sheet_02_v01.png", "sheet_02_v01_raw.png", "sheet_02_v01_thumb.png", "sheet_02_v02_prompt.md",
                     "inventory.json"):
            self.assertTrue((run_dir / name).exists(), name)
        for path in run_dir.rglob("*"):
            if path.suffix in (".json", ".md"):
                self.assertNotIn(KEY, path.read_text())
        code, events = self.cli("list")
        self.assertEqual(events[-1]["runs"][0]["accepted"], 1)


if __name__ == "__main__":
    unittest.main()
