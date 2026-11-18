#!/usr/bin/env python3
"""Refuse commits that would publish private data.

Checks the files staged for commit (default) or every tracked file (--all) for:
  - game data and binaries that must stay on the user's machine (ROMs, saves, savestates);
  - credentials (private keys, API tokens, .env files) and oversized files;
  - absolute home-directory paths;
  - git submodules and nested repositories (external sources live outside the repo);
  - literal strings listed, one per line, in the untracked file .git/info/private-patterns
    (for personal names, emails or hostnames that must never appear in the public repo).
It also requires the commit author email to be a GitHub noreply address.
Python 3.10+, standard library only.
"""
from __future__ import annotations

import argparse
from pathlib import Path
import re
import subprocess
import sys

MAX_BYTES = 2 * 1024 * 1024
FORBIDDEN_SUFFIXES = {
    ".gba", ".gb", ".gbc", ".nds", ".3ds", ".srl", ".rom", ".bin", ".elf", ".sav", ".srm",
    ".sgm", ".dsv", ".ips", ".ups", ".bps", ".xdelta", ".pem", ".key", ".p12", ".pfx",
}
FORBIDDEN_NAMES = re.compile(r"(^|/)(\.gitmodules|\.env(\..*)?|id_(rsa|ed25519|ecdsa)|credentials\.json|.*\.ss[0-9])$")
SECRETS = {
    "private key": re.compile(r"-----BEGIN [A-Z ]*PRIVATE KEY-----"),
    "Anthropic API key": re.compile(r"sk-ant-[A-Za-z0-9_-]{10,}"),
    "OpenAI API key": re.compile(r"\bsk-(?:proj-)?[A-Za-z0-9]{20,}"),
    "GitHub token": re.compile(r"\b(?:gh[pousr]_[A-Za-z0-9]{30,}|github_pat_[A-Za-z0-9_]{30,})"),
    "AWS access key": re.compile(r"\bAKIA[0-9A-Z]{16}\b"),
    "home directory path": re.compile(r"(?:/home/|/Users/|[A-Za-z]:\\Users\\)[A-Za-z0-9._-]+"),
}
NOREPLY = re.compile(r"^[0-9]+\+[A-Za-z0-9-]+@users\.noreply\.github\.com$|^[A-Za-z0-9-]+@users\.noreply\.github\.com$")


def git(*args: str, root: Path) -> bytes:
    return subprocess.run(["git", *args], cwd=root, check=True, capture_output=True).stdout


def local_patterns(root: Path) -> list[str]:
    path = Path(git("rev-parse", "--git-path", "info/private-patterns", root=root).decode().strip())
    path = path if path.is_absolute() else root / path
    if not path.exists():
        return []
    lines = path.read_text(encoding="utf-8").splitlines()
    return [line.strip() for line in lines if line.strip() and not line.lstrip().startswith("#")]


def gitlinks(root: Path) -> list[str]:
    """Index entries with mode 160000: submodules or embedded repositories."""
    entries = git("ls-files", "-s", "-z", root=root).decode().split("\0")
    return [entry.split("\t", 1)[1] for entry in entries if entry.startswith("160000 ")]


def scan(name: str, data: bytes, patterns: list[str]) -> list[str]:
    problems = []
    suffix = Path(name).suffix.lower()
    if suffix in FORBIDDEN_SUFFIXES or FORBIDDEN_NAMES.search(name):
        problems.append(f"{name}: file type must never be committed")
    if len(data) > MAX_BYTES:
        problems.append(f"{name}: {len(data)} bytes exceeds {MAX_BYTES}; large binaries stay local")
    text = data.decode("utf-8", errors="ignore")
    for label, pattern in SECRETS.items():
        if pattern.search(text):
            problems.append(f"{name}: contains a {label}")
    lowered = text.lower()
    for literal in patterns:
        if literal.lower() in lowered or literal.lower() in name.lower():
            problems.append(f"{name}: contains a string from .git/info/private-patterns")
    return problems


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--all", action="store_true", help="scan every tracked file instead of the index")
    args = parser.parse_args(argv)
    try:
        root = Path(git("rev-parse", "--show-toplevel", root=Path.cwd()).decode().strip())
        patterns = local_patterns(root)
        if args.all:
            names = git("ls-files", "-z", root=root).decode().split("\0")
        else:
            names = git("diff", "--cached", "--name-only", "--diff-filter=ACMR", "-z", root=root).decode().split("\0")
        nested = gitlinks(root)
        problems = [f"{name}: nested repository or submodule; external sources live under $TAUROS_DATA"
                    for name in nested]
        for name in filter(lambda n: n and n not in nested, names):
            problems += scan(name, git("show", f":{name}", root=root), patterns)
        email = git("config", "user.email", root=root).decode().strip()
    except (OSError, subprocess.CalledProcessError) as exc:
        print(f"Privacy check error: {exc}", file=sys.stderr)
        return 2
    if not NOREPLY.match(email):
        problems.append("git user.email is not a GitHub noreply address; it would be published in commits")
    for problem in problems:
        print(f"Privacy check: {problem}", file=sys.stderr)
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
