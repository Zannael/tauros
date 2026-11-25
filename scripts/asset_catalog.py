#!/usr/bin/env python3
"""Local searchable catalog of reusable Gen 3 environment graphics (GFX-005).

Stdlib only (Python 3.10+). Everything it reads and writes lives under
``$TAUROS_DATA/assets/catalog`` (default ``~/.local/share/tauros/assets/catalog``),
never inside this repository. External files are treated as data: nothing from
them is executed and archive members are extracted only as images/text.

Commands: fetch, build, check, search, show, measure, view, upgrade (see gfx_upgrades.py).  See README.
"""
from __future__ import annotations

import argparse
import array
import hashlib
import json
import os
import re
import struct
import subprocess
import sys
import time
import unicodedata
import zlib
from pathlib import Path

TOOL_VERSION = 1
PNG_MAGIC = b"\x89PNG\r\n\x1a\n"

# Pinned engine constants (roadmap/docs/master.md, RES-004: expansion 3796c722,
# include/fieldmap.h). Used for static compatibility checks only.
ENGINE = {
    "name": "pokeemerald-expansion",
    "revision": "3796c72237d2ebf0361c6f4a1b4be551d8858904",
    "tiles_in_primary": 512,
    "tiles_total": 1024,
    "metatiles_in_primary": 512,
    "metatiles_total": 1024,
    "palettes_in_primary": 6,
    "palettes_total": 13,
    "tiles_per_metatile": 8,
    "metatile_bytes": 16,
    "attribute_bytes": 2,
}


def data_root() -> Path:
    base = os.environ.get("TAUROS_DATA") or str(Path.home() / ".local/share/tauros")
    return Path(base) / "assets" / "catalog"


# --------------------------------------------------------------------------- PNG


class PngError(ValueError):
    pass


class Png:
    """Decoded PNG: ``rows`` are unfiltered scanlines (bytes, no filter byte)."""

    def __init__(self, width, height, depth, ctype, rows, plte, trns, interlaced):
        self.width, self.height, self.depth, self.ctype = width, height, depth, ctype
        self.rows, self.plte, self.trns, self.interlaced = rows, plte, trns, interlaced

    @property
    def indexed(self) -> bool:
        return self.ctype == 3

    def rgba_rows(self):
        """Yield each row as 8-bit RGBA bytes (4 * width)."""
        w, d, ct = self.width, self.depth, self.ctype
        if ct == 3:
            pal = list(self.plte) + [(0, 0, 0)] * (256 - len(self.plte))
            alpha = list(self.trns or b"") + [255] * 256
            tabs = [bytes(p[c] for p in pal) for c in range(3)] + [bytes(alpha[:256])]
        for raw in self.rows:
            if ct == 6:
                out = raw[:w * 4] if d == 8 else bytes(raw[0::2])
            elif ct == 2:
                src = raw if d == 8 else bytes(raw[0::2])
                out = bytearray(b"\xff" * (4 * w))
                out[0::4], out[1::4], out[2::4] = src[0::3], src[1::3], src[2::3]
            elif ct in (0, 3, 4):
                if ct == 4:
                    src = raw if d == 8 else bytes(raw[0::2])
                    idx, al = src[0::2], src[1::2]
                else:
                    idx = _unpack_samples(raw, d, w)
                    al = None
                out = bytearray(4 * w)
                if ct == 3:
                    for c in range(4):
                        out[c::4] = idx.translate(tabs[c])
                else:
                    if d < 8:
                        scale = 255 // ((1 << d) - 1)
                        idx = bytes(min(255, v * scale) for v in idx)
                    elif d == 16:
                        idx = bytes(raw[0::2])
                    out[0::4] = out[1::4] = out[2::4] = idx[:w]
                    out[3::4] = al[:w] if al else b"\xff" * w
            else:
                raise PngError(f"unsupported colour type {ct}")
            yield bytes(out)

    def index_rows(self):
        if not self.indexed:
            raise PngError("not an indexed PNG")
        for raw in self.rows:
            yield _unpack_samples(raw, self.depth, self.width)


def _unpack_samples(raw: bytes, depth: int, width: int) -> bytes:
    if depth == 8:
        return bytes(raw[:width])
    if depth == 16:
        return bytes(raw[0::2][:width])
    per = 8 // depth
    mask = (1 << depth) - 1
    out = bytearray(len(raw) * per)
    for k in range(per):
        shift = 8 - depth * (k + 1)
        out[k::per] = raw.translate(bytes((v >> shift) & mask for v in range(256)))
    return bytes(out[:width])


def _unfilter(data: bytes, h: int, stride: int, bpp: int):
    rows, prev = [], bytearray(stride)
    pos = 0
    for _ in range(h):
        ft = data[pos]
        cur = bytearray(data[pos + 1:pos + 1 + stride])
        pos += 1 + stride
        if len(cur) != stride:
            raise PngError("truncated image data")
        if ft == 1:
            for i in range(bpp, stride):
                cur[i] = (cur[i] + cur[i - bpp]) & 255
        elif ft == 2:
            cur = bytearray((a + b) & 255 for a, b in zip(cur, prev))
        elif ft == 3:
            for i in range(stride):
                left = cur[i - bpp] if i >= bpp else 0
                cur[i] = (cur[i] + ((left + prev[i]) >> 1)) & 255
        elif ft == 4:
            for i in range(stride):
                a = cur[i - bpp] if i >= bpp else 0
                b = prev[i]
                c = prev[i - bpp] if i >= bpp else 0
                p = a + b - c
                pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                pr = a if pa <= pb and pa <= pc else (b if pb <= pc else c)
                cur[i] = (cur[i] + pr) & 255
        elif ft != 0:
            raise PngError(f"bad filter {ft}")
        rows.append(bytes(cur))
        prev = cur
    return rows


def read_png(path) -> Png:
    data = Path(path).read_bytes()
    if data[:8] != PNG_MAGIC:
        raise PngError("not a PNG (bad signature)")
    pos, idat, plte, trns, ihdr = 8, [], [], None, None
    while pos + 8 <= len(data):
        ln, typ = struct.unpack(">I4s", data[pos:pos + 8])
        body = data[pos + 8:pos + 8 + ln]
        pos += 12 + ln
        if typ == b"IHDR":
            ihdr = struct.unpack(">IIBBBBB", body)
        elif typ == b"PLTE":
            plte = [tuple(body[i:i + 3]) for i in range(0, len(body) - 2, 3)]
        elif typ == b"tRNS":
            trns = body
        elif typ == b"IDAT":
            idat.append(body)
        elif typ == b"IEND":
            break
    if not ihdr:
        raise PngError("missing IHDR")
    w, h, depth, ctype, _, _, il = ihdr
    if il:
        raise PngError("interlaced PNG not supported")
    chans = {0: 1, 2: 3, 3: 1, 4: 2, 6: 4}.get(ctype)
    if chans is None:
        raise PngError(f"bad colour type {ctype}")
    stride = (w * chans * depth + 7) // 8
    bpp = max(1, chans * depth // 8)
    try:
        raw = zlib.decompress(b"".join(idat))
    except zlib.error as exc:
        raise PngError(f"corrupt image data: {exc}") from exc
    return Png(w, h, depth, ctype, _unfilter(raw, h, stride, bpp), plte, trns, bool(il))


def png_size(path):
    with open(path, "rb") as f:
        head = f.read(33)
    if head[:8] != PNG_MAGIC:
        raise PngError("not a PNG (bad signature)")
    w, h, depth, ctype, _, _, il = struct.unpack(">IIBBBBB", head[16:29])
    return w, h, depth, ctype, il


def write_png(path, width, height, rgba_rows):
    """Write an 8-bit RGBA PNG from rows of 4*width bytes."""
    raw = b"".join(b"\x00" + bytes(r) for r in rgba_rows)

    def chunk(t, b):
        return struct.pack(">I", len(b)) + t + b + struct.pack(">I", zlib.crc32(t + b))

    out = PNG_MAGIC + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0))
    out += chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b"")
    Path(path).parent.mkdir(parents=True, exist_ok=True)
    Path(path).write_bytes(out)


def crop_rows(rows, x, y, w, h):
    return [r[x * 4:(x + w) * 4] for r in rows[y:y + h]]


