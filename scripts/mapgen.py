#!/usr/bin/env python3
"""Map generator (GFX-009): automated layout and tileset generation with an image model.

Runs the steps of docs/prompts/gba-map-generation/ without a chat: the verbatim image
prompts of the prompt pack go to the OpenAI image API, the text and vision steps
(layout plan, inventory, sheet check) go to the official Claude CLI with structured
output, and batches are planned deterministically from the inventory. Every run lives
under $TAUROS_DATA/mapgen/runs/<id>/ (run.json, images, prompts) and can be resumed.

Each command prints JSON lines on stdout: {"event": "progress" | "state" | "error" | "done", ...}.
The OpenAI key is read from $TAUROS_DATA/mapgen/openai.env and never printed.
Stdlib only, Python 3.10+.
"""
from __future__ import annotations

import argparse
import base64
import json
import os
import re
import shutil
import subprocess
import sys
import time
import urllib.error
import urllib.request
import uuid
from datetime import datetime, timezone
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import asset_catalog as png  # noqa: E402  (stdlib PNG read/write helpers)

REPO = Path(__file__).resolve().parent.parent
PROMPTS = REPO / "docs/prompts/gba-map-generation"
OPENAI_BASE = os.environ.get("TAUROS_MAPGEN_OPENAI_BASE", "https://api.openai.com/v1")
# Sizes must be multiples of 16 with an aspect between 1:3 and 3:1; above 2560x1440 is experimental.
MAX_IMAGE = (2560, 1440)
SHEET_SIZE = (2560, 1408)          # 40 x 22 metatiles at 4x
SHEET_COLS, SHEET_ROWS = 40, 22
MAX_BATCH = 16
LAYOUT_BORDER = 2                  # magenta metatiles around the layout
# USD per 1M tokens (gpt-image-2.6 Sunburst and Flare, 2026-11-24).
RATES = {"text_in": 5.0, "image_in": 8.0, "image_out": 30.0}
SECTIONS = {1: "Surfaces", 2: "Linear elements", 3: "Vegetation", 4: "Buildings",
            5: "Structural modules", 6: "Small objects"}
DEFAULT_SETTINGS = {"layout_model": "gpt-image-2.6-flare", "sheet_model": "gpt-image-2.6-sunburst",
                    "image_quality": "high", "transparent": True, "claude_model": "opus",
                    "auto_retries": 1, "verify": True}
RETRY_STATUS = {408, 409, 429, 500, 502, 503, 504}
CLAUDE_IMAGE_LIMIT = 3_500_000


class MapgenError(RuntimeError):
    pass


# --------------------------------------------------------------------------- io

def emit(event: str, **fields) -> None:
    print(json.dumps({"event": event, **fields}), flush=True)


def progress(message: str, **fields) -> None:
    emit("progress", message=message, **fields)


def mapgen_root() -> Path:
    base = os.environ.get("TAUROS_DATA") or str(Path.home() / ".local/share/tauros")
    return Path(base) / "mapgen"


def now() -> str:
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def write_json(path: Path, data) -> None:
    tmp = path.with_suffix(path.suffix + ".tmp")
    tmp.write_text(json.dumps(data, indent=1, ensure_ascii=False))
    tmp.replace(path)


def load_env_key(path: Path | None = None) -> str:
    path = path or Path(os.environ.get("TAUROS_MAPGEN_ENV") or mapgen_root() / "openai.env")
    if not path.exists():
        raise MapgenError(f"OpenAI key file missing: {path.name} in the mapgen data folder.")
    for line in path.read_text().splitlines():
        key, _, value = line.strip().partition("=")
        if key == "OPENAI_API_KEY" and value.strip():
            return value.strip().strip("'\"")
    raise MapgenError("OPENAI_API_KEY is not set in the mapgen key file.")


# --------------------------------------------------------------------------- prompts

def prompt_text(name: str) -> str:
    return (PROMPTS / name).read_text()


def verbatim_template(markdown: str) -> str:
    """The fenced block under the 'IMAGE PROMPT (SEND VERBATIM)' heading."""
    match = re.search(r"^##[^\n]*IMAGE PROMPT \(SEND VERBATIM\)[^\n]*\n.*?```[^\n]*\n(.*?)```", markdown,
                      re.S | re.M)
    if not match:
        raise MapgenError("verbatim image prompt not found in the prompt pack")
    return match.group(1).strip()


def fill(template: str, **values) -> str:
    for key, value in values.items():
        template = template.replace("{" + key + "}", str(value))
    left = re.findall(r"\{[A-Z_]+\}", template)
    if left:
        raise MapgenError(f"unfilled placeholders: {', '.join(left)}")
    return template


def correction_prompt(what: str, correction: str, original: str) -> str:
    return ("Edit the first attached image, which is " + what + ". Apply only this correction and keep "
            "everything else unchanged, in the same style, scale and format:\n" + correction.strip() +
            "\n\nThe other attached images are the design references. Original instructions, for context:\n"
            + original)


