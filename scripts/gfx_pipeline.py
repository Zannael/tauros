#!/usr/bin/env python3
"""Asset pipeline and validator for Gen 3 tilesets (GFX-001).

  validate <dir|png>        check Porytiles source layers (bottom/middle/top.png) or one PNG
  validate-compiled <dir>   check a compiled porytiles_bin directory against the engine budgets
  build                     source PNGs -> validate -> Porytiles (container, no network)
                            -> validate compiled output -> optional ROM build, all inside a
                            scratch copy of a project under $TAUROS_DATA

Every violation carries a code, a severity and file/pixel/tile/metatile coordinates. Errors
fail the run (exit 1); warnings and infos are reported only. Sprites and Porypal are not part
of this step. Animated tiles are reported as unsupported and never processed.
Python 3.10+, standard library only.
"""
from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import shutil
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))
import asset_catalog as png  # noqa: E402  (stdlib PNG reader)

ROOT = Path(__file__).resolve().parents[1]
LAYERS = ("bottom", "middle", "top")
# Pinned engine, include/fieldmap.h at expansion 1.16.4.
BUDGETS = {
    "primary": {"palettes": 6, "tiles": 512, "metatiles": 512},
    "secondary": {"palettes": 7, "tiles": 512, "metatiles": 512},
}
PALETTE_SLOTS_IN_PRIMARY = 6
PALETTE_SLOTS_TOTAL = 13
COLOURS_PER_PALETTE = 15            # 16 entries, index 0 is transparent
EXTRINSIC_TRANSPARENCY = (255, 0, 255)
MAX_PER_CODE = 100                  # stored instances per (code, file); counts stay exact

SEVERITIES = ("error", "warning", "info")
CODES = {
    "png.unreadable": "error",
    "package.missing_layer": "error",
    "dimensions.not_multiple_of_8": "error",
    "dimensions.not_multiple_of_16": "error",
    "layers.size_mismatch": "error",
    "palette.too_many_colours": "error",
    "palette.indexed_entries": "error",
    "color.not_rgb555": "error",
    "alpha.semi_transparent": "error",
    "tile.duplicate": "warning",
    "metatile.triple_layer": "error",
    "animation.unsupported": "error",
    "budget.palettes": "error",
    "budget.tiles": "error",
    "budget.metatiles": "error",
    "package.metatile_format": "error",
    "tile.out_of_range": "error",
    "palette.slot_out_of_range": "error",
    "palette.primary_overflow": "error",
    "palette.shared_with_primary": "warning",
    "palette.primary_mismatch": "error",
}


class Violation:
    """One finding. x/y are pixels in the file, tile is (column, row) of the 8x8 tile, metatile an index."""

    FIELDS = ("code", "severity", "file", "message", "x", "y", "tile", "metatile")

    def __init__(self, code, severity, file, message, x=None, y=None, tile=None, metatile=None):
        self.code, self.severity, self.file, self.message = code, severity, file, message
        self.x, self.y, self.tile, self.metatile = x, y, tile, metatile

    def as_dict(self) -> dict:
        return {k: getattr(self, k) for k in self.FIELDS if getattr(self, k) is not None}


