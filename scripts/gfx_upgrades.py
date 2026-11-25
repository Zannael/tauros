#!/usr/bin/env python3
"""Graphical upgrade entries for the local asset catalog: apply, revert, validate, preview (GFX-004).

Stdlib only (Python 3.10+). Entries are JSON files in ``$TAUROS_DATA/assets/catalog/upgrades/``
(they name external assets, so they stay local); they point at files of the catalog sources by
relative path and sha256 and are applied only to a scratch project under ``$TAUROS_DATA``.

Entry fields: id, kind (tileset|weather), title, status (unverified|verified|conversion-required),
status_basis, sources[{id, path, sha256, pin, credits, terms}], pairing, prerequisites,
operations, protect, validate, preview, verification.

Operations (paths are relative to the project, never absolute and never ``..``):
  copy_tree     {from, to, exclude?}   new directory copied from the project itself
  copy_source   {source, to, replace?} one catalog source file, sha256 checked, copied in; replace
                                       may overwrite a file inside a tree this entry created
  write_file    {source, to, content}  authored text file; sha256 of content must equal the source's;
                                       like copy_source it may only overwrite inside a tree it created
  replace_text  {file, old, new}       exactly one occurrence of ``old``

``apply`` records what it created and changed in ``<project>/.tauros/upgrades/<id>/``; ``revert``
restores it and refuses when a file changed afterwards. Both are idempotent. ``protect`` lists
project paths (shared tilesets, collisions, events) that must be byte-identical after ``apply``.
Entries marked conversion-required are refused. Commands: list, show, apply, revert, preview.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import asset_catalog as ac  # noqa: E402
import gfx_pipeline as gp  # noqa: E402

KINDS = ("tileset", "weather")
STATUSES = ("unverified", "verified", "conversion-required")
OPERATIONS = ("copy_tree", "copy_source", "write_file", "replace_text")
STATE_DIR = Path(".tauros/upgrades")


class UpgradeError(Exception):
    pass


def upgrades_dir() -> Path:
    return ac.data_root() / "upgrades"


def sources_root() -> Path:
    return ac.data_root() / "sources"


def sha256_file(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def tree_hash(path: Path) -> str:
    """Hash of a file, or of a directory's relative names and contents; ``absent`` if missing."""
    if not path.exists():
        return "absent"
    if path.is_file():
        return sha256_file(path)
    digest = hashlib.sha256()
    for item in sorted(p for p in path.rglob("*") if p.is_file()):
        digest.update(str(item.relative_to(path)).encode() + b"\0" + sha256_file(item).encode() + b"\n")
    return digest.hexdigest()


def safe(project: Path, rel: str) -> Path:
    if not isinstance(rel, str) or not rel or Path(rel).is_absolute() or ".." in Path(rel).parts:
        raise UpgradeError(f"unsafe project path: {rel!r}")
    full = (project / rel).resolve()
    if project != full and project not in full.parents:
        raise UpgradeError(f"path escapes the project: {rel!r}")
    return full


# ------------------------------------------------------------------- entries

def check_entry(entry: dict) -> list[str]:
    """Static problems of an entry; empty when it is well formed."""
    errors = []
    for key in ("id", "kind", "title", "status", "sources", "prerequisites", "operations", "preview"):
        if key not in entry:
            errors.append(f"missing field {key}")
    if errors:
        return errors
    if entry["kind"] not in KINDS:
        errors.append(f"unknown kind {entry['kind']!r}")
    if entry["status"] not in STATUSES:
        errors.append(f"unknown status {entry['status']!r}")
    if not entry.get("status_basis"):
        errors.append("status_basis is required")
    if entry["kind"] == "tileset" and not entry.get("pairing"):
        errors.append("tileset entries need pairing")
    ids = set()
    for src in entry["sources"]:
        for key in ("id", "sha256", "credits", "terms"):
            if not src.get(key):
                errors.append(f"source {src.get('id', '?')}: missing {key}")
        if src.get("path") and not src.get("pin"):
            errors.append(f"source {src.get('id', '?')}: external file without pin")
        ids.add(src.get("id"))
    for n, op in enumerate(entry["operations"]):
        name = op.get("op")
        if name not in OPERATIONS:
            errors.append(f"operation {n}: unknown {name!r}")
        elif name in ("copy_source", "write_file") and op.get("source") not in ids:
            errors.append(f"operation {n}: source {op.get('source')!r} not listed in sources")
    return errors


