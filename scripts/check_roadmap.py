#!/usr/bin/env python3
"""Validate the Tauros roadmap (roadmap/TODO.md) and its deterministic, derived task graph.

Modes:
  (default)      validate TODO.md and require task-graph.json to be current
  --write-graph  validate, then atomically regenerate task-graph.json
  --ready        list Not started, unblocked, unassigned activities
  --layers       list topological waves of the remaining (not Done) work
"""
from __future__ import annotations

import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import re
import sys
import tempfile

SCHEMA_VERSION = 1
ROOT = Path(__file__).resolve().parents[1]
DEFAULT_TODO = ROOT / "roadmap/TODO.md"
GRAPH_NAME = "task-graph.json"
STATUSES = ("Not started", "In progress", "Done")
REQUIRED = ("Depends on", "Responsible", "Owner", "Scope", "Completion")
OPTIONAL = ("Verification", "Note")
UNASSIGNED = "unassigned"

ID = r"[A-Z][A-Z0-9]*-[0-9]{3,}"
TASK = re.compile(rf"^- \[([ x])\] \*\*({'|'.join(STATUSES)})\*\* \*\*({ID})\*\* (\S.*)$")
RETIRED = re.compile(rf"^- \*\*({ID})\*\* Retired\. Reason: (\S.*) Replacement: (none|`{ID}`(?:, `{ID}`)*)\.$")
FIELD = re.compile(rf"^(?:  )?({'|'.join(REQUIRED + OPTIONAL)}): ?(.*)$")
LOOKS_LIKE_ENTRY = re.compile(rf"^\s*[-*+] \[|^\s*[-*+] \*\*(?:{'|'.join(STATUSES)}|{ID})\*\*")
LOOKS_LIKE_FIELD = re.compile(r"^\s+[A-Z][A-Za-z ]*:")
FENCE = re.compile(r"^(`{3,}|~{3,})")
ID_REFERENCE = re.compile(rf"`({ID})`")
NOTE = re.compile(r"^\(owner: ([^,()]*[^\s,()]), (\S[^()]*)\)\.?$")
VERIFICATION = re.compile(r"^(\d{4}-\d{2}-\d{2})(?:[ T]\d{2}:\d{2}(?::\d{2})?)? UTC\b.*\S")


class RoadmapError(ValueError):
    """An invalid TODO source or a missing/stale generated graph."""

    def __init__(self, errors: list[str] | str):
        self.errors = [errors] if isinstance(errors, str) else list(errors)
        super().__init__("\n".join(self.errors))


def parse_todo(raw: bytes) -> tuple[list[dict], list[dict], list[tuple[int, str]]]:
    """Return tasks, retired IDs and backtick ID references; collect every format error."""
    try:
        lines = raw.decode("utf-8").splitlines()
    except UnicodeDecodeError as exc:
        raise RoadmapError("TODO must be UTF-8") from exc
    errors: list[str] = []
    tasks: list[dict] = []
    retired: list[dict] = []
    references: list[tuple[int, str]] = []
    headings: dict[int, str] = {}
    current: dict | None = None
    fence: str | None = None
    for number, line in enumerate(lines, 1):
        marker = FENCE.match(line.strip())
        if marker:
            token = marker[1]
            if fence is None:
                fence = token
            elif token[0] == fence[0] and len(token) >= len(fence) and line.strip() == token:
                fence = None
            current = None
            continue
        if fence:
            continue
        heading = re.match(r"^(#{1,6}) (.+?)\s*#*\s*$", line)
        if heading:
            level = len(heading[1])
            headings = {k: v for k, v in headings.items() if k < level}
            headings[level] = heading[2]
            current = None
            continue
        section = " / ".join(headings[k] for k in sorted(headings))
        task = TASK.match(line)
        retirement = RETIRED.match(line)
        field = FIELD.match(line)
        if task:
            box, status, task_id, title = task.groups()
            if (box == "x") != (status == "Done"):
                errors.append(f"Line {number}: {task_id}: checkbox [{box}] contradicts status {status}")
            current = dict(id=task_id, status=status, title=title.strip(), section=section,
                           source_line=number, fields={})
            tasks.append(current)
        elif retirement:
            task_id, reason, replacements = retirement.groups()
            retired.append(dict(id=task_id, reason=reason, section=section, source_line=number,
                                replacements=re.findall(ID, replacements)))
            current = None
            continue  # replacements are validated separately, not as prose references
        elif LOOKS_LIKE_ENTRY.match(line):
            errors.append(f"Line {number}: malformed task or retired-ID entry")
            current = None
        elif field:
            key, value = field[1], field[2].strip()
            if current is None:
                errors.append(f"Line {number}: field {key} outside a task")
            elif not value:
                errors.append(f"Line {number}: {current['id']}: empty {key}")
            elif key in current["fields"]:
                errors.append(f"Line {number}: {current['id']}: duplicate {key}")
            else:
                current["fields"][key] = value
            if key == "Depends on":
                continue  # dependency IDs are validated as prerequisites
        elif current is not None and line.strip():
            kind = "unsupported field" if LOOKS_LIKE_FIELD.match(line) else "text inside a task"
            errors.append(f"Line {number}: {current['id']}: {kind}; use a supported single-line field")
        elif not line.strip():
            current = None
        references.extend((number, ref) for ref in ID_REFERENCE.findall(line))
    if fence:
        errors.append("Unclosed code fence")
    if not tasks and not errors:
        errors.append("TODO has no activities")
    if errors:
        raise RoadmapError(errors)
    return tasks, retired, references