# --------------------------------------------------------------------------- planning

def layout_geometry(plan: dict) -> dict:
    """Exact image size for the layout: (map + border) x cell, the largest cell that fits."""
    cols, rows = plan["width"] + 2 * LAYOUT_BORDER, plan["height"] + 2 * LAYOUT_BORDER
    limit = MAX_IMAGE if cols >= rows else MAX_IMAGE[::-1]
    for cell in (48, 40, 32, 28, 24, 20, 16):
        w, h = cols * cell, rows * cell
        if w <= limit[0] and h <= limit[1] and w % 16 == 0 and h % 16 == 0:
            return {"cell": cell, "size": (cols * cell, rows * cell), "border": LAYOUT_BORDER * cell}
    raise MapgenError(f"a {plan['width']}×{plan['height']} map does not fit a {limit[0]}×{limit[1]} image")


def plan_limits(plan: dict) -> list[str]:
    w, h = plan.get("width"), plan.get("height")
    if not isinstance(w, int) or not isinstance(h, int) or w < 10 or h < 10:
        return ["width and height must be integers of at least 10 metatiles"]
    problems = []
    long_side, short_side = MAX_IMAGE[0] // 16 - 2 * LAYOUT_BORDER, MAX_IMAGE[1] // 16 - 2 * LAYOUT_BORDER
    if max(w, h) > long_side or min(w, h) > short_side:
        problems.append(f"{w}×{h} exceeds {long_side}×{short_side} metatiles")
    ratio = (w + 2 * LAYOUT_BORDER) / (h + 2 * LAYOUT_BORDER)
    if not 1 / 3 <= ratio <= 3:
        problems.append("the map must be at most three times longer than wide")
    if (w + 15) * (h + 14) > 10240:
        problems.append("(width + 15) × (height + 14) exceeds the engine limit of 10240 metatiles")
    if not str(plan.get("layout", "")).strip():
        problems.append("layout text is empty")
    return problems


def validate_inventory(inv: dict) -> list[str]:
    problems, seen = [], set()
    assets = inv.get("assets")
    if not isinstance(assets, list) or not assets:
        return ["inventory has no assets"]
    for a in assets:
        aid = a.get("id")
        if not aid or aid in seen:
            problems.append(f"missing or duplicate id: {aid!r}")
        seen.add(aid)
        w, h = a.get("width_metatiles"), a.get("height_metatiles")
        if not isinstance(w, int) or not isinstance(h, int) or w < 1 or h < 1:
            problems.append(f"{aid}: footprint must be positive integers")
        elif w > SHEET_COLS or h > SHEET_ROWS:
            problems.append(f"{aid}: {w}×{h} does not fit a {SHEET_COLS}×{SHEET_ROWS} sheet; decompose it")
        if a.get("section") not in SECTIONS:
            problems.append(f"{aid}: section must be 1-6")
    return problems


def describe(asset: dict, limit: int = 170) -> str:
    notes = " ".join(str(asset.get("notes") or "").split())
    if len(notes) > limit:
        notes = notes[:limit].rsplit(" ", 1)[0] + "…"
    line = f"{asset['name']}: {asset['width_metatiles']}×{asset['height_metatiles']} tiles"
    return f"{line}, {notes}" if notes else line


def batch_items(assets: list[dict]) -> list[dict]:
    """Drawable units: 1×1 surface pieces of one material become one autotile set."""
    items, groups = [], {}
    for a in assets:
        if a["section"] == 1 and a["width_metatiles"] == a["height_metatiles"] == 1:
            family = a.get("palette_family") or a["name"].split()[0]
            groups.setdefault(family, []).append(a)
        else:
            items.append({"ids": [a["id"]], "section": a["section"], "w": a["width_metatiles"],
                          "h": a["height_metatiles"], "line": describe(a)})
    for family, members in groups.items():
        if len(members) == 1:
            a = members[0]
            items.append({"ids": [a["id"]], "section": 1, "w": 1, "h": 1, "line": describe(a)})
            continue
        names = "; ".join(m["name"] for m in members)
        hint = describe(members[0], 120).split(", ", 1)[-1]
        items.append({"ids": [m["id"] for m in members], "section": 1, "w": 6, "h": 3,
                      "line": (f"{family.replace('_', ' ')} ground autotile set ({names}): each piece is 1×1 tile, "
                               f"drawn as a joined 3×3 block (outer corners, edges, seamless interior) plus a "
                               f"separate 2×2 block of inner corners if listed, {hint}")})
    items.sort(key=lambda i: (i["section"], i["ids"][0]))
    return items