def load_entry(entry_id: str) -> dict:
    path = upgrades_dir() / f"{entry_id}.json"
    if not path.is_file():
        raise UpgradeError(f"no entry {entry_id!r} in {upgrades_dir()}")
    entry = json.loads(path.read_text(encoding="utf-8"))
    problems = check_entry(entry)
    if problems:
        raise UpgradeError(f"entry {entry_id} is not valid: " + "; ".join(problems))
    return entry


def list_entries() -> list[dict]:
    out = []
    for path in sorted(upgrades_dir().glob("*.json")):
        out.append(json.loads(path.read_text(encoding="utf-8")))
    return out


def verify_sources(entry: dict) -> dict[str, Path]:
    found = {}
    for src in entry["sources"]:
        if not src.get("path"):
            continue
        path = sources_root() / src["path"]
        if not path.is_file():
            raise UpgradeError(f"source file missing: {src['path']} (run asset_catalog.py fetch)")
        if sha256_file(path) != src["sha256"]:
            raise UpgradeError(f"source sha256 differs from the entry pin: {src['path']}")
        found[src["id"]] = path
    return found


# ------------------------------------------------------------ apply / revert

def state_dir(project: Path, entry_id: str) -> Path:
    return project / STATE_DIR / entry_id


def read_manifest(project: Path, entry_id: str) -> dict | None:
    path = state_dir(project, entry_id) / "manifest.json"
    return json.loads(path.read_text(encoding="utf-8")) if path.is_file() else None


def manifest_drift(project: Path, manifest: dict) -> list[str]:
    """Files that no longer match what apply recorded."""
    drift = []
    for rel, sha in {**manifest["created"], **manifest["final"]}.items():
        if tree_hash(project / rel) != sha:
            drift.append(rel)
    return sorted(drift)


def apply_operations(project: Path, entry: dict, sources: dict[str, Path], state: Path) -> dict:
    created: dict[str, str] = {}
    backups: dict[str, str] = {}
    touched: list[Path] = []                  # created paths, for rollback
    try:
        for op in entry["operations"]:
            name = op["op"]
            if name in ("copy_tree", "copy_source", "write_file"):
                dest = safe(project, op["to"])
                inside = any(root == dest or root in dest.parents for root in touched)
                if dest.exists() and not (op.get("replace") and name != "copy_tree" and inside):
                    raise UpgradeError(f"{op['to']} already exists and is not part of this entry")
                if name == "copy_tree":
                    origin = safe(project, op["from"])
                    if not origin.is_dir():
                        raise UpgradeError(f"{op['from']} is not a directory")
                    skip = set(op.get("exclude", []))
                    shutil.copytree(origin, dest, ignore=lambda _d, names: [n for n in names if n in skip])
                elif name == "write_file":
                    wanted = next(src["sha256"] for src in entry["sources"] if src["id"] == op["source"])
                    if hashlib.sha256(op["content"].encode("utf-8")).hexdigest() != wanted:
                        raise UpgradeError(f"{op['to']}: content does not match the sha256 of source {op['source']}")
                    dest.parent.mkdir(parents=True, exist_ok=True)
                    dest.write_text(op["content"], encoding="utf-8")
                else:
                    dest.parent.mkdir(parents=True, exist_ok=True)
                    shutil.copyfile(sources[op["source"]], dest)
                if not inside:
                    touched.append(dest)
                    created[op["to"]] = ""
            else:
                target = safe(project, op["file"])
                if not target.is_file():
                    raise UpgradeError(f"{op['file']} not found")
                text = target.read_text(encoding="utf-8")
                if text.count(op["old"]) != 1:
                    raise UpgradeError(f"{op['file']}: expected exactly one occurrence of the anchor, "
                                       f"found {text.count(op['old'])}")
                if op["file"] not in backups:
                    backup = state / "backup" / op["file"]
                    backup.parent.mkdir(parents=True, exist_ok=True)
                    shutil.copyfile(target, backup)
                    backups[op["file"]] = tree_hash(target)
                target.write_text(text.replace(op["old"], op["new"]), encoding="utf-8")
        created = {rel: tree_hash(project / rel) for rel in created}
        return {"created": created, "backups": backups}
    except Exception:
        restore(project, backups, state)
        for path in reversed(touched):
            shutil.rmtree(path) if path.is_dir() else path.unlink(missing_ok=True)
        shutil.rmtree(state, ignore_errors=True)
        raise