def parse_dependencies(value: str) -> list[str]:
    if value == "none.":
        return []
    if not re.fullmatch(rf"`{ID}`(?:, `{ID}`)*\.", value):
        raise ValueError(f"invalid Depends on value {value!r}; use 'none.' or `ID`, `ID`.")
    return re.findall(ID, value)


def cycle_members(tasks: list[dict], pending: set[str]) -> list[str]:
    """Trim tasks that only lead into cycles, leaving the cycle members themselves."""
    remaining = set(pending)
    while True:
        sinks = {t["id"] for t in tasks if t["id"] in remaining
                 and not any(t["id"] in u["prerequisites"] for u in tasks if u["id"] in remaining)}
        if not sinks:
            return [t["id"] for t in tasks if t["id"] in remaining]
        remaining -= sinks


def build_graph(raw: bytes, todo_name: str = "TODO.md") -> dict:
    tasks, retired, references = parse_todo(raw)
    errors: list[str] = []
    seen: dict[str, int] = {}
    for item in tasks + retired:
        if item["id"] in seen:
            errors.append(f"Line {item['source_line']}: {item['id']} duplicates or reuses the ID "
                          f"on line {seen[item['id']]} (retired IDs are reserved)")
        else:
            seen[item["id"]] = item["source_line"]
    by_id = {t["id"]: t for t in tasks}
    retired_ids = {r["id"] for r in retired}

    for task in tasks:
        where = f"Line {task['source_line']}: {task['id']}"
        fields = task.pop("fields")
        for key in REQUIRED:
            if key not in fields:
                errors.append(f"{where}: missing {key}")
        try:
            task["prerequisites"] = parse_dependencies(fields.get("Depends on", "none."))
        except ValueError as exc:
            errors.append(f"{where}: {exc}")
            task["prerequisites"] = []
        owner = fields.get("Owner", UNASSIGNED)
        task["owner"] = None if owner == UNASSIGNED else owner
        task["responsible"] = fields.get("Responsible")
        task["scope"] = fields.get("Scope")
        task["completion"] = fields.get("Completion")
        task["verification"] = fields.get("Verification")
        task["note"] = fields.get("Note")

        deps = task["prerequisites"]
        if len(deps) != len(set(deps)):
            errors.append(f"{where}: duplicate prerequisites")
        for dep in dict.fromkeys(deps):
            if dep == task["id"]:
                errors.append(f"{where}: depends on itself")
            elif dep in retired_ids:
                errors.append(f"{where}: depends on retired {dep}; rewrite to its replacement or drop it")
            elif dep not in by_id:
                errors.append(f"{where}: depends on unknown {dep}")

        status = task["status"]
        if status in ("In progress", "Done") and task["owner"] is None:
            errors.append(f"{where}: {status} requires an assigned Owner")
        if status == "In progress":
            note = NOTE.match(task["note"] or "")
            if not note or note[1] != task["owner"]:
                errors.append(f"{where}: In progress requires Note '(owner: {task['owner'] or 'name'}, "
                              "current activity)' matching Owner")
        if status == "Done":
            if not task["verification"]:
                errors.append(f"{where}: Done requires Verification evidence")
        elif task["verification"]:
            errors.append(f"{where}: Verification is allowed only on Done tasks; record intent in Note")
        if task["verification"]:
            match = VERIFICATION.match(task["verification"])
            try:
                valid_date = bool(match) and bool(datetime.date.fromisoformat(match[1]))
            except ValueError:
                valid_date = False
            if not valid_date:
                errors.append(f"{where}: Verification must start with a UTC date "
                              "('YYYY-MM-DD[ HH:MM[:SS]] UTC ...') followed by evidence")

    for record in retired:
        where = f"Line {record['source_line']}: {record['id']}"
        replacements = record["replacements"]
        if len(replacements) != len(set(replacements)):
            errors.append(f"{where}: duplicate replacements")
        for replacement in dict.fromkeys(replacements):
            if replacement not in by_id:
                errors.append(f"{where}: replacement {replacement} is not an existing active task")

    for number, ref in references:
        if ref not in seen:
            errors.append(f"Line {number}: reference to unknown ID {ref}")

    if errors:
        raise RoadmapError(errors)

    # Kahn traversal over every task (Done included) detects all cycles; source order breaks ties.
    pending, visited = set(by_id), set()
    while pending:
        wave = [t["id"] for t in tasks if t["id"] in pending and set(t["prerequisites"]) <= visited]
        if not wave:
            raise RoadmapError("Dependency cycle among: " + ", ".join(cycle_members(tasks, pending)))
        visited.update(wave)
        pending.difference_update(wave)

    for task in tasks:
        task["incomplete_prerequisites"] = [d for d in task["prerequisites"] if by_id[d]["status"] != "Done"]
        task["dependents"] = [t["id"] for t in tasks if task["id"] in t["prerequisites"]]
        if task["status"] != "Not started" and task["incomplete_prerequisites"]:
            errors.append(f"Line {task['source_line']}: {task['id']}: {task['status']} task has incomplete "
                          "prerequisites " + ", ".join(task["incomplete_prerequisites"]))
    if errors:
        raise RoadmapError(errors)

    done = {t["id"] for t in tasks if t["status"] == "Done"}
    pending, layers = set(by_id) - done, []
    while pending:
        wave = [t["id"] for t in tasks if t["id"] in pending and set(t["prerequisites"]) <= done]
        layers.append(wave)
        done.update(wave)
        pending.difference_update(wave)

    ready = [t["id"] for t in tasks if t["status"] == "Not started" and not t["incomplete_prerequisites"]]
    return dict(
        schema_version=SCHEMA_VERSION,
        todo_path=todo_name,
        todo_sha256=hashlib.sha256(raw).hexdigest(),
        tasks=tasks,
        retired_ids=retired,
        ready_ids=ready,
        ready_unassigned_ids=[i for i in ready if by_id[i]["owner"] is None],
        active_ids=[t["id"] for t in tasks if t["status"] == "In progress"],
        topological_layers=layers,
    )