class Report:
    """Violations plus exact counts and the usage figures of the checked asset."""

    def __init__(self, source: str = ""):
        self.source = source
        self.items: list[Violation] = []
        self.counts: dict[str, int] = {}
        self.usage: dict = {}
        self.notes: list[str] = []
        self.error_total = 0

    def add(self, code: str, file: str, message: str, severity: str | None = None, **where):
        severity = severity or CODES[code]
        self.error_total += severity == "error"
        key = f"{code}|{file}"
        self.counts[key] = self.counts.get(key, 0) + 1
        if self.counts[key] <= MAX_PER_CODE:
            self.items.append(Violation(code, severity, file, message, **where))

    def totals(self) -> dict[str, int]:
        out: dict[str, int] = {}
        for key, n in self.counts.items():
            code = key.split("|", 1)[0]
            out[code] = out.get(code, 0) + n
        return out

    @property
    def errors(self) -> int:
        return self.error_total

    def codes(self) -> set[str]:
        return {v.code for v in self.items if v.severity == "error"}

    def merge(self, other: "Report"):
        self.items += other.items
        for k, n in other.counts.items():
            self.counts[k] = self.counts.get(k, 0) + n
        self.error_total += other.error_total
        self.usage.update(other.usage)
        self.notes += other.notes

    def to_json(self) -> dict:
        return {"source": self.source, "ok": self.errors == 0, "errors": self.errors,
                "totals": self.totals(), "usage": self.usage, "notes": self.notes,
                "violations": [i.as_dict() for i in self.items]}

    def render(self) -> str:
        lines = [f"{self.source}: {'OK' if self.errors == 0 else str(self.errors) + ' error(s)'}"]
        for name, value in sorted(self.usage.items()):
            lines.append(f"  usage {name}: {value}")
        for item in self.items:
            where = item.file
            if item.x is not None:
                where += f" px({item.x},{item.y})"
            if item.tile is not None:
                where += f" tile({item.tile[0]},{item.tile[1]})"
            if item.metatile is not None:
                where += f" metatile {item.metatile}"
            lines.append(f"  {item.severity.upper():7} {item.code} {where}: {item.message}")
        for key, n in sorted(self.counts.items()):
            if n > MAX_PER_CODE:
                lines.append(f"  ... {key.replace('|', ' in ')}: {n - MAX_PER_CODE} more not listed")
        lines += [f"  note: {n}" for n in self.notes]
        return "\n".join(lines)