def restore(project: Path, backups: dict[str, str], state: Path):
    for rel in backups:
        backup = state / "backup" / rel
        if backup.is_file():
            shutil.copyfile(backup, project / rel)


def run_validators(project: Path, entry: dict) -> gp.Report:
    report = gp.Report(entry["id"])
    for check in entry.get("validate", []):
        primary_bin = project / check["primary"] if check.get("primary") else None
        report.merge(gp.validate_compiled(project / check["path"], check.get("role", "primary"), primary_bin))
    return report


def apply_entry(project: Path, entry: dict, make: bool = False) -> dict:
    project = gp.scratch_project(project)
    if entry["status"] == "conversion-required":
        raise UpgradeError(f"{entry['id']} is conversion-required: {entry['status_basis']}")
    manifest = read_manifest(project, entry["id"])
    if manifest:
        drift = manifest_drift(project, manifest)
        if drift:
            raise UpgradeError(f"{entry['id']} is applied but changed afterwards: {', '.join(drift)}")
        return {"result": "already-applied", "id": entry["id"]}
    sources = verify_sources(entry)
    protect = {rel: tree_hash(safe(project, rel)) for rel in entry.get("protect", [])}
    state = state_dir(project, entry["id"])
    done = apply_operations(project, entry, sources, state)
    final = {rel: tree_hash(project / rel) for rel in done["backups"]}
    manifest = {"id": entry["id"], "created": done["created"], "final": final,
                "backups": done["backups"], "sources": {s["id"]: s["sha256"] for s in entry["sources"]}}
    state.mkdir(parents=True, exist_ok=True)
    (state / "manifest.json").write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    problems = [f"{rel} changed" for rel, sha in protect.items() if tree_hash(safe(project, rel)) != sha]
    report = run_validators(project, entry)
    if problems or report.errors:
        revert_entry(project, entry["id"])
        errors = [f"{v.file}: {v.message}" for v in report.items if v.severity == "error"]
        raise UpgradeError("rolled back: " + "; ".join(problems + errors))
    if make:
        jobs = os.cpu_count() or 2
        gp.container(project, f"make -j{jobs} rom syms")
    return {"result": "applied", "id": entry["id"], "created": sorted(done["created"]),
            "modified": sorted(done["backups"]), "warnings": sum(1 for v in report.items if v.severity == "warning")}


def revert_entry(project: Path, entry_id: str) -> dict:
    project = gp.scratch_project(project)
    manifest = read_manifest(project, entry_id)
    if not manifest:
        return {"result": "not-applied", "id": entry_id}
    drift = manifest_drift(project, manifest)
    if drift:
        raise UpgradeError(f"{entry_id} cannot be reverted, changed afterwards (revert later entries first): "
                           + ", ".join(drift))
    state = state_dir(project, entry_id)
    restore(project, manifest["backups"], state)
    for rel in manifest["created"]:
        path = project / rel
        shutil.rmtree(path) if path.is_dir() else path.unlink(missing_ok=True)
    shutil.rmtree(state)
    for parent in (project / STATE_DIR, project / STATE_DIR.parent):
        if parent.is_dir() and not any(parent.iterdir()):
            parent.rmdir()
    return {"result": "reverted", "id": entry_id}


# ------------------------------------------------------------------- preview

def read_pal(path: Path):
    lines = path.read_text(encoding="ascii").split()
    return [tuple(int(v) for v in lines[3 + 3 * i:6 + 3 * i]) for i in range(int(lines[2]))]