def serialize(graph: dict) -> bytes:
    return (json.dumps(graph, ensure_ascii=False, indent=2, sort_keys=True) + "\n").encode("utf-8")


def write_atomically(path: Path, data: bytes) -> None:
    with tempfile.NamedTemporaryFile(dir=path.parent, prefix=f".{path.name}.", delete=False) as handle:
        temporary = Path(handle.name)
        try:
            handle.write(data)
            handle.flush()
            os.fsync(handle.fileno())
        except BaseException:
            temporary.unlink(missing_ok=True)
            raise
    try:
        os.chmod(temporary, 0o644)
        temporary.replace(path)
    finally:
        temporary.unlink(missing_ok=True)


def check(todo: Path, write: bool = False) -> dict:
    graph_path = todo.with_name(GRAPH_NAME)
    raw = todo.read_bytes()
    graph = build_graph(raw, todo.name)
    expected = serialize(graph)
    if write:
        if todo.read_bytes() != raw:
            raise RoadmapError("TODO changed during generation; retry")
        write_atomically(graph_path, expected)
    elif not graph_path.exists():
        raise RoadmapError(f"Missing {GRAPH_NAME}; run --write-graph, then validate")
    elif graph_path.read_bytes() != expected:
        raise RoadmapError(f"Stale or manually edited {GRAPH_NAME}; run --write-graph, then validate")
    return graph


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--todo", type=Path, default=DEFAULT_TODO,
                        help="TODO source (default: roadmap/TODO.md); the graph is written beside it")
    modes = parser.add_mutually_exclusive_group()
    modes.add_argument("--write-graph", action="store_true", help="regenerate task-graph.json atomically")
    modes.add_argument("--ready", action="store_true", help="list ready, unassigned activities")
    modes.add_argument("--layers", action="store_true", help="list topological waves of remaining work")
    args = parser.parse_args(argv)
    try:
        graph = check(args.todo, args.write_graph)
    except (OSError, RoadmapError) as exc:
        errors = exc.errors if isinstance(exc, RoadmapError) else [str(exc)]
        for error in errors:
            print(f"Roadmap error: {error}", file=sys.stderr)
        return 2
    by_id = {t["id"]: t for t in graph["tasks"]}
    if args.ready:
        for task_id in graph["ready_unassigned_ids"]:
            task = by_id[task_id]
            print(f"{task_id} [{task['responsible']}] {task['title']}")
        if not graph["ready_unassigned_ids"]:
            print("No ready unassigned activities.")
    elif args.layers:
        for index, layer in enumerate(graph["topological_layers"], 1):
            print(f"Wave {index}: " + ", ".join(layer))
        if not graph["topological_layers"]:
            print("No incomplete activities.")
        else:
            print("Waves after the first are not executable until their prerequisites are Done.")
    else:
        state = "written" if args.write_graph else "current"
        print(f"Roadmap valid: {len(graph['tasks'])} activities, {len(graph['retired_ids'])} retired IDs, "
              f"{len(graph['ready_unassigned_ids'])} ready unassigned; graph {state}.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
