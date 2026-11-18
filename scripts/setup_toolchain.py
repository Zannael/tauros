#!/usr/bin/env python3
"""Set up the Tauros GBA toolchain and pinned engine without touching the host system.

  install (default)  build the Docker image, clone the pinned engine, build its base ROM
  check              verify everything is present and pinned; change nothing

Pins live in toolchain/sources.json. Data lives under $TAUROS_DATA (default
~/.local/share/tauros): engines/<name>-<ref> is a pristine shallow clone with push
disabled, never built in place; sources/<name>-<ref> holds pinned native source
compiled by Cargo build scripts (libmgba); engines/<name>-<ref>-base is a `git archive` copy
built in a hardened container (no network, host uid/gid, no capabilities, read-only
root). Python 3.10+, standard library only.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
MANIFEST = ROOT / "toolchain/sources.json"
BUILD_RECORD = "tauros-base-build.json"
IN_PROGRESS = ".tauros-base-build-in-progress"
SHA256 = re.compile(r"^[0-9a-f]{64}$")
COMMIT = re.compile(r"^[0-9a-f]{40}$")


class SetupError(RuntimeError):
    """A missing prerequisite, a pin mismatch or a failed step."""


def data_dir(override: str | None = None) -> Path:
    value = override or os.environ.get("TAUROS_DATA") or "~/.local/share/tauros"
    return Path(value).expanduser().resolve()


def load_manifest(path: Path = MANIFEST) -> dict:
    manifest = json.loads(path.read_text(encoding="utf-8"))
    errors = []
    if manifest.get("schema") != 1:
        errors.append("schema must be 1")
    image = manifest.get("image", {})
    for key in ("name", "dockerfile", "base", "base_digest"):
        if not image.get(key):
            errors.append(f"image.{key} missing")
    if not str(image.get("base_digest", "")).startswith("sha256:"):
        errors.append("image.base_digest must be a sha256 digest")
    for item in manifest.get("downloads", []):
        if not SHA256.match(item.get("sha256", "")):
            errors.append(f"download {item.get('name')}: sha256 must be 64 hex digits")
        if not str(item.get("url", "")).startswith("https://"):
            errors.append(f"download {item.get('name')}: url must be https")
        if not item.get("license") or not item.get("version"):
            errors.append(f"download {item.get('name')}: license and version required")
    engines = manifest.get("engines", [])
    if not engines:
        errors.append("at least one engine required")
    for engine in engines + manifest.get("sources", []):
        if not COMMIT.match(engine.get("commit", "")):
            errors.append(f"engine {engine.get('name')}: commit must be a full 40-hex sha")
        if not engine.get("ref") or not str(engine.get("url", "")).startswith("https://"):
            errors.append(f"engine {engine.get('name')}: https url and ref required")
    if errors:
        raise SetupError("Invalid manifest: " + "; ".join(errors))
    return manifest


def image_tag(manifest: dict) -> str:
    """Tag derived from the Dockerfile bytes, so any change forces a rebuild."""
    digest = hashlib.sha256((ROOT / manifest["image"]["dockerfile"]).read_bytes()).hexdigest()
    return f"{manifest['image']['name']}:{digest[:12]}"


def hardened_run(tag: str, command: str, workdir: Path | None = None) -> list[str]:
    args = ["docker", "run", "--rm", "--network", "none", "--user", f"{os.getuid()}:{os.getgid()}",
            "--cap-drop", "ALL", "--security-opt", "no-new-privileges", "--read-only",
            "--tmpfs", "/tmp:rw,exec,size=1g", "--init"]
    if workdir is not None:
        args += ["--volume", f"{workdir}:/work:rw", "--workdir", "/work"]
    return args + [tag, "bash", "-c", command]


def run(args: list[str], cwd: Path | None = None, capture: bool = False) -> str:
    result = subprocess.run(args, cwd=cwd, text=True, capture_output=capture)
    if result.returncode:
        detail = (result.stderr or result.stdout or "").strip()[-2000:] if capture else ""
        raise SetupError(f"command failed ({result.returncode}): {' '.join(args[:6])} ... {detail}")
    return result.stdout if capture else ""


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def source_dir(data: Path, source: dict) -> Path:
    return data / "sources" / f"{source['name']}-{source['ref'].replace('/', '-')}"


def engine_dirs(data: Path, engine: dict) -> tuple[Path, Path]:
    slug = f"{engine['name']}-{engine['ref'].replace('/', '-')}"
    return data / "engines" / slug, data / "engines" / f"{slug}-base"


def ensure_image(manifest: dict, tag: str, install: bool) -> bool:
    exists = subprocess.run(["docker", "image", "inspect", tag], capture_output=True).returncode == 0
    if exists or not install:
        return False
    dockerfile = ROOT / manifest["image"]["dockerfile"]
    run(["docker", "build", "--pull=false", "-t", tag, "-f", str(dockerfile), str(dockerfile.parent)])
    return True


def tool_versions(tag: str) -> dict:
    probes = {
        "arm-none-eabi-gcc": '"$DEVKITARM/bin/arm-none-eabi-gcc" --version 2>&1 | head -n1',
        "poryscript": "poryscript -v 2>&1 | head -n1",
        "porytiles": "porytiles --version 2>&1 | head -n1",
        "libpng-dev": "dpkg-query -W -f='${Version}' libpng-dev",
        "libelf1": "dpkg-query -W -f='${Version}' libelf1",
    }
    command = "; ".join(f"printf '%s=' {name}; {probe}; echo" for name, probe in probes.items())
    out = run(hardened_run(tag, command), capture=True)
    versions = {}
    for line in out.splitlines():
        name, sep, value = line.partition("=")
        if sep and name in probes and value.strip():
            versions[name] = value.strip()
    missing = sorted(set(probes) - set(versions))
    if missing:
        raise SetupError("tools missing in image: " + ", ".join(missing))
    return versions


def ensure_clone(engine: dict, clone: Path, install: bool) -> bool:
    """Shallow, pristine clone of one pinned commit with push disabled."""
    created = False
    if not clone.exists():
        if not install:
            raise SetupError(f"clone missing: {clone}")
        clone.mkdir(parents=True)
        run(["git", "init", "-q"], cwd=clone)
        run(["git", "remote", "add", "origin", engine["url"]], cwd=clone)
        run(["git", "remote", "set-url", "--push", "origin", "DISABLED"], cwd=clone)
        run(["git", "fetch", "-q", "--depth", "1", "origin", engine["commit"]], cwd=clone)
        run(["git", "-c", "advice.detachedHead=false", "checkout", "-q", "--detach", "FETCH_HEAD"], cwd=clone)
        created = True
    head = run(["git", "rev-parse", "HEAD"], cwd=clone, capture=True).strip()
    if head != engine["commit"]:
        raise SetupError(f"{clone.name} at {head}, pinned {engine['commit']}")
    if run(["git", "status", "--porcelain"], cwd=clone, capture=True).strip():
        raise SetupError(f"clone is dirty; it must stay pristine: {clone}")
    if run(["git", "remote", "get-url", "--push", "origin"], cwd=clone, capture=True).strip() != "DISABLED":
        raise SetupError(f"{clone.name}: push URL must be DISABLED")
    return created


def read_record(base: Path) -> dict | None:
    try:
        return json.loads((base / BUILD_RECORD).read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return None


def base_build_current(engine: dict, base: Path, tag: str) -> bool:
    record = read_record(base)
    if not record or record.get("commit") != engine["commit"] or record.get("image") != tag:
        return False
    try:
        return all(sha256_file(base / engine[key]) == record[f"{key}_sha256"] for key in ("rom", "sym"))
    except (OSError, KeyError):
        return False


def check_pair(base: Path, engine: dict) -> None:
    """ROM and .sym must both be derived from the final ELF, or symbols may lie."""
    elf = base / Path(engine["rom"]).with_suffix(".elf").name
    stamps = {name: (base / name).stat().st_mtime_ns for name in (engine["rom"], engine["sym"])}
    stale = [name for name, stamp in stamps.items() if stamp < elf.stat().st_mtime_ns]
    if stale:
        raise SetupError("built from an older ELF than the final one: " + ", ".join(stale))


def build_base(engine: dict, clone: Path, base: Path, tag: str, install: bool) -> bool:
    if base_build_current(engine, base, tag):
        return False
    if not install:
        raise SetupError(f"base build missing or stale: {base}")
    if base.exists():
        ours = read_record(base) is not None or (base / IN_PROGRESS).exists()
        if not ours and any(base.iterdir()):
            raise SetupError(f"refusing to replace a directory Tauros did not create: {base}")
        shutil.rmtree(base)
    base.mkdir(parents=True)
    (base / IN_PROGRESS).touch()
    archive = subprocess.Popen(["git", "archive", "--format=tar", engine["commit"]], cwd=clone,
                               stdout=subprocess.PIPE)
    subprocess.run(["tar", "-x", "-C", str(base)], stdin=archive.stdout, check=True)
    if archive.wait():
        raise SetupError("git archive failed")
    # The copy deliberately has no .git; the expansion's build refuses that unless told so.
    (base / ".histignore").touch()
    jobs = os.cpu_count() or 2
    # One invocation, so the ROM and the symbol table come from the same ELF.
    run(hardened_run(tag, f"make -j{jobs} rom syms", base))
    check_pair(base, engine)
    record = dict(commit=engine["commit"], ref=engine["ref"], image=tag,
                  rom_sha256=sha256_file(base / engine["rom"]), sym_sha256=sha256_file(base / engine["sym"]))
    (base / BUILD_RECORD).write_text(json.dumps(record, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    (base / IN_PROGRESS).unlink()
    return True


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("mode", nargs="?", choices=("install", "check"), default="install")
    parser.add_argument("--data", help="override $TAUROS_DATA")
    args = parser.parse_args(argv)
    install = args.mode == "install"
    try:
        if shutil.which("docker") is None:
            raise SetupError("docker is required (see README External sources)")
        manifest = load_manifest()
        data = data_dir(args.data)
        tag = image_tag(manifest)
        changes = []
        if ensure_image(manifest, tag, install):
            changes.append(f"built image {tag}")
        if subprocess.run(["docker", "image", "inspect", tag], capture_output=True).returncode:
            raise SetupError(f"image {tag} missing; run install")
        print(f"image {tag}")
        for name, version in tool_versions(tag).items():
            print(f"  {name}: {version}")
        for engine in manifest["engines"]:
            clone, base = engine_dirs(data, engine)
            if ensure_clone(engine, clone, install):
                changes.append(f"cloned {engine['name']} {engine['ref']}")
            if build_base(engine, clone, base, tag, install):
                changes.append(f"built base ROM for {engine['name']} {engine['ref']}")
            record = read_record(base)
            print(f"engine {engine['name']} {engine['ref']} {engine['commit'][:12]} (clean, push disabled)")
            print(f"  base rom sha256 {record['rom_sha256']}")
            print(f"  base sym sha256 {record['sym_sha256']}")
        for source in manifest.get("sources", []):
            clone = source_dir(data, source)
            if ensure_clone(source, clone, install):
                changes.append(f"cloned {source['name']} {source['ref']}")
            print(f"source {source['name']} {source['ref']} {source['commit'][:12]} "
                  "(clean, push disabled)")
        print("changes: " + (", ".join(changes) if changes else "none"))
        return 0
    except (OSError, SetupError, subprocess.CalledProcessError, json.JSONDecodeError) as exc:
        print(f"Toolchain error: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