def rgb555_values() -> frozenset[int]:
    """Channel values reachable from a 5-bit value under the common expansions."""
    out = set()
    for v in range(32):
        out |= {v * 8, v * 8 | v >> 2, v * 255 // 31}
    return frozenset(out)


RGB555 = rgb555_values()


def flip_variants(tile: tuple[int, ...]):
    rows = [tile[i * 8:i * 8 + 8] for i in range(8)]
    for flip_h in (False, True):
        for flip_v in (False, True):
            r = rows[::-1] if flip_v else rows
            yield tuple(v for row in r for v in (row[::-1] if flip_h else row))


def canonical(tile: tuple[int, ...]) -> tuple[int, ...]:
    return min(flip_variants(tile))


def load_pixels(path: Path, report: Report, name: str):
    """(png, width, height, rows of (r,g,b,a) tuples) or None, reporting unreadable files."""
    try:
        image = png.read_png(path)
        rows = [list(zip(r[0::4], r[1::4], r[2::4], r[3::4])) for r in image.rgba_rows()]
    except (OSError, png.PngError) as exc:
        report.add("png.unreadable", name, f"cannot read PNG: {exc}")
        return None
    return image, image.width, image.height, rows


class Sheet:
    """One checked image: tiles keyed by (column, row) plus per-pixel findings."""

    def __init__(self, name: str, width: int, height: int):
        self.name, self.width, self.height = name, width, height
        self.tiles: dict[tuple[int, int], tuple[int, ...]] = {}
        self.colours: dict[tuple[int, int], frozenset[int]] = {}


def analyze_image(path: Path, name: str, report: Report, converted: bool = False, require16: bool = False,
                  transparent: tuple[int, int, int] | None = EXTRINSIC_TRANSPARENCY) -> Sheet | None:
    loaded = load_pixels(path, report, name)
    if loaded is None:
        return None
    image, width, height, rows = loaded
    if width % 8 or height % 8:
        report.add("dimensions.not_multiple_of_8", name,
                   f"size {width}x{height} is not a multiple of 8 (8x8 tiles)", x=width, y=height)
    elif (converted or require16) and (width % 16 or height % 16):
        report.add("dimensions.not_multiple_of_16", name,
                   f"size {width}x{height} is not a multiple of 16 (metatile grid)", x=width, y=height)
    if image.indexed and len(image.plte) > 16:
        report.add("palette.indexed_entries", name,
                   f"indexed palette has {len(image.plte)} entries, a tile palette holds 16")
    sheet = Sheet(name, width, height)
    bad_alpha: dict[tuple[int, int], list] = {}
    bad_colour: dict[tuple[int, int], list] = {}
    for ty in range(height // 8):
        for tx in range(width // 8):
            keys, colours = [], set()
            for py in range(8):
                row = rows[ty * 8 + py]
                for px in range(8):
                    r, g, b, a = row[tx * 8 + px]
                    if a == 0 or (transparent is not None and (r, g, b) == transparent and a == 255):
                        keys.append(-1)
                        continue
                    x, y = tx * 8 + px, ty * 8 + py
                    if a != 255:
                        entry = bad_alpha.setdefault((tx, ty), [0, x, y, a])
                        entry[0] += 1
                    if not (r in RGB555 and g in RGB555 and b in RGB555):
                        entry = bad_colour.setdefault((tx, ty), [0, x, y, (r, g, b)])
                        entry[0] += 1
                    key = r << 16 | g << 8 | b
                    keys.append(key)
                    colours.add(key)
            sheet.tiles[(tx, ty)] = tuple(keys)
            sheet.colours[(tx, ty)] = frozenset(colours)
            if len(colours) > COLOURS_PER_PALETTE:
                report.add("palette.too_many_colours", name,
                      f"{len(colours)} opaque colours in one tile, a palette holds {COLOURS_PER_PALETTE} "
                      "plus transparency", x=tx * 8, y=ty * 8, tile=(tx, ty))
    for (tx, ty), (n, x, y, a) in bad_alpha.items():
        report.add("alpha.semi_transparent", name, f"{n} semi-transparent pixel(s), first alpha {a}; "
                   "map graphics have no partial alpha", x=x, y=y, tile=(tx, ty))
    for (tx, ty), (n, x, y, c) in bad_colour.items():
        report.add("color.not_rgb555", name, f"{n} pixel(s) not exactly RGB555, first colour {c}",
                   x=x, y=y, tile=(tx, ty))
    return sheet


def pack_palettes(colour_sets) -> int:
    """First-fit-decreasing upper bound on the palettes needed for these per-tile colour sets."""
    palettes: list[set[int]] = []
    for colours in sorted({c for c in colour_sets if c}, key=len, reverse=True):
        if any(colours <= p for p in palettes):
            continue
        for p in palettes:
            if len(p | colours) <= COLOURS_PER_PALETTE:
                p |= colours
                break
        else:
            palettes.append(set(colours))
    return len(palettes)


def flag_duplicates(sheets: list[Sheet], report: Report) -> int:
    """Report flip-aware duplicate tiles; return the number of unique non-empty tiles."""
    seen: dict[tuple[int, ...], tuple[str, int, int]] = {}
    for sheet in sheets:
        for (tx, ty), tile in sheet.tiles.items():
            if all(v == -1 for v in tile):
                continue
            key = canonical(tile)
            if key in seen:
                first = seen[key]
                report.add("tile.duplicate", sheet.name, f"same as {first[0]} tile({first[1]},{first[2]}) "
                      "(flip-aware)", x=tx * 8, y=ty * 8, tile=(tx, ty))
            else:
                seen[key] = (sheet.name, tx, ty)
    return len(seen)


def validate_layers(package: Path, role: str = "primary", transparent=EXTRINSIC_TRANSPARENCY,
                    budgets: dict | None = None) -> Report:
    """Validate a Porytiles source package: bottom/middle/top.png (+ attributes.csv, anim/)."""
    report = Report(str(package))
    budget = {**BUDGETS[role], **(budgets or {})}
    sheets: list[Sheet] = []
    for layer in LAYERS:
        path = package / f"{layer}.png"
        if not path.is_file():
            report.add("package.missing_layer", f"{layer}.png", f"{layer}.png is missing from the package")
            continue
        sheet = analyze_image(path, f"{layer}.png", report, require16=True, transparent=transparent)
        if sheet:
            sheets.append(sheet)
    anim = package / "anim"
    if anim.is_dir() and any(anim.iterdir()):
        report.add("animation.unsupported", "anim/", "animated tiles are unsupported by this pipeline and "
                   "were not processed")
    if not sheets:
        return report
    sizes = {(s.width, s.height) for s in sheets}
    if len(sizes) > 1:
        detail = ", ".join(f"{s.name} {s.width}x{s.height}" for s in sheets)
        report.add("layers.size_mismatch", "layers", f"layers differ in size: {detail}")
        return report
    width, height = sizes.pop()
    if width % 16 or height % 16:
        return report
    cols, rows = width // 16, height // 16
    occupied = {}
    for sheet in sheets:
        occupied[sheet.name] = [[any(v != -1 for dx in (0, 1) for dy in (0, 1)
                                     for v in sheet.tiles[(mx * 2 + dx, my * 2 + dy)])
                                 for mx in range(cols)] for my in range(rows)]
    used_cells = 0
    for my in range(rows):
        for mx in range(cols):
            flags = [occupied[s.name][my][mx] for s in sheets]
            used_cells += any(flags)
            if len(sheets) == 3 and all(flags):
                report.add("metatile.triple_layer", "layers",
                      "content on bottom, middle and top; the engine has two layers",
                           x=mx * 16, y=my * 16, metatile=my * cols + mx)
    unique = flag_duplicates(sheets, report)
    tile_sets = [c for s in sheets for c in s.colours.values()]
    palettes = pack_palettes(tile_sets)
    report.usage.update({"metatiles": used_cells, "tiles_estimate": unique + 1,
                         "palettes_estimate": palettes, "budget": budget})
    # Estimates: Porytiles composites layers and deduplicates, so only the compiled check is exact.
    for code, used, limit, what in (
            ("budget.metatiles", used_cells, budget["metatiles"], "non-empty metatile cells"),
            ("budget.tiles", unique + 1, budget["tiles"], "unique tiles (flip-aware, plus the blank tile)"),
            ("budget.palettes", palettes, budget["palettes"], "palettes by greedy packing")):
        if used > limit:
            report.add(code, "layers", f"{used} {what} exceed the {role} budget of {limit} "
                       "(source-stage estimate; the compiled check is exact)", severity="warning")
    return report


def validate_png(path: Path, converted: bool = True, transparent=EXTRINSIC_TRANSPARENCY) -> Report:
    """Validate one loose image, e.g. converted Gen 4 art (multiple of 16, no partial alpha)."""
    report = Report(str(path))
    sheet = analyze_image(path, path.name, report, converted=converted, transparent=transparent)
    if sheet:
        unique = flag_duplicates([sheet], report)
        report.usage.update({"tiles_estimate": unique + 1,
                        "palettes_estimate": pack_palettes(sheet.colours.values())})
    return report


def read_words(path: Path) -> list[int]:
    data = path.read_bytes()
    return [int.from_bytes(data[i:i + 2], "little") for i in range(0, len(data) - 1, 2)]


def validate_compiled(bin_dir: Path, role: str = "primary", primary_bin: Path | None = None,
                      budgets: dict | None = None) -> Report:
    """Exact check of Porytiles output (or any decomp tileset directory) against the engine."""
    report = Report(str(bin_dir))
    budget = {**BUDGETS[role], **(budgets or {})}
    meta_path, attr_path = bin_dir / "metatiles.bin", bin_dir / "metatile_attributes.bin"
    tiles_path = bin_dir / "tiles.png"
    for path in (meta_path, tiles_path):
        if not path.is_file():
            report.add("png.unreadable" if path.suffix == ".png" else "package.metatile_format",
                       path.name, f"{path.name} not found")
            return report
    raw = meta_path.read_bytes()
    attr_size = attr_path.stat().st_size if attr_path.is_file() else None
    triple = "24-byte metatiles: a triple-layer package; the engine reads 16 bytes (two layers)"
    if attr_size is not None and attr_size % 2 == 0 and len(raw) == attr_size // 2 * 16:
        count = len(raw) // 16
    elif attr_size is not None and attr_size % 2 == 0 and len(raw) == attr_size // 2 * 24:
        report.add("package.metatile_format", "metatiles.bin", triple)
        return report
    elif attr_size is None and len(raw) % 16 == 0:
        count = len(raw) // 16
    elif attr_size is None and len(raw) % 24 == 0:
        report.add("package.metatile_format", "metatiles.bin", triple)
        return report
    elif attr_size is not None and len(raw) % 16 == 0 and attr_size == len(raw) // 16 * 4:
        report.add("package.metatile_format", "metatile_attributes.bin",
                   "4-byte attributes (FireRed layout); the pinned Emerald engine reads 2 bytes")
        return report
    else:
        report.add("package.metatile_format", "metatiles.bin",
                   f"{len(raw)} bytes with {attr_size} attribute bytes do not form 16-byte metatiles "
                   "with 2-byte attributes")
        return report
    try:
        image = png.read_png(tiles_path)
    except (OSError, png.PngError) as exc:
        report.add("png.unreadable", "tiles.png", f"cannot read PNG: {exc}")
        return report
    if image.width % 8 or image.height % 8:
        report.add("dimensions.not_multiple_of_8", "tiles.png", f"size {image.width}x{image.height}")
        return report
    tiles = image.width // 8 * (image.height // 8)
    words = read_words(meta_path)
    used_pals: set[int] = set()
    max_tile = 0
    shared_slots: set[int] = set()              # primary palettes used by the secondary's own tiles
    reported: set[tuple[str, int]] = set()      # one finding per metatile and code

    def once(code, meta, message):
        if (code, meta) not in reported:
            reported.add((code, meta))
            report.add(code, "metatiles.bin", message, metatile=meta)

    for meta in range(count):
        for slot in range(8):
            word = words[meta * 8 + slot]
            tile, pal = word & 0x3FF, word >> 12
            used_pals.add(pal)
            max_tile = max(max_tile, tile)
            local = tile if role == "primary" else tile - 512   # secondary ids below 512 are the primary's
            if local >= tiles or (role == "primary" and tile >= 512):
                once("tile.out_of_range", meta,
                      f"tile id {tile} is outside the {tiles} tiles of tiles.png")
            if pal >= PALETTE_SLOTS_TOTAL:
                once("palette.slot_out_of_range", meta,
                      f"palette {pal} is outside the {PALETTE_SLOTS_TOTAL} map palettes")
            elif role == "primary" and pal >= PALETTE_SLOTS_IN_PRIMARY:
                once("palette.primary_overflow", meta,
                      f"primary tileset uses palette {pal}; slots {PALETTE_SLOTS_IN_PRIMARY}+ belong to "
                      "the secondary")
            elif role == "secondary" and pal < PALETTE_SLOTS_IN_PRIMARY and tile >= 512:
                shared_slots.add(pal)
                once("palette.shared_with_primary", meta,
                      f"secondary tile {tile} is drawn with primary palette {pal}; it only renders "
                      "correctly with the same partner primary")
    own = {p for p in used_pals if (p < PALETTE_SLOTS_IN_PRIMARY) == (role == "primary") and p < PALETTE_SLOTS_TOTAL}
    if role == "secondary" and primary_bin is not None:
        for pal in sorted(shared_slots):
            mine = png.parse_pal(bin_dir / "palettes" / f"{pal:02d}.pal")
            theirs = png.parse_pal(primary_bin / "palettes" / f"{pal:02d}.pal")
            if mine is not None and theirs is not None and mine != theirs:
                report.add("palette.primary_mismatch", f"palettes/{pal:02d}.pal",
                      f"secondary was built against a different palette {pal} than the given primary")
    metatile_limit = budget["metatiles"]
    report.usage.update({"metatiles": count, "tiles": tiles, "max_tile_id": max_tile,
                         "palettes": len(own), "palette_slots_used": sorted(used_pals), "budget": budget})
    for code, used, limit, what in (("budget.metatiles", count, metatile_limit, "metatiles"),
                                    ("budget.tiles", tiles, budget["tiles"], "tiles"),
                                    ("budget.palettes", len(own), budget["palettes"], "palettes")):
        if used > limit:
            report.add(code, "metatiles.bin" if code != "budget.tiles" else "tiles.png",
                       f"{used} {what} exceed the {role} budget of {limit}")
    return report


# ---------------------------------------------------------------- pipeline

def _toolchain():
    import setup_toolchain
    return setup_toolchain


def scratch_project(project: Path) -> Path:
    """Only a scratch copy under $TAUROS_DATA may be modified, never the repository."""
    project = project.expanduser().resolve()
    data = _toolchain().data_dir()
    if ROOT == project or ROOT in project.parents:
        raise SystemExit(f"refusing to build inside the repository: {project}")
    if data not in project.parents:
        raise SystemExit(f"the project must be a scratch copy under {data}")
    if not (project / "include/fieldmap.h").is_file():
        raise SystemExit(f"not a decomp project (include/fieldmap.h missing): {project}")
    return project


def container(project: Path, command: str) -> str:
    st = _toolchain()
    tag = st.image_tag(st.load_manifest())
    return st.run(st.hardened_run(tag, command, project), capture=True)


def tileset_dir(project: Path, name: str, role: str) -> Path:
    slug = name.removeprefix("gTileset_")
    out = []
    for i, ch in enumerate(slug):
        if ch.isupper() and i and not slug[i - 1].isupper() and slug[i - 1] != "_":
            out.append("_")
        out.append(ch.lower())
    return project / "data/tilesets" / role / "".join(out)


def build(args) -> int:
    project = scratch_project(Path(args.project))
    source = Path(args.source).expanduser().resolve()
    role = args.role
    report = validate_layers(source, role)
    print(report.render())
    if report.errors:
        print("Stopped before Porytiles: fix the source violations above.", file=sys.stderr)
        return 1
    flags = "--metatile-attribute-size 2"
    if role == "secondary":
        if not args.primary:
            raise SystemExit("--primary is required for a secondary tileset")
        flags += f" --primary-pairing-mode manual --primary-pairing-partners {args.primary}"
    managed = project / "porytiles/tilesets" / args.name
    if not managed.exists():
        sec = " --secondary" if role == "secondary" else ""
        container(project, f"porytiles create-tileset {args.name}{sec} {flags}")
    target = tileset_dir(project, args.name, role)
    src = target / "porytiles_src"
    anim = src / "anim"
    if anim.exists():
        shutil.rmtree(anim)                  # the create template ships a sample animation
    for layer in LAYERS:
        shutil.copyfile(source / f"{layer}.png", src / f"{layer}.png")
    if (source / "attributes.csv").is_file():
        shutil.copyfile(source / "attributes.csv", src / "attributes.csv")
    container(project, f"porytiles compile-tileset {args.name} {flags}")
    compiled = validate_compiled(target / "porytiles_bin", role,
                                 tileset_dir(project, args.primary, "primary") / "porytiles_bin"
                                 if args.primary and (tileset_dir(project, args.primary, "primary")
                                                      / "porytiles_bin").is_dir() else None)
    print(compiled.render())
    if compiled.errors:
        return 1
    if args.make:
        jobs = os.cpu_count() or 2
        container(project, f"make -j{jobs} rom syms")
        rom = project / "pokeemerald.gba"
        print(f"built {rom.name}: {rom.stat().st_size} bytes")
    return 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="cmd", required=True)
    v = sub.add_parser("validate")
    v.add_argument("path")
    v.add_argument("--role", choices=BUDGETS, default="primary")
    v.add_argument("--converted", action="store_true", help="loose PNG of converted Gen 4 art")
    v.add_argument("--json", action="store_true")
    c = sub.add_parser("validate-compiled")
    c.add_argument("path")
    c.add_argument("--role", choices=BUDGETS, default="primary")
    c.add_argument("--primary-bin")
    c.add_argument("--json", action="store_true")
    b = sub.add_parser("build")
    b.add_argument("--project", required=True, help="scratch copy of a project under $TAUROS_DATA")
    b.add_argument("--source", required=True, help="directory with bottom.png, middle.png, top.png")
    b.add_argument("--name", required=True, help="tileset symbol, e.g. gTileset_MyTiles")
    b.add_argument("--role", choices=BUDGETS, default="primary")
    b.add_argument("--primary", help="partner primary symbol for a secondary")
    b.add_argument("--make", action="store_true", help="build the ROM in the container afterwards")
    args = parser.parse_args(argv)
    if args.cmd == "build":
        return build(args)
    path = Path(args.path)
    if args.cmd == "validate-compiled":
        report = validate_compiled(path, args.role, Path(args.primary_bin) if args.primary_bin else None)
    elif path.is_dir():
        report = validate_layers(path, args.role)
    else:
        report = validate_png(path, converted=args.converted or True)
    print(json.dumps(report.to_json(), indent=2) if args.json else report.render())
    return 1 if report.errors else 0


if __name__ == "__main__":
    sys.exit(main())