def fits(items: list[dict]) -> bool:
    """Shelf packing with a one-tile gap after every asset."""
    x = y = shelf = 0
    for it in sorted(items, key=lambda i: -i["h"]):
        w, h = it["w"] + 1, it["h"] + 1
        if x + w > SHEET_COLS + 1:
            x, y, shelf = 0, y + shelf, 0
        if w > SHEET_COLS + 1 or y + h > SHEET_ROWS + 1:
            return False
        x, shelf = x + w, max(shelf, h)
    return True


def plan_batches(inventory: dict) -> list[dict]:
    batches = []
    for item in batch_items(inventory["assets"]):
        last = batches[-1] if batches else None
        if (last and last["section"] == item["section"] and len(last["items"]) < MAX_BATCH
                and fits(last["items"] + [item])):
            last["items"].append(item)
        else:
            batches.append({"section": item["section"], "items": [item]})
    for n, b in enumerate(batches, 1):
        same = [x for x in batches if x["section"] == b["section"]]
        part = f" {same.index(b) + 1}/{len(same)}" if len(same) > 1 else ""
        b.update(n=n, title=SECTIONS[b["section"]] + part, status="pending", versions=[], accepted=None)
    return batches


# --------------------------------------------------------------------------- image checks

def is_magenta(r: int, g: int, b: int) -> bool:
    return r > 200 and b > 200 and g < 90


def is_background(px: bytes, i: int) -> bool:
    return px[i + 3] < 128 or is_magenta(px[i], px[i + 1], px[i + 2])


def harden_alpha(path: Path) -> None:
    """The model returns soft alpha (halos, 252-253 for solid pixels): make it binary."""
    image = png.read_png(path)
    rows = []
    for r in image.rgba_rows():
        b = bytearray(r)
        for i in range(3, len(b), 4):
            if b[i] < 128:
                b[i - 3:i + 1] = b"\0\0\0\0"
            else:
                b[i] = 255
        rows.append(bytes(b))
    png.write_png(path, image.width, image.height, rows)