def load_tileset(directory: Path):
    """tiles as lists of 64 palette indices, 16 palettes, metatile words."""
    image = ac.read_png(directory / "tiles.png")
    rows = list(image.index_rows())
    per_row = image.width // 8
    tiles = []
    for ty in range(image.height // 8):
        for tx in range(per_row):
            tiles.append([rows[ty * 8 + y][tx * 8 + x] for y in range(8) for x in range(8)])
    pals = [read_pal(p) if (p := directory / "palettes" / f"{i:02d}.pal").is_file() else [(0, 0, 0)] * 16
            for i in range(16)]
    data = (directory / "metatiles.bin").read_bytes()
    return tiles, pals, [int.from_bytes(data[i:i + 2], "little") for i in range(0, len(data), 2)]


def render_map(project: Path, layout_id: str, primary: str, secondary: str):
    """Approximate rendering (bottom then top layer, no animation, no layer-type split)."""
    layouts = json.loads((project / "data/layouts/layouts.json").read_text(encoding="utf-8"))["layouts"]
    layout = next(item for item in layouts if item["id"] == layout_id)
    p_tiles, p_pals, p_meta = load_tileset(project / primary)
    s_tiles, s_pals, s_meta = load_tileset(project / secondary)
    blocks = (project / layout["blockdata_filepath"]).read_bytes()
    width, height = layout["width"], layout["height"]
    backdrop = p_pals[0][0]
    pixels = [[backdrop] * (width * 16) for _ in range(height * 16)]
    for n in range(width * height):
        block = int.from_bytes(blocks[n * 2:n * 2 + 2], "little")
        mid = block & 0x3FF
        words = p_meta[mid * 8:mid * 8 + 8] if mid < 512 else s_meta[(mid - 512) * 8:(mid - 512) * 8 + 8]
        ox, oy = n % width * 16, n // width * 16
        for i, word in enumerate(words):
            tid, hflip, vflip, slot = word & 0x3FF, word >> 10 & 1, word >> 11 & 1, word >> 12
            tile = p_tiles[tid] if tid < 512 else s_tiles[tid - 512]
            pal = p_pals[slot] if slot < 6 else s_pals[slot]
            bx, by = ox + (i % 2) * 8, oy + (i // 2 % 2) * 8
            for y in range(8):
                for x in range(8):
                    idx = tile[(7 - y if vflip else y) * 8 + (7 - x if hflip else x)]
                    if idx:
                        pixels[by + y][bx + x] = pal[idx]
    return width * 16, height * 16, pixels


def preview_entry(project: Path, entry: dict, out: Path | None = None) -> Path:
    project = gp.scratch_project(project)
    spec = entry["preview"]
    if "layout" not in spec:
        raise UpgradeError(f"{entry['id']} has no rendered preview: {spec.get('manual', 'check in the emulator')}")
    if not read_manifest(project, entry["id"]):
        raise UpgradeError(f"{entry['id']} is not applied; the after side needs the applied project")
    before = render_map(project, spec["layout"], *spec["before"])
    after = render_map(project, spec["layout"], *spec["after"])
    width, height = before[0] + 8 + after[0], before[1]
    rows = []
    for y in range(height):
        row = bytearray()
        for side, gap in ((before, 0), (after, 8)):
            row += bytes(255 for _ in range(gap * 4)) if gap else b""
            for r, g, b in side[2][y]:
                row += bytes((r, g, b, 255))
        rows.append(bytes(row))
    out = out or ac.data_root() / "previews" / "upgrades" / f"{entry['id']}.png"
    ac.write_png(out, width, height, rows)
    return out


# ----------------------------------------------------------------------- CLI

def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    sub = parser.add_subparsers(dest="cmd", required=True)
    sub.add_parser("list")
    for name in ("show", "apply", "revert", "preview"):
        p = sub.add_parser(name)
        p.add_argument("id")
        if name != "show":
            p.add_argument("--project", required=True, help="scratch project under $TAUROS_DATA")
        if name == "apply":
            p.add_argument("--make", action="store_true", help="build the ROM in the container afterwards")
        if name == "preview":
            p.add_argument("--out")
    args = parser.parse_args(argv)
    try:
        if args.cmd == "list":
            for entry in list_entries():
                print(f"{entry['id']}  [{entry['kind']}, {entry['status']}]  {entry['title']}")
            return 0
        entry = load_entry(args.id)
        if args.cmd == "show":
            print(json.dumps(entry, indent=2, ensure_ascii=False))
        elif args.cmd == "apply":
            print(json.dumps(apply_entry(Path(args.project), entry, args.make), indent=2))
        elif args.cmd == "revert":
            print(json.dumps(revert_entry(Path(args.project), args.id), indent=2))
        else:
            print(preview_entry(Path(args.project), entry, Path(args.out) if args.out else None))
    except UpgradeError as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