def scale_rows(rows, factor_num, factor_den=1):
    """Nearest-neighbour resize by factor_num/factor_den (both ints)."""
    h = len(rows)
    w = len(rows[0]) // 4 if h else 0
    nw, nh = max(1, w * factor_num // factor_den), max(1, h * factor_num // factor_den)
    xs = [min(w - 1, i * factor_den // factor_num) for i in range(nw)]
    out, last_y, last = [], None, None
    for j in range(nh):
        sy = min(h - 1, j * factor_den // factor_num)
        if sy != last_y:
            src = rows[sy]
            last = b"".join(src[x * 4:x * 4 + 4] for x in xs)
            last_y = sy
        out.append(last)
    return out


def flatten(rows, bg=(128, 128, 128)):
    """Composite RGBA rows on an opaque background colour."""
    out = []
    for r in rows:
        b = bytearray(r)
        for i in range(0, len(b), 4):
            a = b[i + 3]
            if a != 255:
                for c in range(3):
                    b[i + c] = (b[i + c] * a + bg[c] * (255 - a)) // 255
                b[i + 3] = 255
        out.append(bytes(b))
    return out


def fit_preview(rows, max_side=256, min_side=96):
    """Scale RGBA rows to fit max_side (down) or reach min_side (integer up)."""
    h = len(rows)
    w = len(rows[0]) // 4
    longest = max(w, h)
    if longest > max_side:
        return scale_rows(rows, max_side, longest)
    if longest < min_side:
        k = max(1, min(8, min_side // longest))
        return scale_rows(rows, k)
    return rows


# ----------------------------------------------------------------- image analysis

_DIGITS = {
    "0": "111101101101111", "1": "010110010010111", "2": "111001111100111",
    "3": "111001111001111", "4": "101101111001001", "5": "111100111001111",
    "6": "111100111101111", "7": "111001001001001", "8": "111101111101111",
    "9": "111101111001111",
}


def detect_upscale(rows, w, h, max_factor=4):
    """Largest f in 1..max_factor such that the RGBA image is an exact f-times
    nearest-neighbour enlargement (every f*f block uniform)."""
    best = 1
    for f in range(2, max_factor + 1):
        if w % f or h % f:
            continue
        ok = True
        for y in range(0, h, f):
            r0 = rows[y]
            for k in range(1, f):
                if rows[y + k] != r0:
                    ok = False
                    break
            if not ok:
                break
            px = [r0[i:i + 4] for i in range(0, w * 4, 4)]
            for x in range(0, w, f):
                if any(px[x + k] != px[x] for k in range(1, f)):
                    ok = False
                    break
            if not ok:
                break
        if ok:
            best = f
    return best


def opaque_cells(rows, w, h, cell):
    """Boolean grid (list of lists) of cells containing any pixel with alpha > 0."""
    cw, ch = (w + cell - 1) // cell, (h + cell - 1) // cell
    grid = [[False] * cw for _ in range(ch)]
    for cy in range(ch):
        for y in range(cy * cell, min(h, cy * cell + cell)):
            row = rows[y]
            alphas = row[3::4]
            for cx in range(cw):
                if not grid[cy][cx] and any(alphas[cx * cell:(cx + 1) * cell]):
                    grid[cy][cx] = True
    return grid


def segment(rows, w, h, cell, bridge=1):
    """Cell-aligned connected regions of opaque content as (x, y, w, h) pixels.

    Cells within ``bridge`` empty cells of each other are merged. Meaning is NOT
    inferred; regions are only proposals that a reviewer names."""
    grid = opaque_cells(rows, w, h, cell)
    ch, cw = len(grid), len(grid[0]) if grid else 0
    seen = [[False] * cw for _ in range(ch)]
    out = []
    for sy in range(ch):
        for sx in range(cw):
            if not grid[sy][sx] or seen[sy][sx]:
                continue
            stack, minx, miny, maxx, maxy = [(sx, sy)], sx, sy, sx, sy
            seen[sy][sx] = True
            while stack:
                cx, cy = stack.pop()
                minx, maxx, miny, maxy = min(minx, cx), max(maxx, cx), min(miny, cy), max(maxy, cy)
                for dy in range(-bridge - 1, bridge + 2):
                    for dx in range(-bridge - 1, bridge + 2):
                        nx, ny = cx + dx, cy + dy
                        if 0 <= nx < cw and 0 <= ny < ch and grid[ny][nx] and not seen[ny][nx]:
                            seen[ny][nx] = True
                            stack.append((nx, ny))
            out.append((minx * cell, miny * cell, (maxx - minx + 1) * cell, (maxy - miny + 1) * cell))
    out.sort(key=lambda r: (r[1], r[0]))
    return out


def draw_label(buf, w, x, y, text, color=(255, 255, 0), px=2):
    for n, ch in enumerate(text):
        bits = _DIGITS.get(ch)
        if not bits:
            continue
        for i, b in enumerate(bits):
            if b == "1":
                for dy in range(px):
                    for dx in range(px):
                        xx, yy = x + (n * 4 + i % 3) * px + dx, y + (i // 3) * px + dy
                        if 0 <= yy < len(buf) and 0 <= xx < w:
                            buf[yy][xx * 4:xx * 4 + 4] = bytes(color) + b"\xff"


def cmd_view(args):
    """Dev helper: crop (+optional numbered region overlay) to a PNG for review."""
    p = read_png(args.file)
    rows = flatten(list(p.rgba_rows()), (150, 150, 150))
    x, y = args.x, args.y
    w = args.w or p.width - x
    h = args.h or p.height - y
    k = args.scale
    crop = [bytearray(r) for r in scale_rows(crop_rows(rows, x, y, w, h), k)]
    W = w * k
    regs = []
    if args.segment:
        cell = args.cell
        regs = [r for r in segment(list(p.rgba_rows()), p.width, p.height, cell, args.bridge)]
        for n, (rx, ry, rw, rh) in enumerate(regs):
            if rx < x or ry < y or rx + rw > x + w or ry + rh > y + h:
                continue
            col = [(255, 0, 0), (0, 160, 255), (0, 200, 0), (255, 140, 0)][n % 4]
            X0, Y0, X1, Y1 = (rx - x) * k, (ry - y) * k, (rx - x + rw) * k - 1, (ry - y + rh) * k - 1
            for xx in range(X0, X1 + 1):
                crop[Y0][xx * 4:xx * 4 + 3] = bytes(col)
                crop[Y1][xx * 4:xx * 4 + 3] = bytes(col)
            for yy in range(Y0, Y1 + 1):
                crop[yy][X0 * 4:X0 * 4 + 3] = bytes(col)
                crop[yy][X1 * 4:X1 * 4 + 3] = bytes(col)
            draw_label(crop, W, X0 + 2, Y0 + 2, str(n), col if False else (255, 255, 0))
            print(n, rx, ry, rw, rh)
    elif args.grid:
        g = args.grid
        for gy in range(h):
            if (y + gy) % g == 0:
                for xx in range(W):
                    crop[gy * k][xx * 4:xx * 4 + 3] = b"\x00\xc8\xff"
        for gx in range(w):
            if (x + gx) % g == 0:
                for yy in range(len(crop)):
                    crop[yy][gx * k * 4:gx * k * 4 + 3] = b"\x00\xc8\xff"
    write_png(args.out, W, len(crop), crop)
    print(f"wrote {args.out} {W}x{len(crop)}")


# ------------------------------------------------------------------ vocabulary

KINDS = ("component", "composite", "tileset")  # single piece / assembled object / whole tileset or sheet
FORMS = ("decomp-tileset", "decomp-tileset-patch", "raw-sheet", "loose-image", "autotile", "demo-image")
STATUSES = ("verified", "conversion-required", "unverified", "not-applicable")
ROLES = ("ground", "path", "edge", "tree", "vegetation", "roof", "building", "door", "fence",
         "water", "prop", "structure", "demo-map", "tileset")
BIOMES = ("snow", "cave", "desert", "water", "beach", "forest", "volcano", "ruins", "urban",
          "temperate", "space", "unknown")
SETTINGS = ("exterior", "interior", "unknown")
CONFIDENCE = ("high", "medium", "low")
ARCHETYPES = {
    "town": ("ground", "edge", "tree", "roof", "building", "door"),
    "route": ("ground", "edge", "tree", "path"),
}
TAGS = frozenset("""
snowy ice cliff cave-entrance rock border boulder bush decoration grass-tuft igloo house awning shop pine tree
round-tree large pond bench fence sign mailbox stairs wall structure roof wall window door animated autotile path
ground flowers pokecenter mart gym vehicle dome tower stone statue banner shed grass-edge water shore demo
temple ruins desert pyramid volcano lava forest swamp beach underwater reef coral sewer space meteor interior
urban gate cafe office dojo lab library garden hedge brick cactus palm furniture primary secondary patch
""".split())
NAME_BIOME = (  # keyword (word in name) -> biome
    ("snowpoint", "snow"), ("snow", "snow"), ("winter", "snow"), ("frozen", "snow"), ("ice", "snow"),
    ("cave", "cave"), ("cavern", "cave"), ("grotto", "cave"), ("moon", "cave"),
    ("desert", "desert"), ("pyramid", "desert"), ("egypt", "desert"), ("cacti", "desert"), ("cactus", "desert"),
    ("underwater", "water"), ("reef", "water"), ("coral", "water"), ("sea", "water"), ("aquarium", "water"),
    ("beach", "beach"), ("palm", "beach"),
    ("forest", "forest"), ("woods", "forest"), ("shady", "forest"), ("ivy", "forest"), ("swamp", "forest"),
    ("volcano", "volcano"), ("lava", "volcano"),
    ("ruins", "ruins"), ("temple", "ruins"), ("altar", "ruins"),
    ("city", "urban"), ("town", "urban"), ("village", "urban"), ("skyscraper", "urban"), ("silph", "urban"),
    ("space", "space"), ("meteor", "space"),
)
NAME_TAG = (
    ("ruins", "ruins"), ("temple", "temple"), ("desert", "desert"), ("pyramid", "pyramid"), ("volcano", "volcano"),
    ("lava", "lava"), ("forest", "forest"), ("swamp", "swamp"), ("beach", "beach"), ("underwater", "underwater"),
    ("reef", "reef"), ("coral", "coral"), ("sewer", "sewer"), ("space", "space"), ("meteor", "meteor"),
    ("interior", "interior"), ("indoors", "interior"), ("cafe", "cafe"), ("office", "office"), ("dojo", "dojo"),
    ("lab", "lab"), ("gate", "gate"), ("garden", "garden"), ("hedge", "hedge"), ("brick", "brick"),
    ("cactus", "cactus"), ("cacti", "cactus"), ("palm", "palm"), ("snow", "snowy"), ("winter", "snowy"),
    ("fence", "fence"), ("flower", "flowers"), ("rock", "rock"), ("bush", "bush"), ("tree", "tree"),
    ("primary", "primary"), ("secondary", "secondary"), ("cave", "cave-entrance"), ("cliff", "cliff"),
    ("water", "water"), ("path", "path"), ("stairs", "stairs"), ("bench", "bench"), ("sign", "sign"),
)
NAME_ROLE = (  # only for single-object files whose whole name is the object
    ("tree", "tree"), ("palm", "tree"), ("bush", "vegetation"), ("cactus", "vegetation"), ("cacti", "vegetation"),
    ("flower", "vegetation"), ("fence", "fence"), ("rock", "prop"), ("sign", "prop"), ("ball", "prop"),
    ("aquarium", "prop"), ("path", "path"), ("stone", "path"),
)


def slug(text: str) -> str:
    text = unicodedata.normalize("NFKD", text).encode("ascii", "ignore").decode()
    s = re.sub(r"[^a-z0-9]+", "-", text.lower()).strip("-")
    return s or "x"


def words(text: str):
    return re.findall(r"[a-z0-9]+", text.lower())


def name_biome(name: str):
    ws = words(name)
    for kw, biome in NAME_BIOME:
        if kw in ws or (len(kw) > 3 and any(w.startswith(kw) for w in ws)):
            return biome, f"name-keyword:{kw}"
    return "unknown", "no-keyword"


def name_tags(name: str):
    ws = words(name)
    out = []
    for kw, tag in NAME_TAG:
        if (kw in ws or (len(kw) > 3 and any(w.startswith(kw) for w in ws))) and tag in TAGS and tag not in out:
            out.append(tag)
    return out


def name_setting(name: str) -> str:
    ws = words(name)
    return "interior" if any(w in ws for w in ("interior", "interiors", "indoors")) else "unknown"


# --------------------------------------------------------------- image analysis


def distinct_colours(rows, w):
    seen = set()
    semi = False
    for row in rows:
        a = array.array("I")
        a.frombytes(row[:w * 4])
        seen.update(set(a))
    cols = set()
    for v in seen:
        alpha = (v >> 24) & 255 if sys.byteorder == "little" else v & 255
        if alpha == 0:
            continue
        if alpha != 255:
            semi = True
        r, g, b = (v & 255, (v >> 8) & 255, (v >> 16) & 255) if sys.byteorder == "little" else \
            ((v >> 24) & 255, (v >> 16) & 255, (v >> 8) & 255)
        cols.add((r, g, b))
    return cols, semi


def tile_stats(rows, w, h, up=1, max_native_px=1_500_000):
    """Per-8x8 native tile statistics (unique tiles, max opaque colours)."""
    nw, nh = w // up, h // up
    if nw * nh > max_native_px:
        return {"analysed": False, "reason": "image too large for tile statistics"}
    native = []
    for y in range(0, nh * up, up):
        a = array.array("I")
        a.frombytes(rows[y][:w * 4])
        native.append(a[::up])
    uniq, maxc, tiles = set(), 0, 0
    le = sys.byteorder == "little"
    for ty in range(nh // 8):
        for tx in range(nw // 8):
            key = tuple(bytes(native[ty * 8 + k][tx * 8:tx * 8 + 8]) for k in range(8))
            tiles += 1
            if key in uniq:
                continue
            uniq.add(key)
            cols = set()
            for chunk in key:
                arr = array.array("I")
                arr.frombytes(chunk)
                cols.update(v for v in arr if (((v >> 24) & 255) if le else (v & 255)) != 0)
            maxc = max(maxc, len(cols))
    return {"analysed": True, "grid_tiles": tiles, "unique_tiles": len(uniq), "max_opaque_colours_per_tile": maxc,
            "grid_aligned": (nw % 8 == 0 and nh % 8 == 0)}


def png_facts(png: Png, rows):
    """Measured facts about a decoded PNG (rows = list of RGBA rows)."""
    cols, semi = distinct_colours(rows, png.width)
    up = detect_upscale(rows, png.width, png.height) if png.width * png.height <= 8_000_000 else 1
    ts = tile_stats(rows, png.width, png.height, up)
    mode = {3: "indexed", 6: "rgba", 2: "rgb", 0: "gray", 4: "gray-alpha"}[png.ctype]
    return {
        "mode": mode, "bit_depth": png.depth, "size": [png.width, png.height], "native_scale": up,
        "opaque_colours": len(cols), "semi_transparent": semi,
        "rgb555_exact": all(c % 8 == 0 for col in cols for c in col) if cols else None,
        "plte_entries": len(png.plte) if png.indexed else None, "tiles": ts,
    }


def raw_sheet_compat(facts, form_note="raw RGBA/indexed sheet without metatile data"):
    """Static compatibility with the pinned engine for a loose image (never 'verified')."""
    checks, reasons = {}, []
    ok_idx = facts["mode"] == "indexed" and facts["opaque_colours"] <= 16
    checks["indexed_png_le16_colours"] = "pass" if ok_idx else "fail"
    if not ok_idx:
        reasons.append(f"{facts['mode']} image with {facts['opaque_colours']} opaque colours; "
                       "needs indexing into 16-colour palettes")
    t = facts["tiles"]
    if t.get("analysed"):
        m = t["max_opaque_colours_per_tile"]
        checks["max_15_opaque_colours_per_8x8_tile"] = "pass" if m <= 15 else "fail"
        if m > 15:
            reasons.append(f"an 8x8 tile uses {m} opaque colours (limit 15 + transparent)")
        checks["grid_8px"] = "pass" if t["grid_aligned"] else "fail"
        if not t["grid_aligned"]:
            reasons.append("dimensions are not multiples of 8 at native scale")
    else:
        checks["max_15_opaque_colours_per_8x8_tile"] = "not-checked"
        checks["grid_8px"] = "not-checked"
    checks["metatile_data"] = "fail"
    reasons.append(form_note + "; metatiles, attributes and palette assignment must be built")
    checks["metatile_layers"] = "not-checked"
    status = "conversion-required"
    return {"status": status, "scope": COMPAT_SCOPE, "checks": checks, "reasons": reasons}


COMPAT_SCOPE = (f"static checks against {ENGINE['name']} {ENGINE['revision'][:8]} "
                "(format, palettes, metatile layers); no build, import or in-game test")


# ------------------------------------------------------- decomp tileset analysis


def parse_pal(path):
    try:
        lines = Path(path).read_text(errors="replace").split()
    except OSError:
        return None
    if len(lines) < 3 or lines[0] != "JASC-PAL" or lines[1] != "0100":
        return None
    try:
        n = int(lines[2])
        vals = list(map(int, lines[3:3 + 3 * n]))
    except ValueError:
        return None
    if len(vals) != 3 * n:
        return None
    return [tuple(vals[i:i + 3]) for i in range(0, len(vals), 3)]


def decomp_tileset_facts(d: Path, name: str):
    """Static facts and compatibility checks for a decomp-style tileset folder."""
    files = {p.name.lower(): p for p in d.iterdir() if p.is_file()}
    pal_dir = next((p for p in d.iterdir() if p.is_dir() and p.name.lower() == "palettes"), None)
    pals = {}
    for p in ([q for q in pal_dir.iterdir()] if pal_dir else []) + [q for q in d.iterdir() if q.suffix == ".pal"]:
        m = re.fullmatch(r"(\d+)\.pal", p.name)
        if m:
            pals[int(m.group(1))] = p
    role = "primary" if "primary" in name.lower() else ("secondary" if "secondary" in name.lower() else None)
    checks, reasons, facts = {}, [], {"role_basis": "folder name" if role else "none"}
    facts["tileset_role"] = role

    # tiles image
    tiles_png = files.get("tiles.png") or next(
        (q for n, q in sorted(files.items())
         if n.endswith(".png") and "tiles" in n and "example" not in n and "demo" not in n), None)
    tile_count = None
    if tiles_png:
        try:
            png = read_png(tiles_png)
            idx_ok = png.indexed and png.depth in (4, 8)
            used = set()
            for r in png.index_rows():
                used.update(r)
            tile_count = (png.width // 8) * (png.height // 8)
            facts["tiles_image"] = {"size": [png.width, png.height], "mode": "indexed" if png.indexed else "other",
                                    "bit_depth": png.depth, "indices_used": len(used), "tiles": tile_count}
            checks["tiles_png_indexed"] = "pass" if idx_ok and len(used) <= 16 else "fail"
            checks["tiles_png_width_128"] = "pass" if png.width == 128 else "fail"
            if not (idx_ok and len(used) <= 16):
                reasons.append("tiles.png is not an indexed image using at most 16 indices")
            if png.width != 128:
                reasons.append(f"tiles.png width {png.width} (decomp tile images are 128 px wide)")
        except (PngError, ValueError, OSError) as exc:
            checks["tiles_png_indexed"] = "fail"
            reasons.append(f"tiles.png unreadable: {exc}")
    elif "tiles.4bpp" in files:
        tile_count = files["tiles.4bpp"].stat().st_size // 32
        facts["tiles_image"] = {"source": "tiles.4bpp", "tiles": tile_count}
        checks["tiles_png_indexed"] = "not-checked"
    else:
        checks["tiles_png_indexed"] = "fail"
        reasons.append("no tiles.png or tiles.4bpp")
    if tile_count is not None:
        limit = ENGINE["tiles_in_primary"] if role == "primary" else (
            ENGINE["tiles_total"] - ENGINE["tiles_in_primary"] if role == "secondary" else None)
        if limit is None:
            checks["tile_budget"] = "not-checked"
        else:
            checks["tile_budget"] = "pass" if tile_count <= limit else "fail"
            if tile_count > limit:
                reasons.append(f"{tile_count} tiles exceed the {role} budget of {limit}")

    # palettes
    bad = [i for i, p in pals.items() if parse_pal(p) is None or len(parse_pal(p)) != 16]
    facts["palette_files"] = sorted(pals)
    if not pals:
        checks["palettes_jasc_16"] = "fail"
        reasons.append("no NN.pal palette files")
    else:
        checks["palettes_jasc_16"] = "fail" if bad else "pass"
        if bad:
            reasons.append(f"palette files not valid 16-colour JASC: {sorted(bad)}")
        extra = [i for i in pals if i >= ENGINE["palettes_total"]]
        if extra:  # decomp tilesets ship 16 palette files; only metatile references matter
            facts["palette_files_beyond_engine_count"] = sorted(extra)
        cols = [c for i in pals if (pp := parse_pal(pals[i])) for c in pp]
        facts["palette_rgb555_exact"] = all(v % 8 == 0 for c in cols for v in c) if cols else None

    # metatiles and layers
    mt, at = files.get("metatiles.bin"), files.get("metatile_attributes.bin")
    layers, count = None, None
    if mt:
        msize = mt.stat().st_size
        if at:
            asize = at.stat().st_size
            fits = []
            for lay, per in ((2, 16), (3, 24)):
                if msize % per == 0:
                    n = msize // per
                    if n and asize % n == 0 and asize // n in (2, 4):
                        fits.append((lay, n, asize // n))
            if len(fits) == 1:
                layers, count, attr_bytes = fits[0]
                facts["attribute_bytes"] = attr_bytes
                checks["engine_attribute_width_2"] = "pass" if attr_bytes == ENGINE["attribute_bytes"] else "fail"
                if attr_bytes != ENGINE["attribute_bytes"]:
                    reasons.append(f"attributes are {attr_bytes} bytes per metatile; pinned engine reads 2")
            checks["metatile_layers_determined"] = "pass" if layers else "fail"
            if not layers:
                reasons.append("metatiles.bin/metatile_attributes.bin sizes fit no known layout")
        else:
            checks["metatile_layers_determined"] = "not-checked"
            reasons.append("metatile_attributes.bin missing: layer count cannot be determined from sizes alone")
        facts["metatile_bytes"] = msize
        if layers:
            facts["metatile_layers"] = layers
            facts["metatiles"] = count
            checks["engine_two_layer_metatiles"] = "pass" if layers == 2 else "fail"
            if layers == 3:
                reasons.append("package uses triple-layer metatiles; pinned engine reads two layers "
                               "(needs triple-layer support or re-layering)")
            lim = ENGINE["metatiles_in_primary"] if role == "primary" else (
                ENGINE["metatiles_total"] - ENGINE["metatiles_in_primary"] if role == "secondary" else None)
            if lim is not None:
                checks["metatile_budget"] = "pass" if count <= lim else "fail"
                if count > lim:
                    reasons.append(f"{count} metatiles exceed the {role} budget of {lim}")
            if layers == 2:
                data = mt.read_bytes()
                entries = struct.unpack(f"<{len(data) // 2}H", data)
                tmax = max((e & 0x3FF) for e in entries)
                pmax = max((e >> 12) for e in entries)
                facts["max_tile_index"], facts["max_palette_index"] = tmax, pmax
                if tile_count is not None and role == "secondary":
                    hi = [e & 0x3FF for e in entries if (e & 0x3FF) >= ENGINE["tiles_in_primary"]]
                    ok = not hi or max(hi) < ENGINE["tiles_in_primary"] + tile_count
                    checks["tile_indices_in_range"] = "pass" if ok else "fail"
                    if not ok:
                        reasons.append("metatiles reference secondary tiles beyond the supplied tile image")
                elif tile_count is not None and role == "primary":
                    ok = tmax < ENGINE["tiles_in_primary"]
                    checks["tile_indices_in_range"] = "pass" if ok else "fail"
                    if not ok:
                        reasons.append("primary metatiles reference tile indices >= 512")
                checks["metatile_palette_indices_lt_13"] = "pass" if pmax < ENGINE["palettes_total"] else "fail"
                if pmax >= ENGINE["palettes_total"]:
                    reasons.append(f"metatile palette index {pmax} exceeds the engine's palette count")
    else:
        checks["metatile_layers_determined"] = "fail"
        reasons.append("no metatiles.bin")
    if role is None:
        checks["primary_or_secondary_known"] = "not-checked"
        reasons.append("primary/secondary role not stated in the folder name; budgets not checked")
    anim = [p.name for p in d.iterdir() if p.is_dir() and p.name.lower().startswith("anim")]
    facts["animations"] = bool(anim)
    if anim:
        checks["animations"] = "not-checked"
        reasons.append("ships tile animations; animation tables were not inspected")

    fails = [k for k, v in checks.items() if v == "fail"]
    unchecked = [k for k, v in checks.items() if v == "not-checked"]
    status = "conversion-required" if fails else ("unverified" if unchecked else "verified")
    return facts, {"status": status, "scope": COMPAT_SCOPE, "checks": checks, "reasons": reasons}


def read_text_head(path: Path, limit=400) -> str:
    try:
        t = path.read_text(errors="replace")
    except OSError:
        return ""
    t = re.sub(r"!\[[^\]]*\]\([^)]*\)", "", t)
    return re.sub(r"\s+", " ", t).strip()[:limit]


def find_one(d: Path, *names):
    low = {p.name.lower(): p for p in d.iterdir() if p.is_file()}
    for n in names:
        if n.lower() in low:
            return low[n.lower()]
    return None


# ------------------------------------------------------------------------ build

BUILD_VERSION = 1
SKIP_DIRS = ("anim", "anims", "door_anims", "porytiles", "porytiles_source_files", "for porytiles")


def file_sha256(path: Path) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def load_json(path: Path, default=None):
    try:
        return json.loads(Path(path).read_text())
    except FileNotFoundError:
        return default


def ekat_credits(base: Path):
    f = next(iter(base.rglob("Credits.txt")), None)
    out = {}
    if not f:
        return out
    for block in re.split(r"\n\s*\n", f.read_text(errors="replace")):
        lines = [l.strip() for l in block.strip().splitlines() if l.strip()]
        if len(lines) >= 2 and re.match(r"\d+\.", lines[1]):
            names = [re.sub(r"^\d+\.\s*", "", l) for l in lines[1:] if re.match(r"\d+\.", l)]
            out[slug(lines[0])] = "; ".join(names)
    return out


class Builder:
    def __init__(self, root: Path):
        self.root = root
        self.sources = load_json(root / "sources.json", [])
        self.overrides = load_json(root / "annotations/overrides.json", {})
        self.cache_path = root / ".cache/facts.json"
        self.cache = load_json(self.cache_path, {})
        self.items, self.errors, self.used = [], [], set()
        self.preview_dir = root / "previews"

    # -- helpers
    def rel(self, p: Path) -> str:
        return str(p.relative_to(self.root))

    def preview(self, item_id: str, rows, w, h, mode="full"):
        name = item_id.replace("/", "__") + ".png"
        rows = flatten(rows)
        if mode == "top-crop":
            rows = rows[:min(len(rows), 2 * w)]
        rows = fit_preview(rows, 256)
        write_png(self.preview_dir / name, len(rows[0]) // 4, len(rows), rows)
        return "previews/" + name

    def facts_for(self, path: Path, rows_needed: bool):
        sha = file_sha256(path)
        key = f"{sha}:{BUILD_VERSION}"
        png = read_png(path) if (rows_needed or key not in self.cache) else None
        rows = list(png.rgba_rows()) if png else None
        if key not in self.cache:
            self.cache[key] = png_facts(png, rows)
        return self.cache[key], png, rows, sha

    def add(self, item):
        self.items.append(item)

    def unique(self, iid):
        base, n = iid, 2
        while iid in self.used:
            iid, n = f"{base}-{n}", n + 1
        self.used.add(iid)
        return iid

    def finalize_override(self, item):
        ov = self.overrides.get(item["id"])
        if not ov:
            return
        for k in ("contains", "biome", "setting"):
            if k in ov:
                item[k] = ov[k]
        item["tags"] = sorted(set(item["tags"]) | set(ov.get("tags", [])))
        item["description"] = re.sub(r"Theme(?: from name)?: [^.]*\.", f"Theme: {item['biome']} (README and visual review).",
                                     item["description"])
        if ov.get("desc_extra"):
            item["description"] = item["description"].rstrip(".") + ". " + ov["desc_extra"]
        item["meta_basis"]["override"] = ov["basis"]
        item["confidence"] = "medium"

    # -- per source
    def run(self):
        self.preview_dir.mkdir(parents=True, exist_ok=True)
        for src in self.sources:
            base = self.root / src["local_dir"]
            if not base.is_dir():
                self.errors.append(f"{src['id']}: missing {src['local_dir']} (run fetch)")
                continue
            ann = load_json(self.root / f"annotations/{src['id']}.json", {"files": {}})
            credits = ekat_credits(base) if src["id"] == "ekat" else {}
            for sc in src["scan"]:
                self.scan(src, base, sc, ann.get("files", {}), credits)
        self.cache_path.parent.mkdir(exist_ok=True)
        self.cache_path.write_text(json.dumps(self.cache))
        return self.items

    def item_id(self, src, scan_root: Path, p: Path, scan):
        rel = p.relative_to(scan_root)
        parts = [slug(x.rsplit(".", 1)[0] if x.lower().endswith(".png") else x) for x in rel.parts]
        if scan.get("prefix"):
            parts.insert(0, scan["prefix"])
        return src["id"] + "/" + "/".join(parts)

    def scan(self, src, base, scan, annotations, credits):
        sroot = base / scan["path"]
        top_done = set()
        for dirpath, dirnames, filenames in os.walk(sroot):
            d = Path(dirpath)
            dirnames[:] = sorted(x for x in dirnames if x.lower() not in SKIP_DIRS)
            if scan["mode"] == "tileset-dirs" and d == sroot:
                continue
            has_mt = any(f.lower() == "metatiles.bin" for f in filenames)
            if has_mt:
                self.tileset_dir(src, sroot, d, scan)
                dirnames[:] = []
                top_done.add(d.relative_to(sroot).parts[0] if d != sroot else "")
                continue
            if scan["mode"] == "tileset-dirs":
                continue
            for fn in sorted(filenames):
                if fn.lower().endswith(".png"):
                    self.image_item(src, base, sroot, d / fn, scan, annotations, credits)
        if scan["mode"] == "tileset-dirs":
            for d in sorted(x for x in sroot.iterdir() if x.is_dir()):
                if d.name not in top_done:
                    self.fallback_dir(src, sroot, d, scan)

    # -- tileset folder (decomp layout)
    def tileset_dir(self, src, sroot, d, scan):
        title = d.name
        readme = find_one(d, "README.md", "readme.md")
        rhead = read_text_head(readme, 160) if readme else ""
        facts, compat = decomp_tileset_facts(d, title + " " + rhead)
        iid = self.unique(self.item_id(src, sroot, d, scan))
        biome, basis = name_biome(title)
        cred = find_one(d, "credits.md", "Credits.md")
        creditsrc = read_text_head(cred, 220) if cred else rhead
        pals = len(facts["palette_files"])
        bits = [f"{facts.get('metatiles', '?')} metatiles" if facts.get("metatiles") else None,
                f"{facts['tiles_image']['tiles']} tiles" if facts.get("tiles_image") else None, f"{pals} palettes"]
        layer = f", {facts['metatile_layers']}-layer metatiles" if facts.get("metatile_layers") else ""
        desc = (f"{title}: {facts['tileset_role'] or 'decomp'} tileset folder ({', '.join(b for b in bits if b)}{layer})"
                f"{', animated' if facts['animations'] else ''}. Theme from name: {biome if biome != 'unknown' else 'not stated'}.")
        pic = find_one(d, "tiles.png") or next((q for q in sorted(d.glob("*.png"))
                                                if "tiles" in q.name.lower() and "example" not in q.name.lower()), None)
        item = dict(
            id=iid, source=src["id"], kind="tileset", form="decomp-tileset" if "tiles_image" in facts else "decomp-tileset-patch",
            role="tileset", contains=[], title=title, description=desc, biome=biome, setting=name_setting(title),
            style=src["style"], tags=sorted(set(name_tags(title) + (["patch"] if "patch" in desc else []))),
            path=self.rel(d), region=None, size=facts.get("tiles_image", {}).get("size"), facts=facts,
            compat=compat, credits=creditsrc or src["authors"], confidence="medium",
            meta_basis={"biome": basis, "role": "folder name" if facts["tileset_role"] else "none"})
        if pic:
            try:
                f, png, rows, _ = self.facts_for(pic, True)
                item["preview"] = self.preview(iid, rows, png.width, png.height, "top-crop" if png.height > 3 * png.width else "full")
                item["palette"] = {k: f[k] for k in ("mode", "opaque_colours", "rgb555_exact")}
                if facts.get("palette_rgb555_exact") is not None:
                    item["palette"]["palette_files"] = pals
                    item["palette"]["palette_files_rgb555_exact"] = facts["palette_rgb555_exact"]
            except (PngError, OSError) as exc:
                item["preview"] = None
                item["facts"]["preview_error"] = str(exc)
        else:
            item["preview"] = None
        self.finalize_override(item)
        self.add(item)

    def fallback_dir(self, src, sroot, d, scan):
        pngs = [q for q in sorted(d.rglob("*.png")) if not any(x in q.parts for x in SKIP_DIRS)]
        title = d.name
        iid = self.unique(self.item_id(src, sroot, d, scan))
        biome, basis = name_biome(title)
        pic = next((q for q in pngs if q.name.lower() in ("tiles.png", "example.png")), pngs[0] if pngs else None)
        item = dict(
            id=iid, source=src["id"], kind="tileset", form="raw-sheet", role="tileset", contains=[], title=title,
            description=(f"{title}: folder with a non-standard layout ({len(pngs)} PNG files, no top-level metatiles.bin); "
                         "not analysed structurally."),
            biome=biome, setting=name_setting(title), style=src["style"], tags=sorted(name_tags(title)),
            path=self.rel(d), region=None, size=None, facts={"png_files": len(pngs)},
            compat={"status": "unverified", "scope": COMPAT_SCOPE, "checks": {"layout": "not-checked"},
                    "reasons": ["non-standard folder layout (nested project or Porytiles sources)"]},
            credits=read_text_head(find_one(d, "README.md", "readme.md") or d, 200) if find_one(d, "README.md", "readme.md") else src["authors"],
            confidence="low", meta_basis={"biome": basis})
        item["preview"] = None
        if pic:
            try:
                f, png, rows, _ = self.facts_for(pic, True)
                item["preview"] = self.preview(iid, rows, png.width, png.height, "top-crop" if png.height > 3 * png.width else "full")
                item["size"] = [png.width, png.height]
                item["palette"] = {k: f[k] for k in ("mode", "opaque_colours", "rgb555_exact")}
            except (PngError, OSError):
                pass
        self.finalize_override(item)
        self.add(item)

    # -- loose images and annotated sheets
    def image_item(self, src, base, sroot, path, scan, annotations, credits):
        relkey = str(path.relative_to(base))
        ann = annotations.get(relkey, {})
        regions = ann.get("regions", [])
        whole_only = len(regions) == 1 and regions[0]["box"] is None
        try:
            facts, png, rows, sha = self.facts_for(path, True)
        except (PngError, OSError, zlib.error) as exc:
            self.errors.append(f"{relkey}: unreadable image ({exc}); recorded as skipped")
            self.add(dict(id=self.unique(self.item_id(src, sroot, path, scan)), source=src["id"], kind="tileset", form="loose-image",
                          role="tileset", contains=[], title=path.stem, description=f"{path.stem}: image could not be decoded ({exc}).",
                          biome="unknown", setting="unknown", style=src["style"], tags=[], path=self.rel(path), region=None,
                          size=None, facts={"decode_error": str(exc)}, preview=None,
                          compat={"status": "unverified", "scope": COMPAT_SCOPE, "checks": {"decodable": "fail"}, "reasons": [str(exc)]},
                          credits=src["authors"], confidence="low", meta_basis={}))
            return
        w, h = png.width, png.height
        iid = self.unique(self.item_id(src, sroot, path, scan))
        stem = path.stem
        parent = path.parent.name
        biome, basis = name_biome(stem)
        tags = name_tags(stem)
        autotile = "autotile" in str(path.relative_to(sroot)).lower()
        form = "autotile" if autotile else ("loose-image" if w < 64 and h < 64 else "raw-sheet")
        credit = credits.get(slug(stem)) or src["authors"]
        if src["id"] == "taar":
            rd = find_one(path.parent, "README.md")
            credit = read_text_head(rd, 200) if rd else src["authors"]
        if not whole_only:
            kind, role = ("component", next((r for k, r in NAME_ROLE if k in words(stem)), "tileset")) \
                if scan["mode"] == "images" and src["id"] == "taar" and parent in ("Oomer", "KyuZee", "yoshord", "decoration") \
                else ("tileset", "tileset")
            if kind == "tileset":
                theme = biome if biome != "unknown" else "not stated in the file name"
                desc = (f"{stem}: {form.replace('-', ' ')} {w}x{h}px, {facts['mode']} with {facts['opaque_colours']} opaque colours"
                        f"{', enlarged %dx' % facts['native_scale'] if facts['native_scale'] > 1 else ''}. Theme: {theme}.")
            else:
                desc = f"{stem.replace('_', ' ')} ({role}); object type from file name only."
            sheet_ti = dict(
                id=iid, source=src["id"], kind=kind, form="loose-image" if kind == "component" else form, role=role,
                contains=[], title=stem, description=desc, biome=biome, setting=name_setting(stem), style=src["style"],
                tags=sorted(set(tags)), path=self.rel(path), region=None, size=[w, h], palette=self.pal_summary(facts),
                facts=facts, compat=raw_sheet_compat(facts), credits=credit,
                confidence="low" if biome == "unknown" else "medium",
                meta_basis={"biome": basis, "role": "file name" if kind == "component" else "none"}, sha256=sha)
            sheet_ti["preview"] = self.preview(iid, rows, w, h, "top-crop" if h > 3 * w else "full")
            self.finalize_override(sheet_ti)
            self.add(sheet_ti)
        for r in regions:
            self.region_item(src, path, iid, w, h, rows, facts, r, credit, sha, whole_only, ann, relkey)

    def pal_summary(self, facts):
        return {"mode": facts["mode"], "opaque_colours": facts["opaque_colours"], "semi_transparent": facts["semi_transparent"],
                "rgb555_exact": facts["rgb555_exact"],
                "max_opaque_colours_per_8x8_tile": facts["tiles"].get("max_opaque_colours_per_tile")}

    def region_item(self, src, path, file_id, w, h, rows, facts, r, credit, sha, whole, ann, relkey):
        box = r["box"] or [0, 0, w, h]
        x, y, bw, bh = box
        iid = f"{file_id}/{r['key']}" if not whole else file_id
        if x < 0 or y < 0 or bw <= 0 or bh <= 0 or x + bw > w or y + bh > h:
            self.errors.append(f"{iid}: region {box} outside image {w}x{h}")
            return
        crop = crop_rows(rows, x, y, bw, bh)
        cols, semi = distinct_colours(crop, bw)
        if not cols:
            self.errors.append(f"{iid}: region {box} is fully transparent")
            return
        up = facts["native_scale"]
        rf = {"mode": facts["mode"], "bit_depth": facts["bit_depth"], "size": [bw, bh], "native_scale": up,
              "opaque_colours": len(cols), "semi_transparent": semi,
              "rgb555_exact": all(c % 8 == 0 for col in cols for c in col),
              "plte_entries": facts["plte_entries"], "tiles": tile_stats(crop, bw, bh, up)}
        autotile = "autotile" in r["tags"]
        biome = r.get("biome", "unknown")
        if r["role"] == "demo-map":
            compat = {"status": "not-applicable", "scope": COMPAT_SCOPE, "checks": {},
                      "reasons": ["example scene image; not a tile source"]}
        else:
            compat = raw_sheet_compat(rf, "raw RGBA sheet region" if not autotile else "RPG Maker-style autotile image")
        item = dict(
            id=iid, source=src["id"], kind=r["kind"], form="demo-image" if r["role"] == "demo-map" else ("autotile" if autotile else "raw-sheet"),
            role=r["role"], contains=r.get("contains", []), title=r["key"].replace("-", " "), description=r["desc"] + ".",
            biome=biome, setting=r.get("setting", "unknown"), style=src["style"], tags=sorted(set(r["tags"])),
            path=self.rel(path), region=None if r["box"] is None else {"x": x, "y": y, "w": bw, "h": bh},
            size=[bw, bh], palette=self.pal_summary(rf), facts=rf, compat=compat, credits=credit,
            confidence=r["confidence"], meta_basis={"annotation": ann.get("reviewed_by", "annotation file"),
                                                    "reviewed_on": ann.get("reviewed_on")},
            sha256=sha)
        if r.get("note"):
            item["note"] = r["note"]
        item["preview"] = self.preview(iid, crop, bw, bh, "top-crop" if bh > 3 * bw else "full")
        self.add(item)


# ------------------------------------------------------------------------ groups


def build_groups(root: Path, items):
    by_id = {i["id"]: i for i in items}
    raw = load_json(root / "annotations/groups.json", [])
    out, errors = [], []
    for g in raw:
        missing_ids = [m for m in g["members"] if m not in by_id]
        if missing_ids:
            errors.append(f"{g['id']}: unknown members {missing_ids}")
            continue
        members = [by_id[m] for m in g["members"]]
        req = ARCHETYPES[g["archetype"]]
        coverage = {}
        for role in ("ground", "path", "edge", "tree", "roof", "building", "door", "fence", "water", "prop", "vegetation"):
            if any(m["role"] == role for m in members):
                coverage[role] = "component" if any(m["role"] == role and m["kind"] == "component" for m in members) else "composite"
            elif any(role in m.get("contains", []) for m in members):
                coverage[role] = "inside-composite-only"
            else:
                coverage[role] = "missing"
        missing = [r for r in req if coverage[r] == "missing"]
        weak = [r for r in req if coverage[r] == "inside-composite-only"]
        statuses = sorted({m["compat"]["status"] for m in members if m["compat"]["status"] != "not-applicable"})
        rec = dict(id=g["id"], title=g["title"], description=g["description"], archetype=g["archetype"], biome=g["biome"],
                   style=g["style"], sources=g["source_ids"], members=[{"id": m["id"], "role": m["role"], "kind": m["kind"]} for m in members],
                   coverage=coverage, required_roles=list(req), missing_roles=missing, inside_composite_only=weak,
                   compat_statuses=statuses, emerald_ready=("yes (static checks)" if statuses == ["verified"] else "no"),
                   notes=g.get("notes", []))
        rec["preview"] = group_preview(root, rec, by_id)
        out.append(rec)
    return out, errors


def group_preview(root: Path, rec, by_id):
    tiles = []
    for m in rec["members"]:
        pv = by_id[m["id"]].get("preview")
        if not pv or m["kind"] == "tileset" and len(rec["members"]) > 1:
            continue
        try:
            png = read_png(root / pv)
        except (PngError, OSError):
            continue
        rows = list(png.rgba_rows())
        h = len(rows)
        tiles.append(scale_rows(rows, 72, h) if h > 72 else rows)
    if not tiles:
        return None
    width, x, y, rowh, placed = 768, 0, 0, 0, []
    for t in tiles:
        tw, th = len(t[0]) // 4, len(t)
        if x + tw > width:
            x, y, rowh = 0, y + rowh + 4, 0
        placed.append((x, y, t))
        x += tw + 4
        rowh = max(rowh, th)
    height = y + rowh
    canvas = [bytearray(b"\x40\x40\x40\xff" * width) for _ in range(height)]
    for px, py, t in placed:
        for j, r in enumerate(t):
            canvas[py + j][px * 4:px * 4 + len(r)] = r
    name = "previews/" + rec["id"].replace("/", "__") + ".png"
    write_png(root / name, width, height, canvas)
    return name


# ------------------------------------------------------------------------ output


def cmd_build(args):
    root = data_root()
    t0 = time.time()
    b = Builder(root)
    items = b.run()
    ids = [i["id"] for i in items]
    dup = sorted({i for i in ids if ids.count(i) > 1})
    if dup:
        b.errors.append(f"duplicate ids: {dup[:5]}")
    groups, gerr = build_groups(root, items)
    b.errors += gerr
    # sources: add content digests
    srcs = load_json(root / "sources.json", [])
    for s in srcs:
        s["item_count"] = sum(1 for i in items if i["source"] == s["id"])
    items.sort(key=lambda i: i["id"])
    (root / "items.jsonl").write_text("".join(json.dumps(i, ensure_ascii=False, sort_keys=True) + "\n" for i in items))
    (root / "groups.jsonl").write_text("".join(json.dumps(g, ensure_ascii=False, sort_keys=True) + "\n" for g in groups))
    manifest = {"tool_version": TOOL_VERSION, "build_version": BUILD_VERSION, "engine": ENGINE, "items": len(items),
                "groups": len(groups), "by_source": {s["id"]: s["item_count"] for s in srcs},
                "by_status": {st: sum(1 for i in items if i["compat"]["status"] == st) for st in STATUSES},
                "by_kind": {k: sum(1 for i in items if i["kind"] == k) for k in KINDS},
                "build_seconds": round(time.time() - t0, 1), "errors": b.errors}
    (root / "manifest.json").write_text(json.dumps(manifest, indent=1))
    print(json.dumps(manifest, indent=1))
    return 1 if [e for e in b.errors if "unreadable" not in e] else 0


# ------------------------------------------------------------------------ search

STOP = {"a", "an", "the", "of", "for", "with", "and", "in", "on", "to", "di", "il", "lo", "la", "le", "i", "gli", "un",
        "una", "e", "con", "per", "da", "del", "della", "dei", "delle", "set", "tileset", "tiles", "asset", "assets"}
LEXICON = {  # Italian and loose English -> catalog vocabulary
    "citta": "city", "paese": "town", "paesino": "town", "villaggio": "village", "borgo": "village",
    "innevata": "snow", "innevato": "snow", "innevate": "snow", "nevoso": "snow", "nevosa": "snow", "neve": "snow",
    "snowy": "snow", "winter": "snow", "inverno": "snow", "invernale": "snow", "ghiaccio": "ice", "ghiacciato": "ice",
    "frozen": "ice", "albero": "tree", "alberi": "tree", "trees": "tree", "pino": "pine", "pini": "pine",
    "tetto": "roof", "tetti": "roof", "roofs": "roof", "porta": "door", "porte": "door", "doors": "door",
    "edificio": "building", "edifici": "building", "buildings": "building", "casa": "house", "case": "house",
    "houses": "house", "negozio": "shop", "bordo": "edge", "bordi": "edge", "edges": "edge", "border": "edge",
    "borders": "edge", "cliff": "edge", "cliffs": "edge", "ledge": "edge", "terreno": "ground", "suolo": "ground",
    "grotta": "cave", "caverna": "cave", "deserto": "desert", "foresta": "forest", "bosco": "forest",
    "sentiero": "path", "percorso": "path", "strada": "path", "acqua": "water", "staccionata": "fence",
    "recinto": "fence", "vulcano": "volcano", "spiaggia": "beach", "sott'acqua": "underwater", "fiori": "flowers",
    "cespuglio": "bush", "bushes": "bush", "interno": "interior", "interni": "interior", "indoor": "interior",
    "city": "town", "village": "town", "cittadina": "town", "mare": "water", "sea": "water",
}
QUERY_BIOMES = {"snow": "snow", "ice": "snow", "cave": "cave", "desert": "desert", "water": "water", "underwater": "water",
                "beach": "beach", "forest": "forest", "volcano": "volcano", "lava": "volcano", "ruins": "ruins",
                "space": "space", "temperate": "temperate", "johto": "temperate"}
ROLE_ALIASES = {"house": "building", "building": "building", "shop": "building", "town": None, "edge": "edge",
                "tree": "tree", "pine": "tree", "roof": "roof", "door": "door", "ground": "ground", "path": "path",
                "fence": "fence", "bush": "vegetation", "flowers": "vegetation", "water": "water"}


def norm_query(q: str):
    q = unicodedata.normalize("NFKD", q.lower()).encode("ascii", "ignore").decode()
    out = []
    for w in re.findall(r"[a-z0-9']+", q):
        if w in STOP:
            continue
        raw = w
        w = LEXICON.get(LEXICON.get(w, w), LEXICON.get(w, w))
        if w.endswith("s") and len(w) > 4 and w[:-1] in {"tree", "roof", "door", "house", "flower", "building", "path"}:
            w = w[:-1]
        for t in (w, raw):  # keep the literal word too: titles say "village" even when "town" is the concept
            if t not in out and t not in STOP:
                out.append(t)
    return out


def interpret(q: str):
    toks = norm_query(q)
    biomes = sorted({QUERY_BIOMES[t] for t in toks if t in QUERY_BIOMES})
    arche = "town" if "town" in toks else ("route" if any(t in ("route",) for t in toks) else None)
    roles = sorted({ROLE_ALIASES[t] for t in toks if ROLE_ALIASES.get(t)})
    return {"tokens": toks, "biomes": biomes, "archetype": arche, "roles": roles}


def load_catalog(root: Path):
    items = [json.loads(l) for l in (root / "items.jsonl").read_text().splitlines() if l]
    groups = [json.loads(l) for l in (root / "groups.jsonl").read_text().splitlines() if l]
    return items, groups


def _item_words(i):
    return set(words(i["title"])), set(words(i["description"]))


def score_item(i, qi):
    toks, biomes = qi["tokens"], qi["biomes"]
    if biomes and i["biome"] not in biomes:
        if i["biome"] != "unknown":
            return 0.0
        base_pen = 0.4
    else:
        base_pen = 1.0
    tw, dw = _item_words(i)
    s = 0.0
    matched = 0
    roles = set([i["role"]] + i.get("contains", []))
    for t in toks:
        hit = 0.0
        if t in i["tags"]:
            hit = max(hit, 2.0)
        if t == i["role"] or ROLE_ALIASES.get(t) == i["role"]:
            hit = max(hit, 3.0)
        elif ROLE_ALIASES.get(t) in roles:
            hit = max(hit, 1.5)
        if t in QUERY_BIOMES and QUERY_BIOMES[t] == i["biome"]:
            hit = max(hit, 3.0)
        if t in tw:
            hit = max(hit, 2.0)
        elif t in dw:
            hit = max(hit, 1.0)
        if hit:
            matched += 1
        s += hit
    if not matched:
        return 0.0
    if qi["roles"] and i["role"] not in qi["roles"] and not set(qi["roles"]) & set(i.get("contains", [])):
        s *= 0.5  # asked for a role this item is not
    if i["kind"] == "tileset" and qi["roles"]:
        s *= 0.6
    if i["role"] == "demo-map":
        s *= 0.3
    conf = {"high": 1.0, "medium": 0.95, "low": 0.85}[i["confidence"]]
    return s * base_pen * conf


def _missing_for(g, archetype):
    return [r for r in ARCHETYPES[archetype] if g["coverage"].get(r, "missing") == "missing"]


def score_group(g, qi):
    s = 0.0
    if qi["biomes"] and g["biome"] in qi["biomes"]:
        s += 5
    elif qi["biomes"]:
        return 0.0
    if qi["archetype"] and g["archetype"] == qi["archetype"]:
        s += 3
    text = set(words(g["title"] + " " + g["description"]))
    s += sum(1 for t in qi["tokens"] if t in text)
    s += sum(0.5 for t in qi["tokens"] if ROLE_ALIASES.get(t) and g["coverage"].get(ROLE_ALIASES[t], "missing") != "missing")
    if qi["archetype"]:
        s -= 1.0 * len(_missing_for(g, qi["archetype"]))
    return s if s > 0 else 0.0


def fmt_member(i):
    r = i["region"]
    where = f"{r['w']}x{r['h']}@{r['x']},{r['y']}" if r else (f"{i['size'][0]}x{i['size'][1]}" if i.get("size") else "?")
    flag = "" if i["confidence"] != "low" else "?"
    return f"{i['id']}{flag} {where}"


def search(root: Path, query: str, groups_n=2, items_n=5, members_per_role=3):
    items, groups = load_catalog(root)
    by_id = {i["id"]: i for i in items}
    qi = interpret(query)
    gs = sorted(((score_group(g, qi), g) for g in groups), key=lambda x: -x[0])
    gs = [(s, g) for s, g in gs if s >= 4]
    gs = [(s, g) for s, g in gs if s >= 0.6 * gs[0][0]][:groups_n]
    role_query = bool(qi["roles"]) and not qi["archetype"]
    if role_query:  # a specific piece was asked for: list pieces, not whole kits
        gs = []
    grouped = {m["id"] for _, g in gs for m in g["members"]}
    cand = sorted(((score_item(i, qi), i) for i in items if i["id"] not in grouped), key=lambda x: (-x[0], x[1]["id"]))
    cand = [(s, i) for s, i in cand if s >= 2.0]
    per_source, picked = {}, []
    for s, i in cand:  # diversity: at most 3 items per source
        if per_source.get(i["source"], 0) < 3:
            per_source[i["source"]] = per_source.get(i["source"], 0) + 1
            picked.append((s, i))
    cand = picked[:items_n]
    kits = sorted({g["id"] for g in groups for _, i in cand for m in g["members"] if m["id"] == i["id"]}) if role_query else []
    return {"query": query, "kits": kits, "interpreted": {k: qi[k] for k in ("biomes", "archetype", "roles")},
            "groups": [(s, g) for s, g in gs], "items": cand, "by_id": by_id}


def render(res, root: Path, members_per_role=3):
    L = []
    qi = res["interpreted"]
    L.append(f"Q: {res['query']} -> biome={','.join(qi['biomes']) or '-'} kit={qi['archetype'] or '-'} role={','.join(qi['roles']) or '-'}")
    by_id = res["by_id"]
    if not res["groups"] and not res["items"]:
        L.append("No relevant entry. Nothing in the catalog matches these terms; do not assume a fit.")
    for s, g in res["groups"]:
        L.append(f"GROUP {g['id']} [{g['biome']}, {g['style']}] {g['description']}")
        byrole = {}
        for m in g["members"]:
            byrole.setdefault(m["role"], []).append(by_id[m["id"]])
        for role in list(g["required_roles"]) + [r for r in byrole if r not in g["required_roles"]]:
            mem = byrole.get(role, [])
            if not mem:
                continue
            shown = [fmt_member(i) for i in mem[:members_per_role]]
            more = f" (+{len(mem) - members_per_role} more)" if len(mem) > members_per_role else ""
            L.append(f"  {role}: " + "; ".join(shown) + more)
        arche = qi["archetype"] or g["archetype"]
        req = ARCHETYPES[arche]
        cov = []
        for r in req:
            c = g["coverage"][r]
            cov.append(f"{r}={'OK' if c in ('component', 'composite') else ('only-in-composites' if c == 'inside-composite-only' else 'MISSING')}")
        L.append(f"  coverage for {arche}: " + " ".join(cov))
        miss = _missing_for(g, arche)
        if miss:
            L.append("  MISSING: " + ", ".join(miss) + " (not in this group; do not assume)")
        weak = [r for r in req if g["coverage"][r] == "inside-composite-only"]
        if weak:
            L.append("  not separable: " + ", ".join(weak) + " exist only inside composite objects")
        nometa = [m for m in g["members"] if by_id[m["id"]]["compat"]["checks"].get("metatile_data") == "fail"]
        if nometa:
            L.append(f"  NOT PROVIDED: metatile data (collision, behaviours, layers) and palette assignment for {len(nometa)}/{len(g['members'])} members")
        L.append(f"  Emerald-ready: {g['emerald_ready']} ({', '.join(g['compat_statuses'])}); preview {g['preview']}")
        for n in g["notes"][:2]:
            L.append(f"  note: {n}")
    for s, i in res["items"]:
        L.append(f"ITEM {fmt_member(i)} [{i['kind']}/{i['role']}, {i['biome']}, {i['compat']['status']}] {i['description']}")
    if res.get("kits"):
        L.append("In kits: " + ", ".join(res["kits"]))
    if res["groups"] or res["items"]:
        L.append("(? = low-confidence region; `show <id>` for path, preview and checks)")
    return "\n".join(L)


def cmd_search(args):
    root = data_root()
    res = search(root, " ".join(args.query), args.groups, args.items)
    if args.json:
        out = {"query": res["query"], "interpreted": res["interpreted"],
               "groups": [dict(id=g["id"], score=round(s, 2), missing=g["missing_roles"], not_separable=g["inside_composite_only"],
                               members=g["members"], emerald_ready=g["emerald_ready"]) for s, g in res["groups"]],
               "items": [dict(id=i["id"], score=round(s, 2), path=i["path"], region=i["region"], status=i["compat"]["status"])
                         for s, i in res["items"]]}
        print(json.dumps(out, ensure_ascii=False))
    else:
        print(render(res, root))
    return 0


def cmd_show(args):
    root = data_root()
    items, groups = load_catalog(root)
    for rec in items + groups:
        if rec["id"] == args.id:
            rec = dict(rec)
            if "path" in rec:
                rec["absolute_path"] = str(root / rec["path"])
            if rec.get("preview"):
                rec["preview"] = str(root / rec["preview"])
            if not args.full:
                rec.pop("facts", None)
            print(json.dumps(rec, indent=1, ensure_ascii=False))
            return 0
    print(f"no such id: {args.id}", file=sys.stderr)
    return 1


# ------------------------------------------------------------------------- check

ID_RE = re.compile(r"^[a-z0-9]+(-[a-z0-9]+)*(/[a-z0-9]+(-[a-z0-9]+)*)+$")
REQUIRED = ("id", "source", "kind", "form", "role", "contains", "title", "description", "biome", "setting", "style", "tags",
            "path", "region", "size", "compat", "credits", "confidence", "preview", "meta_basis")
READY_CLAIM = re.compile(r"emerald[- ]ready|ready for emerald|drop-in", re.I)
KEY_CHECKS = ("tiles_png_indexed", "palettes_jasc_16", "engine_two_layer_metatiles", "engine_attribute_width_2")


def git_head(path: Path):
    try:
        return subprocess.run(["git", "-C", str(path), "rev-parse", "HEAD"], capture_output=True, text=True,
                              timeout=30).stdout.strip()
    except (OSError, subprocess.SubprocessError):
        return ""


def validate(root: Path, deep=False):
    errs, warns = [], []
    srcs = load_json(root / "sources.json", None)
    if not srcs:
        return ["sources.json missing"], []
    for s in srcs:
        for k in ("id", "name", "page_url", "fetch", "retrieved", "version", "authors", "terms", "credit_requirement"):
            if not s.get(k):
                errs.append(f"source {s.get('id')}: missing {k}")
        f = s.get("fetch", {})
        if f.get("kind") == "git":
            head = git_head(root / "sources" / f["dir"])
            if head != f["revision"]:
                errs.append(f"source {s['id']}: checkout at {head[:8] or 'none'}, pinned {f['revision'][:8]}")
        elif f.get("kind") in ("files", "manual"):
            for ent in f["files"]:
                fp = root / "sources" / f["extract"] / ent["name"]
                if not fp.is_file() or file_sha256(fp) != ent["sha256"]:
                    errs.append(f"source {s['id']}: {ent['name']} missing or differs from pin")
        elif f.get("kind") == "archive":
            arc = root / f["file"]
            if not arc.is_file():
                errs.append(f"source {s['id']}: archive {f['file']} missing")
            elif file_sha256(arc) != f["sha256"]:
                errs.append(f"source {s['id']}: archive sha256 differs from pin")
        if not (root / s["local_dir"]).is_dir():
            errs.append(f"source {s['id']}: {s['local_dir']} missing")
    try:
        items, groups = load_catalog(root)
    except FileNotFoundError:
        return errs + ["items.jsonl/groups.jsonl missing (run build)"], warns
    seen, src_ids = set(), {s["id"] for s in srcs}
    png_size_cache, deep_rows = {}, {}
    for i in items:
        iid = i.get("id", "?")
        miss = [k for k in REQUIRED if k not in i]
        if miss:
            errs.append(f"{iid}: missing fields {miss}")
            continue
        if iid in seen:
            errs.append(f"{iid}: duplicate id")
        seen.add(iid)
        if not ID_RE.match(iid) or not iid.startswith(i["source"] + "/"):
            errs.append(f"{iid}: malformed id")
        if i["source"] not in src_ids:
            errs.append(f"{iid}: unknown source")
        for key, vocab in (("kind", KINDS), ("form", FORMS), ("role", ROLES), ("biome", BIOMES), ("setting", SETTINGS),
                           ("confidence", CONFIDENCE)):
            if i[key] not in vocab:
                errs.append(f"{iid}: {key}={i[key]!r} not in vocabulary")
        bad = [t for t in i["tags"] if t not in TAGS]
        if bad:
            errs.append(f"{iid}: tags outside vocabulary {bad}")
        if len(i["description"]) > 420:
            warns.append(f"{iid}: description longer than 420 chars")
        c = i["compat"]
        if c["status"] not in STATUSES or not c.get("scope"):
            errs.append(f"{iid}: bad compat block")
        if c["status"] == "verified":
            ch = c["checks"]
            if any(v != "pass" for v in ch.values()) or not all(k in ch for k in KEY_CHECKS[:3]):
                errs.append(f"{iid}: verified without passing format, palette and layer checks")
        elif READY_CLAIM.search(i["description"]):
            errs.append(f"{iid}: description claims readiness without verified status")
        full = root / i["path"]
        if not full.exists():
            errs.append(f"{iid}: path {i['path']} missing")
            continue
        if i["region"]:
            r = i["region"]
            sz = png_size_cache.get(i["path"])
            if sz is None:
                try:
                    sz = png_size_cache[i["path"]] = png_size(full)[:2]
                except (PngError, OSError):
                    errs.append(f"{iid}: region on unreadable image")
                    continue
            if r["x"] < 0 or r["y"] < 0 or r["w"] <= 0 or r["h"] <= 0 or r["x"] + r["w"] > sz[0] or r["y"] + r["h"] > sz[1]:
                errs.append(f"{iid}: region outside image {sz}")
            if deep:
                if deep_rows.get("path") != i["path"]:  # decode each image once (items are sorted by id)
                    deep_rows.update(path=i["path"], rows=list(read_png(full).rgba_rows()))
                rows = deep_rows["rows"]
                if not distinct_colours(crop_rows(rows, r["x"], r["y"], r["w"], r["h"]), r["w"])[0]:
                    errs.append(f"{iid}: region fully transparent")
        if i["preview"]:
            pv = root / i["preview"]
            try:
                png_size(pv)
            except (PngError, OSError):
                errs.append(f"{iid}: preview {i['preview']} missing or unreadable")
        elif i["kind"] != "tileset":
            errs.append(f"{iid}: component/composite without preview")
    by_id = {i["id"]: i for i in items}
    for g in groups:
        for m in g["members"]:
            if m["id"] not in by_id:
                errs.append(f"{g['id']}: unknown member {m['id']}")
        calc = [r for r in g["required_roles"]
                if not any(by_id[m["id"]]["role"] == r or r in by_id[m["id"]].get("contains", []) for m in g["members"] if m["id"] in by_id)]
        if calc != g["missing_roles"]:
            errs.append(f"{g['id']}: stored missing roles {g['missing_roles']} != recomputed {calc}")
        if g.get("preview") and not (root / g["preview"]).is_file():
            errs.append(f"{g['id']}: preview missing")
    mf = load_json(root / "manifest.json", {})
    if mf.get("items") != len(items) or mf.get("groups") != len(groups):
        errs.append("manifest counts differ from items.jsonl/groups.jsonl (rebuild)")
    for pv in (root / "previews").glob("*.png") if (root / "previews").is_dir() else []:
        stem = pv.stem.replace("__", "/")
        if stem not in by_id and stem not in {g["id"] for g in groups}:
            warns.append(f"orphan preview {pv.name}")
    return errs, warns


def cmd_check(args):
    errs, warns = validate(data_root(), deep=args.deep)
    for w in warns[:20]:
        print("warn:", w)
    for e in errs[:50]:
        print("error:", e)
    print(f"check: {len(errs)} errors, {len(warns)} warnings" + (" (deep)" if args.deep else ""))
    return 1 if errs else 0


# ----------------------------------------------------------------------- measure

SAMPLES = [  # (query, expected top group or None, predicate on listed items, expects_empty)
    ("città innevata", "grp/cinnayva-snow-town", lambda i: i["biome"] == "snow", False),
    ("snowy town", "grp/cinnayva-snow-town", lambda i: i["biome"] == "snow", False),
    ("snow roof house", None, lambda i: i["biome"] == "snow" and ("roof" in i["tags"] or i["role"] == "building"), False),
    ("pine trees in the snow", None, lambda i: i["biome"] == "snow" and i["role"] == "tree", False),
    ("porta", None, lambda i: i["role"] == "door" or "door" in i["contains"], False),
    ("edge for snowy cliffs", None, lambda i: i["biome"] == "snow" and i["role"] == "edge", False),
    ("johto town", "grp/alistair-johto-town", lambda i: i["style"].startswith("frlg"), False),
    ("desert village", None, lambda i: i["biome"] == "desert" or "desert" in i["tags"], False),
    ("volcano", None, lambda i: i["biome"] == "volcano" or "volcano" in i["tags"], False),
    ("underwater", None, lambda i: i["biome"] == "water", False),
    ("space station", None, lambda i: i["biome"] == "space", False),
    ("neon cyberpunk skyline", None, lambda i: False, True),
]


def cmd_measure(args):
    root = data_root()
    items, groups = load_catalog(root)
    rows, t_all = [], 0
    for q, exp_group, pred, expect_empty in SAMPLES:
        t0 = time.perf_counter()
        res = search(root, q)
        text = render(res, root)
        ms = (time.perf_counter() - t0) * 1000
        listed = [i for _, i in res["items"]]
        prec = (sum(1 for i in listed if pred(i)) / len(listed)) if listed else None
        grp_ok = None
        if exp_group:
            grp_ok = bool(res["groups"]) and res["groups"][0][1]["id"] == exp_group
        empty = not res["groups"] and not res["items"]
        rows.append({"query": q, "groups": len(res["groups"]), "items": len(listed), "bytes": len(text.encode()),
                     "words": len(text.split()), "lines": text.count("\n") + 1, "ms": round(ms, 1),
                     "item_precision": None if prec is None else round(prec, 2), "top_group_expected": exp_group,
                     "top_group_correct": grp_ok, "expected_empty": expect_empty, "returned_empty": empty})
    sizes = {"items.jsonl_bytes": (root / "items.jsonl").stat().st_size, "groups.jsonl_bytes": (root / "groups.jsonl").stat().st_size,
             "source_files": sum(1 for _ in (root / "sources").rglob("*") if _.is_file()),
             "source_image_files": sum(1 for _ in (root / "sources").rglob("*.png"))}
    report = {"date": time.strftime("%Y-%m-%d", time.gmtime()), "catalog": json.loads((root / "manifest.json").read_text())["items"],
              "method": "metadata-based relevance predicates (retrieval consistency, not independent ground truth); "
                        "bytes are UTF-8 characters of the rendered response; no token counts are claimed",
              "sizes": sizes, "samples": rows}
    (root / "reports").mkdir(exist_ok=True)
    (root / "reports/measure.json").write_text(json.dumps(report, indent=1))
    print(f"{'query':32} {'grp':>3} {'itm':>3} {'bytes':>6} {'words':>5} {'ms':>6} {'prec':>5} grp_ok empty(exp/got)")
    for r in rows:
        print(f"{r['query'][:32]:32} {r['groups']:>3} {r['items']:>3} {r['bytes']:>6} {r['words']:>5} {r['ms']:>6} "
              f"{str(r['item_precision']):>5} {str(r['top_group_correct']):>6} {r['expected_empty']}/{r['returned_empty']}")
    nb = [r["bytes"] for r in rows]
    print(f"response bytes: min {min(nb)} median {sorted(nb)[len(nb) // 2]} max {max(nb)}; "
          f"items.jsonl {sizes['items.jsonl_bytes']} bytes; {sizes['source_files']} source files ({sizes['source_image_files']} PNG)")
    return 0


# ------------------------------------------------------------------------- fetch

UA = "Mozilla/5.0 (X11; Linux x86_64) tauros-asset-catalog"


def _download(url: str, dest: Path):
    import urllib.request
    req = urllib.request.Request(url, headers={"User-Agent": UA})
    with urllib.request.urlopen(req, timeout=120) as r, open(dest, "wb") as out:
        total = 0
        while chunk := r.read(1 << 20):
            total += len(chunk)
            if total > 200 * 1024 * 1024:
                raise RuntimeError("download larger than 200 MB")
            out.write(chunk)


def _resolve(f):
    import urllib.request
    if f["resolver"] == "google-drive":
        return f"https://drive.usercontent.google.com/download?id={f['drive_id']}&export=download&confirm=t"
    if f["resolver"] == "mediafire":
        class NoRedirect(urllib.request.HTTPRedirectHandler):
            def redirect_request(self, *a, **k):
                return None
        op = urllib.request.build_opener(NoRedirect)
        try:
            op.open(urllib.request.Request(f["url"], headers={"User-Agent": UA}), timeout=60)
            raise RuntimeError("expected redirect from the resource download link")
        except urllib.error.HTTPError as e:
            page = e.headers.get("Location")
        html = urllib.request.urlopen(urllib.request.Request(page, headers={"User-Agent": UA}), timeout=60).read().decode("utf-8", "replace")
        m = re.search(r'https://download[0-9]*\.mediafire\.com/[^"\']+', html)
        if not m:
            raise RuntimeError("download link not found on the file page")
        return m.group(0).replace("%5C%27", "%27")
    raise RuntimeError(f"unknown resolver {f['resolver']}")


def _safe_extract(zpath: Path, dest: Path):
    import zipfile
    z = zipfile.ZipFile(zpath)
    total = 0
    for m in z.infolist():
        n = m.filename
        if n.startswith("/") or ".." in n.split("/"):
            raise RuntimeError(f"unsafe archive member {n}")
        total += m.file_size
    if total > 400 * 1024 * 1024:
        raise RuntimeError("archive expands beyond 400 MB")
    for m in z.infolist():
        if m.is_dir() or m.filename.lower().endswith((".png", ".txt", ".md")):
            z.extract(m, dest)  # other member types are ignored, never executed


def cmd_fetch(args):
    root = data_root()
    srcs = load_json(root / "sources.json", None)
    if not srcs:
        print(f"{root / 'sources.json'} missing", file=sys.stderr)
        return 1
    rc = 0
    for s in srcs:
        if args.source and s["id"] != args.source:
            continue
        f = s["fetch"]
        try:
            if f["kind"] == "git":
                d = root / "sources" / f["dir"]
                if not (d / ".git").exists():
                    d.mkdir(parents=True, exist_ok=True)
                    run = lambda *a: subprocess.run(["git", "-c", "core.hooksPath=/dev/null", "-C", str(d), *a], check=True)
                    run("init", "-q")
                    run("remote", "add", "origin", f["url"])
                    run("remote", "set-url", "--push", "origin", "DISABLED")
                    run("sparse-checkout", "set", *f["sparse"])
                    run("fetch", "-q", "--depth", "1", "--filter=blob:none", "origin", f["revision"])
                    run("checkout", "-q", "FETCH_HEAD")
                head = git_head(d)
                if head != f["revision"]:
                    raise RuntimeError(f"checkout at {head}, pinned {f['revision']}")
            elif f["kind"] == "manual":  # login-gated host: the user saves the files, we only verify the pins
                out = root / "sources" / f["extract"]
                bad = [e["name"] for e in f["files"] if not (out / e["name"]).is_file() or file_sha256(out / e["name"]) != e["sha256"]]
                if bad:
                    raise RuntimeError(f"save these files from {s['page_url']} into {out}: {bad}")
            elif f["kind"] == "files":
                out = root / "sources" / f["extract"]
                out.mkdir(parents=True, exist_ok=True)
                for ent in f["files"]:
                    dest = out / ent["name"]
                    if not dest.is_file() or file_sha256(dest) != ent["sha256"]:
                        tmp = dest.with_suffix(".part")
                        _download(ent["url"], tmp)
                        if file_sha256(tmp) != ent["sha256"]:
                            tmp.unlink()
                            raise RuntimeError(f"{ent['name']} does not match the pinned sha256 (upstream changed?)")
                        tmp.replace(dest)
            else:
                arc = root / f["file"]
                if not arc.is_file() or file_sha256(arc) != f["sha256"]:
                    arc.parent.mkdir(parents=True, exist_ok=True)
                    tmp = arc.with_suffix(arc.suffix + ".part")
                    _download(_resolve(f | {"url": f["url"]}), tmp)
                    if file_sha256(tmp) != f["sha256"]:
                        tmp.unlink()
                        raise RuntimeError("downloaded file does not match the pinned sha256 (upstream changed?)")
                    tmp.replace(arc)
                out = root / "sources" / f["extract"]
                if arc.suffix == ".zip":
                    _safe_extract(arc, out)
                else:
                    out.mkdir(parents=True, exist_ok=True)
                    (out / arc.name).write_bytes(arc.read_bytes())
            print(f"fetch {s['id']}: ok")
        except (RuntimeError, OSError, subprocess.SubprocessError) as exc:
            print(f"fetch {s['id']}: FAILED {exc}", file=sys.stderr)
            rc = 1
    return rc


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    sub = ap.add_subparsers(dest="cmd", required=True)
    sub.add_parser("build", help="regenerate items.jsonl, groups.jsonl, previews").set_defaults(fn=cmd_build)
    p = sub.add_parser("fetch", help="fetch pinned sources")
    p.add_argument("--source")
    p.set_defaults(fn=cmd_fetch)
    p = sub.add_parser("check", help="validate sources and catalog")
    p.add_argument("--deep", action="store_true", help="decode images to verify regions")
    p.set_defaults(fn=cmd_check)
    p = sub.add_parser("search", help="compact search")
    p.add_argument("query", nargs="+")
    p.add_argument("--groups", type=int, default=2)
    p.add_argument("--items", type=int, default=5)
    p.add_argument("--json", action="store_true")
    p.set_defaults(fn=cmd_search)
    p = sub.add_parser("show", help="print one item or group")
    p.add_argument("id")
    p.add_argument("--full", action="store_true")
    p.set_defaults(fn=cmd_show)
    sub.add_parser("measure", help="run sample queries and record sizes").set_defaults(fn=cmd_measure)
    p = sub.add_parser("view", help="dev: crop/overlay a PNG for review")
    p.add_argument("file")
    p.add_argument("--x", type=int, default=0)
    p.add_argument("--y", type=int, default=0)
    p.add_argument("--w", type=int, default=0)
    p.add_argument("--h", type=int, default=0)
    p.add_argument("--scale", type=int, default=1)
    p.add_argument("--grid", type=int, default=0)
    p.add_argument("--segment", action="store_true")
    p.add_argument("--cell", type=int, default=16)
    p.add_argument("--bridge", type=int, default=1)
    p.add_argument("--out", default="view.png")
    p.set_defaults(fn=cmd_view)
    p = sub.add_parser("upgrade", help="graphical upgrade entries (GFX-004): list, show, apply, revert, preview")
    p.add_argument("rest", nargs=argparse.REMAINDER)
    p.set_defaults(fn=lambda a: __import__("gfx_upgrades").main(a.rest))
    args = ap.parse_args(argv)
    return args.fn(args)


if __name__ == "__main__":
    sys.exit(main())