def thumbnail(path: Path, out: Path, factor: int = 4) -> tuple[int, int, list[bytes]]:
    image = png.read_png(path)
    rows = list(image.rgba_rows())
    small = png.scale_rows(rows, 1, factor)
    png.write_png(out, len(small[0]) // 4, len(small), small)
    return image.width, image.height, small


def sheet_checks(size: tuple[int, int], small: list[bytes], expected: int, want=SHEET_SIZE) -> dict:
    """Size, magenta background and a count of separated assets on the 1/4 thumbnail."""
    h, w = len(small), len(small[0]) // 4
    background = sum(is_background(r, i) for r in small for i in range(0, len(r), 4))
    ratio = background / (w * h)
    cell = 4  # 16 px of the full image
    cw, ch = w // cell, h // cell
    filled = [[False] * cw for _ in range(ch)]
    for cy in range(ch):
        for cx in range(cw):
            n = 0
            for y in range(cy * cell, cy * cell + cell):
                r = small[y]
                for x in range(cx * cell, cx * cell + cell):
                    if not is_background(r, 4 * x):
                        n += 1
            filled[cy][cx] = n * 3 > cell * cell
    seen, components = set(), 0
    for cy in range(ch):
        for cx in range(cw):
            if not filled[cy][cx] or (cx, cy) in seen:
                continue
            stack, area = [(cx, cy)], 0
            seen.add((cx, cy))
            while stack:
                x, y = stack.pop()
                area += 1
                for nx, ny in ((x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)):
                    if 0 <= nx < cw and 0 <= ny < ch and filled[ny][nx] and (nx, ny) not in seen:
                        seen.add((nx, ny))
                        stack.append((nx, ny))
            components += area >= 4
    reasons = []
    if tuple(size) != tuple(want):
        reasons.append(f"size {size[0]}×{size[1]} instead of {want[0]}×{want[1]}")
    if ratio < 0.2:
        reasons.append(f"background is not empty ({ratio:.0%} transparent or magenta)")
    if expected and components < max(1, round(expected * 0.6)):
        reasons.append(f"only {components} separate assets found for {expected} expected")
    return {"size": list(size), "background_ratio": round(ratio, 3), "components": components,
            "expected": expected, "pass": not reasons, "reasons": reasons}


def layout_checks(size: tuple[int, int], small: list[bytes], want: tuple[int, int]) -> dict:
    h, w = len(small), len(small[0]) // 4
    edge = [small[0], small[-1]] + [bytes(r[0:4] + r[-4:]) for r in small]
    pixels = [(r[i], r[i + 1], r[i + 2]) for r in edge for i in range(0, len(r), 4)]
    border = sum(is_magenta(*p) for p in pixels) / max(1, len(pixels))
    reasons = []
    if tuple(size) != tuple(want):
        reasons.append(f"size {size[0]}×{size[1]} instead of {want[0]}×{want[1]}")
    if border < 0.6:
        reasons.append(f"magenta border incomplete ({border:.0%} of the edge)")
    return {"size": list(size), "border_ratio": round(border, 3), "pass": not reasons, "reasons": reasons,
            "thumb_size": [w, h]}


# --------------------------------------------------------------------------- OpenAI images

def multipart(fields: dict, files: list[tuple[str, Path]]) -> tuple[bytes, str]:
    boundary = "tauros" + uuid.uuid4().hex
    out = bytearray()
    for name, value in fields.items():
        out += (f"--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n").encode()
    for name, path in files:
        out += (f"--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"; filename=\"{path.name}\"\r\n"
                f"Content-Type: image/png\r\n\r\n").encode()
        out += path.read_bytes() + b"\r\n"
    out += f"--{boundary}--\r\n".encode()
    return bytes(out), f"multipart/form-data; boundary={boundary}"


def openai_edit(prompt: str, images: list[Path], size: tuple[int, int], settings: dict, out: Path, *,
                model: str, complexity: str = "high", transparent: bool = False, attempts: int = 4,
                sleep=time.sleep) -> dict:
    """One image-edit call; writes the PNG to `out` and returns token usage with its cost."""
    key = load_env_key()
    fields = {"model": model, "prompt": prompt, "size": f"{size[0]}x{size[1]}",
              "quality": settings["image_quality"], "n": "1", "output_format": "png", "complexity": complexity}
    if transparent:
        fields["background"] = "transparent"
    body, ctype = multipart(fields, [("image[]", p) for p in images])
    delay = 4
    for attempt in range(1, attempts + 1):
        request = urllib.request.Request(f"{OPENAI_BASE}/images/edits", data=body, method="POST",
                                         headers={"Authorization": f"Bearer {key}", "Content-Type": ctype})
        try:
            with urllib.request.urlopen(request, timeout=600) as response:
                payload = json.loads(response.read())
            data = (payload.get("data") or [{}])[0]
            if not data.get("b64_json"):
                raise MapgenError("the image API returned no image")
            out.write_bytes(base64.b64decode(data["b64_json"]))
            if transparent:
                shutil.copyfile(out, out.with_name(out.stem + "_raw.png"))
                harden_alpha(out)
            return usage_cost(payload.get("usage") or {}, model)
        except urllib.error.HTTPError as error:
            detail = _api_message(error)
            if error.code not in RETRY_STATUS or attempt == attempts:
                raise MapgenError(f"image API error {error.code}: {detail}") from None
            progress(f"Image API busy ({error.code}), retrying in {delay}s ({attempt}/{attempts - 1})")
        except (urllib.error.URLError, TimeoutError, ConnectionError) as error:
            if attempt == attempts:
                raise MapgenError(f"image API unreachable: {getattr(error, 'reason', error)}") from None
            progress(f"Image API unreachable, retrying in {delay}s ({attempt}/{attempts - 1})")
        sleep(delay)
        delay *= 2
    raise MapgenError("image generation failed")


def usage_cost(usage: dict, model: str) -> dict:
    details = usage.get("input_tokens_details") or {}
    image_in = details.get("image_tokens", 0)
    text_in = details.get("text_tokens", max(0, usage.get("input_tokens", 0) - image_in))
    image_out = usage.get("output_tokens", 0)
    cost = (text_in * RATES["text_in"] + image_in * RATES["image_in"] + image_out * RATES["image_out"]) / 1e6
    return {"model": model, "text_in": text_in, "image_in": image_in, "image_out": image_out,
            "cost_usd": round(cost, 4)}


def _api_message(error: urllib.error.HTTPError) -> str:
    try:
        message = json.loads(error.read()).get("error", {}).get("message", "")
    except (ValueError, AttributeError):
        message = ""
    return re.sub(r"sk-[A-Za-z0-9_\-]{8,}", "[redacted]", message)[:400] or error.reason


# --------------------------------------------------------------------------- Claude CLI

def claude_json(text: str, images: list[Path], schema: dict, settings: dict, run: dict,
                timeout: int = 1200) -> dict:
    content = []
    for path in images:
        data = path.read_bytes()
        if len(data) > CLAUDE_IMAGE_LIMIT:
            image = png.read_png(path)
            rows = png.scale_rows(list(image.rgba_rows()), 1, 2)
            small = path.with_name(path.stem + "_half.png")
            png.write_png(small, len(rows[0]) // 4, len(rows), rows)
            data = small.read_bytes()
        content.append({"type": "image", "source": {"type": "base64", "media_type": "image/png",
                                                     "data": base64.b64encode(data).decode()}})
    content.append({"type": "text", "text": text})
    message = {"type": "user", "message": {"role": "user", "content": content}}
    command = [os.environ.get("TAUROS_MAPGEN_CLAUDE", "claude"), "-p", "--input-format", "stream-json",
               "--output-format", "stream-json", "--verbose", "--no-session-persistence", "--tools", "",
               "--json-schema", json.dumps(schema), "--model", settings["claude_model"]]
    try:
        done = subprocess.run(command, input=json.dumps(message) + "\n", text=True, capture_output=True,
                              timeout=timeout)
    except FileNotFoundError:
        raise MapgenError("Claude CLI not found; install it and sign in with `claude`.") from None
    except subprocess.TimeoutExpired:
        raise MapgenError("Claude CLI timed out.") from None
    result = None
    for line in done.stdout.splitlines():
        try:
            event = json.loads(line)
        except ValueError:
            continue
        if event.get("type") == "result":
            result = event
    if not result or result.get("is_error") or result.get("structured_output") is None:
        reason = (result or {}).get("result") or (done.stderr.strip().splitlines() or ["no result"])[-1]
        raise MapgenError(f"Claude CLI failed: {str(reason)[:300]}")
    usage = run.setdefault("usage", {})
    usage["claude_calls"] = usage.get("claude_calls", 0) + 1
    usage["claude_cost_usd"] = round(usage.get("claude_cost_usd", 0) + (result.get("total_cost_usd") or 0), 4)
    return result["structured_output"]


PLAN_SCHEMA = {"type": "object", "required": ["orientation", "width", "height", "layout", "assets"],
               "properties": {"orientation": {"enum": ["landscape", "portrait"]},
                              "width": {"type": "integer"}, "height": {"type": "integer"},
                              "layout": {"type": "string"},
                              "assets": {"type": "array", "items": {"type": "string"}}}}
ASSET_SCHEMA = {"type": "object", "required": ["id", "name", "section", "width_metatiles", "height_metatiles"],
                "properties": {"id": {"type": "string"}, "name": {"type": "string"},
                               "section": {"type": "integer"}, "width_metatiles": {"type": "integer"},
                               "height_metatiles": {"type": "integer"}, "layer": {"type": "string"},
                               "palette_family": {"type": "string"}, "parent_asset": {"type": ["string", "null"]},
                               "collision": {"type": "string"}, "behavior": {"type": "string"},
                               "notes": {"type": "string"}}}
INVENTORY_SCHEMA = {"type": "object", "required": ["assets"],
                    "properties": {"assets": {"type": "array", "items": ASSET_SCHEMA},
                                   "uncertainties": {"type": "array", "items": {"type": "string"}}}}
VERIFY_SCHEMA = {"type": "object", "required": ["present", "missing", "cropped", "extra", "background_ok"],
                 "properties": {"present": {"type": "array", "items": {"type": "integer"}},
                                "missing": {"type": "array", "items": {"type": "integer"}},
                                "cropped": {"type": "array", "items": {"type": "integer"}},
                                "extra": {"type": "array", "items": {"type": "string"}},
                                "background_ok": {"type": "boolean"}, "notes": {"type": "string"}}}


# --------------------------------------------------------------------------- runs

class Run:
    def __init__(self, path: Path):
        self.dir = path
        self.data = json.loads((path / "run.json").read_text())

    @classmethod
    def open(cls, run_id: str) -> "Run":
        if not re.fullmatch(r"[A-Za-z0-9_\-]+", run_id or ""):
            raise MapgenError("invalid run id")
        path = mapgen_root() / "runs" / run_id
        if not (path / "run.json").exists():
            raise MapgenError(f"run {run_id} not found")
        return cls(path)

    def save(self) -> None:
        self.data["updated"] = now()
        write_json(self.dir / "run.json", self.data)
        emit("state", state=self.public())

    def public(self) -> dict:
        return {**self.data, "settings": self.settings, "dir": str(self.dir)}

    @property
    def settings(self) -> dict:
        return {**DEFAULT_SETTINGS, **self.data.get("settings", {})}

    def count_image(self, version: dict, usage: dict) -> None:
        version["usage"] = usage
        total = self.data.setdefault("usage", {})
        total["image_calls"] = total.get("image_calls", 0) + 1
        total["image_cost_usd"] = round(total.get("image_cost_usd", 0) + usage["cost_usd"], 4)

    def accepted(self, step: dict) -> Path:
        index = step.get("accepted")
        if index is None:
            raise MapgenError("accept a version first")
        return self.dir / step["versions"][index]["file"]


def style_path(run: Run) -> Path:
    path = run.dir / "style_reference.png"
    if not path.exists():
        raise MapgenError("style reference missing from the run")
    return path


def new_version(run: Run, step: dict, stem: str, prompt: str, kind: str, correction: str = "") -> dict:
    number = len(step["versions"]) + 1
    version = {"file": f"{stem}_v{number:02d}.png", "thumb": f"{stem}_v{number:02d}_thumb.png",
               "prompt": f"{stem}_v{number:02d}_prompt.md", "kind": kind, "correction": correction,
               "created": now()}
    (run.dir / version["prompt"]).write_text(prompt)
    return version


def cmd_create(args) -> None:
    scene = Path(args.scene_file).read_text() if args.scene_file else (args.scene or "")
    if not scene.strip():
        raise MapgenError("describe the scene first")
    style = Path(args.style).expanduser() if args.style else mapgen_root() / "style_reference.png"
    if not style.exists():
        raise MapgenError(f"style reference not found: {style.name}")
    if png.png_size(style)[:2] == (0, 0):
        raise MapgenError("style reference is not a PNG")
    slug = re.sub(r"[^a-z0-9]+", "-", scene.lower())[:32].strip("-") or "map"
    run_id = datetime.now().strftime("%Y%m%d-%H%M%S-") + slug
    path = mapgen_root() / "runs" / run_id
    path.mkdir(parents=True)
    shutil.copyfile(style, path / "style_reference.png")
    write_json(path / "run.json", {
        "id": run_id, "created": now(), "scene": scene.strip(), "settings": dict(DEFAULT_SETTINGS),
        "layout": {"status": "pending", "plan": None, "versions": [], "accepted": None},
        "inventory": {"status": "pending", "file": None, "count": 0, "uncertainties": []},
        "batches": [], "usage": {"image_calls": 0, "image_cost_usd": 0, "claude_calls": 0, "claude_cost_usd": 0}})
    run = Run(path)
    run.save()
    emit("done", run=run_id)


def cmd_list(_args) -> None:
    runs = []
    for path in sorted((mapgen_root() / "runs").glob("*/run.json"), reverse=True):
        try:
            data = json.loads(path.read_text())
        except ValueError:
            continue
        done = sum(b.get("accepted") is not None for b in data.get("batches", []))
        runs.append({"id": data["id"], "scene": data["scene"][:80], "updated": data.get("updated"),
                     "batches": len(data.get("batches", [])), "accepted": done})
    key = mapgen_root() / "openai.env"
    emit("done", runs=runs, key_ready=key.exists(), default_style=(mapgen_root() / "style_reference.png").exists())


def cmd_state(args) -> None:
    emit("done", state=Run.open(args.run).public())


def cmd_settings(args) -> None:
    run = Run.open(args.run)
    settings = run.settings
    for name in ("layout_model", "sheet_model", "image_quality", "claude_model"):
        if getattr(args, name):
            settings[name] = getattr(args, name)
    if args.auto_retries is not None:
        settings["auto_retries"] = max(0, min(3, args.auto_retries))
    if args.verify is not None:
        settings["verify"] = args.verify == "on"
    if args.transparent is not None:
        settings["transparent"] = args.transparent == "on"
    run.data["settings"] = settings
    run.save()
    emit("done")


def cmd_layout(args) -> None:
    run = Run.open(args.run)
    step = run.data["layout"]
    settings = run.settings
    pack = prompt_text("01_map_layout.md")
    if step["plan"] is None or args.replan:
        step["status"] = "planning"
        run.save()
        progress("Claude is designing the map layout…")
        note = ""
        for _ in range(2):
            plan = claude_json(
                "You run steps 1 and 3 of the prompt below inside an automated pipeline. Do not generate "
                "images. Design the full map for the scene description and return JSON: orientation, width "
                "and height in metatiles, layout (the text that replaces {LAYOUT}: at most 150 words of "
                "concrete English, map size first) and assets (every distinct asset design you plan, one "
                "short line each)." + note + "\n\n=== PROMPT ===\n" + pack +
                "\n\n=== SCENE DESCRIPTION ===\n" + run.data["scene"], [], PLAN_SCHEMA, settings, run.data)
            problems = plan_limits(plan)
            if not problems:
                break
            note = " A previous answer was rejected: " + "; ".join(problems) + "."
        else:
            step["status"] = "failed"
            run.save()
            raise MapgenError("layout plan invalid: " + "; ".join(problems))
        step["plan"] = plan
        run.save()
    plan = step["plan"]
    geometry = layout_geometry(plan)
    size = geometry["size"]
    prompt = fill(verbatim_template(pack), IMAGE_SIZE=f"{size[0]}×{size[1]}", CELL=geometry["cell"],
                  SIZE=f"{plan['width']} × {plan['height']}", BORDER=geometry["border"], LAYOUT=plan["layout"])
    inputs, kind, complexity = [style_path(run)], "generate", "high"
    if args.correction:
        base = step["versions"][-1] if step["versions"] else None
        if not base:
            raise MapgenError("generate a layout before correcting it")
        inputs = [run.dir / base["file"], style_path(run)]
        prompt, kind = correction_prompt("a full RPG map layout", args.correction, prompt), "edit"
        complexity = "low"
    version = new_version(run, step, "layout", prompt, kind, args.correction or "")
    step["status"] = "generating"
    run.save()
    progress("Generating the layout image…")
    usage = openai_edit(prompt, inputs, size, settings, run.dir / version["file"],
                        model=settings["layout_model"], complexity=complexity)
    run.count_image(version, usage)
    version["geometry"] = {"cell": geometry["cell"], "border": geometry["border"]}
    width, height, small = thumbnail(run.dir / version["file"], run.dir / version["thumb"])
    version["checks"] = layout_checks((width, height), small, size)
    step["versions"].append(version)
    step["status"] = "review"
    run.save()
    emit("done")


def cmd_accept(args) -> None:
    run = Run.open(args.run)
    if args.step == "layout":
        step = run.data["layout"]
    elif args.step == "batch":
        step = _batch(run, args.batch)
    else:
        raise MapgenError("unknown step")
    if not step["versions"]:
        raise MapgenError("nothing to accept yet")
    index = len(step["versions"]) - 1 if args.version is None else args.version - 1
    if not 0 <= index < len(step["versions"]):
        raise MapgenError("no such version")
    step["accepted"], step["status"] = index, "accepted"
    if args.step == "layout" and run.data["inventory"]["status"] != "pending":
        run.data["inventory"] = {"status": "pending", "file": None, "count": 0, "uncertainties": []}
        run.data["batches"] = []
        progress("Layout changed: inventory and batches reset.")
    run.save()
    emit("done")


def cmd_inventory(args) -> None:
    run = Run.open(args.run)
    layout = run.accepted(run.data["layout"])
    step = run.data["inventory"]
    step["status"] = "running"
    run.save()
    progress("Claude is building the asset inventory from the layout…")
    plan = json.dumps(run.data["layout"]["plan"], ensure_ascii=False)
    inventory = claude_json(
        "Execute the prompt below on the attached map layout image, inside an automated pipeline: return "
        "only the JSON. Use sections 1-6 exactly as defined (1 surfaces, 2 linear elements, 3 vegetation, "
        "4 buildings, 5 structural modules, 6 small objects), integer footprints in metatiles at game "
        "scale, at most 24×16 each (decompose larger structures into modules), and describe each asset's "
        "design, colours and materials in notes. The layout was planned as: " + plan +
        "\n\n=== PROMPT ===\n" + prompt_text("02_asset_inventory.md"),
        [layout], INVENTORY_SCHEMA, run.settings, run.data)
    problems = validate_inventory(inventory)
    if problems:
        step["status"], step["problems"] = "failed", problems
        run.save()
        raise MapgenError("inventory invalid: " + "; ".join(problems[:5]))
    write_json(run.dir / "inventory.json", inventory)
    run.data["inventory"] = {"status": "accepted", "file": "inventory.json", "count": len(inventory["assets"]),
                             "uncertainties": inventory.get("uncertainties", [])[:20]}
    batches = plan_batches(inventory)
    for b in batches:
        b["lines"] = [i["line"] for i in b["items"]]
        b["ids"] = [x for i in b["items"] for x in i["ids"]]
        b["expected"] = len(b["items"])
        del b["items"]
    run.data["batches"] = batches
    progress(f"{len(inventory['assets'])} assets in {len(batches)} batches.")
    run.save()
    emit("done")


def _batch(run: Run, number) -> dict:
    for b in run.data["batches"]:
        if b["n"] == number:
            return b
    raise MapgenError(f"batch {number} not found")


def generate_sheet(run: Run, batch: dict, correction: str = "") -> None:
    settings = run.settings
    layout = run.accepted(run.data["layout"])
    transparent = settings["transparent"]
    base_prompt = fill(verbatim_template(prompt_text("03_tileset_sheet_generation.md")),
                       SHEET_SIZE=f"{SHEET_SIZE[0]}×{SHEET_SIZE[1]}",
                       BACKGROUND="fully transparent" if transparent else "solid pure magenta (#FF00FF)",
                       ASSET_LIST="\n".join(f"- {line}" for line in batch["lines"]))
    stem = f"sheet_{batch['n']:02d}"
    if correction:
        if not batch["versions"]:
            raise MapgenError("generate the batch before correcting it")
        previous = run.dir / batch["versions"][-1]["file"]
        attempts = [(correction_prompt("a game tileset sheet", correction, base_prompt),
                     [previous, layout, style_path(run)], "edit", "low")]
    else:
        attempts = [(base_prompt, [layout, style_path(run)], "generate", "high")]
    retries = settings["auto_retries"]
    while attempts:
        prompt, inputs, kind, complexity = attempts.pop(0)
        version = new_version(run, batch, stem, prompt, kind, correction)
        batch["status"] = "generating"
        run.save()
        progress(f"Batch {batch['n']} · {batch['title']}: generating ({len(batch['lines'])} assets)…",
                 batch=batch["n"])
        usage = openai_edit(prompt, inputs, SHEET_SIZE, settings, run.dir / version["file"],
                            model=settings["sheet_model"], complexity=complexity, transparent=transparent)
        run.count_image(version, usage)
        width, height, small = thumbnail(run.dir / version["file"], run.dir / version["thumb"])
        checks = sheet_checks((width, height), small, batch["expected"])
        if settings["verify"]:
            batch["status"] = "checking"
            run.save()
            progress(f"Batch {batch['n']}: Claude is checking the sheet…", batch=batch["n"])
            listing = "\n".join(f"{i}. {line}" for i, line in enumerate(batch["lines"], 1))
            try:
                verdict = claude_json(
                    "The attached image should be a game tileset sheet on an empty background "
                    "containing exactly these numbered assets, each complete and separated:\n" + listing +
                    "\nReport by number which are present and recognisable, which are missing, which are "
                    "cropped or merged with another asset, anything drawn that is not in the list, and "
                    "whether the background is empty (transparent or pure magenta).", [run.dir / version["file"]],
                    VERIFY_SCHEMA,
                    settings, run.data, timeout=600)
                checks["verify"] = verdict
                missing = sorted(set(verdict.get("missing", [])) | set(verdict.get("cropped", [])))
                if missing:
                    checks["pass"] = False
                    checks["reasons"].append("missing or cropped: " + ", ".join(map(str, missing)))
            except MapgenError as error:
                checks["verify_error"] = str(error)
        version["checks"] = checks
        batch["versions"].append(version)
        if checks["pass"] or retries <= 0:
            break
        retries -= 1
        progress(f"Batch {batch['n']}: check failed ({'; '.join(checks['reasons'])}), retrying.",
                 batch=batch["n"])
        attempts.append((base_prompt + "\n\nPrevious attempt failed: " + "; ".join(checks["reasons"]) +
                         ". Fix these: draw every listed asset once, complete and separated.",
                         [layout, style_path(run)], "retry", "high"))
    batch["status"] = "review"
    run.save()


def cmd_sheet(args) -> None:
    run = Run.open(args.run)
    generate_sheet(run, _batch(run, args.batch), args.correction or "")
    emit("done")


def cmd_sheets(args) -> None:
    run = Run.open(args.run)
    pending = [b for b in run.data["batches"] if not b["versions"]]
    if not pending:
        raise MapgenError("every batch already has a sheet")
    for index, batch in enumerate(pending, 1):
        progress(f"Sheet {index} of {len(pending)}", total=len(pending), index=index)
        generate_sheet(run, batch)
    emit("done")


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    sub = parser.add_subparsers(dest="command", required=True)
    p = sub.add_parser("create")
    p.add_argument("--scene")
    p.add_argument("--scene-file")
    p.add_argument("--style")
    sub.add_parser("list")
    for name in ("state", "inventory", "sheets"):
        sub.add_parser(name).add_argument("run")
    p = sub.add_parser("settings")
    p.add_argument("run")
    p.add_argument("--layout-model")
    p.add_argument("--sheet-model")
    p.add_argument("--image-quality", choices=["low", "medium", "high", "xhigh", "max"])
    p.add_argument("--transparent", choices=["on", "off"])
    p.add_argument("--claude-model")
    p.add_argument("--auto-retries", type=int)
    p.add_argument("--verify", choices=["on", "off"])
    p = sub.add_parser("layout")
    p.add_argument("run")
    p.add_argument("--correction")
    p.add_argument("--replan", action="store_true")
    p = sub.add_parser("sheet")
    p.add_argument("run")
    p.add_argument("batch", type=int)
    p.add_argument("--correction")
    p = sub.add_parser("accept")
    p.add_argument("run")
    p.add_argument("step", choices=["layout", "batch"])
    p.add_argument("--batch", type=int)
    p.add_argument("--version", type=int)
    args = parser.parse_args(argv)
    handler = {"create": cmd_create, "list": cmd_list, "state": cmd_state, "settings": cmd_settings,
               "layout": cmd_layout, "accept": cmd_accept, "inventory": cmd_inventory,
               "sheet": cmd_sheet, "sheets": cmd_sheets}[args.command]
    try:
        handler(args)
    except (MapgenError, png.PngError, OSError) as error:
        _fail_running(args)
        emit("error", message=str(error))
        return 1
    except KeyboardInterrupt:
        _fail_running(args)
        emit("error", message="stopped")
        return 130
    return 0


def _fail_running(args) -> None:
    """Leave no step marked as running after an error or a stop."""
    run_id = getattr(args, "run", None)
    if not run_id:
        return
    try:
        run = Run.open(run_id)
    except (MapgenError, OSError, ValueError):
        return
    busy = {"planning", "generating", "checking", "running"}
    changed = False
    for step in [run.data["layout"], run.data["inventory"], *run.data["batches"]]:
        if step.get("status") in busy:
            step["status"] = "review" if step.get("versions") else ("failed" if step is run.data["inventory"]
                                                                     else "pending")
            changed = True
    if changed:
        run.save()


def _stop(*_):
    raise KeyboardInterrupt


if __name__ == "__main__":
    import signal
    signal.signal(signal.SIGTERM, _stop)
    sys.exit(main())
