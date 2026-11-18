"""Roadmap format, invariant, determinism and repository-coverage checks (stdlib unittest)."""
import contextlib
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import re
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts/check_roadmap.py"
SPEC = importlib.util.spec_from_file_location("check_roadmap", SCRIPT)
roadmap = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(roadmap)

REAL_TODO = ROOT / "roadmap/TODO.md"
PLAN = ROOT / "romhack-editor-reuse-plan.md"
EVIDENCE = "2026-11-18 UTC: fixture tests ran and passed."


def activity(task_id="CORE-001", status="Not started", deps="none.", owner="unassigned",
             note=None, verification=None, indent="  ", omit=()):
    box = "x" if status == "Done" else " "
    lines = [f"- [{box}] **{status}** **{task_id}** Expected result."]
    for key, value in (("Depends on", deps), ("Responsible", "Agent"), ("Owner", owner),
                       ("Scope", "Bounded work."), ("Completion", "Observable check."),
                       ("Note", note), ("Verification", verification)):
        if value is not None and key not in omit:
            lines.append(f"{indent}{key}: {value}")
    return "\n".join(lines) + "\n\n"


def done(task_id="CORE-001", deps="none."):
    return activity(task_id, "Done", deps, owner="Codex", verification=EVIDENCE)


def progress(task_id="CORE-001", deps="none."):
    return activity(task_id, "In progress", deps, owner="Codex", note="(owner: Codex, implementing)")


def graph(text):
    return roadmap.build_graph(text.encode())


class Parsing(unittest.TestCase):
    def test_exact_requested_format_and_metadata(self):
        text = "# Track\n\n## Core\n\n" + activity()
        result = graph(text)
        task = result["tasks"][0]
        self.assertEqual((task["source_line"], task["section"]), (5, "Track / Core"))
        self.assertEqual((task["title"], task["responsible"], task["owner"]),
                         ("Expected result.", "Agent", None))
        self.assertEqual((task["scope"], task["completion"]), ("Bounded work.", "Observable check."))
        self.assertEqual(result["schema_version"], 1)
        self.assertEqual(result["todo_sha256"], hashlib.sha256(text.encode()).hexdigest())

    def test_unindented_fields_are_accepted(self):
        self.assertEqual(graph(activity(indent=""))["tasks"][0]["scope"], "Bounded work.")

    def test_fenced_examples_are_ignored(self):
        for fence in ("```", "~~~~"):
            text = f"{fence}markdown\n" + activity() + "- [ ] broken\n" + f"{fence}\n" + activity("UI-001")
            with self.subTest(fence=fence):
                self.assertEqual([t["id"] for t in graph(text)["tasks"]], ["UI-001"])

    def test_empty_invalid_utf8_and_unclosed_fence(self):
        for raw in (b"# Empty\n", b"\xff", (activity() + "```\n").encode()):
            with self.subTest(raw=raw), self.assertRaises(roadmap.RoadmapError):
                roadmap.build_graph(raw)

    def test_missing_empty_and_duplicate_fields(self):
        cases = [activity(omit=(key,)) for key in roadmap.REQUIRED]
        cases += [activity().replace("Scope: Bounded work.", "Scope:"),
                  activity().replace("Scope: Bounded work.", "Scope:   "),
                  activity().rstrip("\n") + "\n  Scope: Again.\n"]
        for text in cases:
            with self.subTest(text=text), self.assertRaises(roadmap.RoadmapError):
                graph(text)

    def test_malformed_entries_and_stray_text(self):
        body = activity().rstrip("\n") + "\n"
        for tail in ("  Unsupported: lost scope\n", "Loose scope text\n", "   Scope: bad indent\n"):
            with self.subTest(tail=tail), self.assertRaises(roadmap.RoadmapError):
                graph(body + tail)
        for header in ("- [ ] **Blocked** **CORE-002** Result.\n", "- [X] **Done** **CORE-002** Result.\n",
                       "- [ ] **Not started** **bad-id** Result.\n", "- [ ] **Not started** **CORE-02** R.\n",
                       "- **OLD-001** Retired. Reason: x. Replacement: CORE-001.\n"):
            with self.subTest(header=header), self.assertRaises(roadmap.RoadmapError):
                graph(activity() + header)

    def test_field_outside_task_rejected(self):
        with self.assertRaisesRegex(roadmap.RoadmapError, "outside a task"):
            graph(activity() + "Scope: orphan.\n")

    def test_all_errors_are_reported_together(self):
        with self.assertRaises(roadmap.RoadmapError) as caught:
            graph(activity(omit=("Scope",)) + activity("UI-001", omit=("Completion",)))
        self.assertEqual(len(caught.exception.errors), 2)

    def test_unknown_backtick_reference_rejected_retired_reference_allowed(self):
        retired = "## Retired IDs\n\n- **OLD-001** Retired. Reason: Merged. Replacement: none.\n"
        self.assertEqual(len(graph("See `OLD-001`.\n\n" + activity() + retired)["tasks"]), 1)
        with self.assertRaisesRegex(roadmap.RoadmapError, "unknown ID GONE-001"):
            graph("See `GONE-001`.\n\n" + activity())


class Retirement(unittest.TestCase):
    BASE = activity() + "## Retired IDs\n\n- **OLD-001** Retired. Reason: Merged. Replacement: `CORE-001`.\n"

    def test_retired_record_is_indexed(self):
        record = graph(self.BASE)["retired_ids"][0]
        self.assertEqual((record["id"], record["reason"], record["replacements"]),
                         ("OLD-001", "Merged.", ["CORE-001"]))

    def test_retired_id_cannot_be_reused_or_depended_on(self):
        for suffix in ("\n" + activity("OLD-001"), "- **OLD-001** Retired. Reason: Again. Replacement: none.\n"):
            with self.subTest(suffix=suffix), self.assertRaisesRegex(roadmap.RoadmapError, "reuses"):
                graph(self.BASE + suffix)
        with self.assertRaisesRegex(roadmap.RoadmapError, "depends on retired OLD-001"):
            graph(self.BASE + "\n" + activity("UI-001", deps="`OLD-001`."))

    def test_invalid_replacements(self):
        for replacement in ("`MISSING-001`", "`OLD-002`", "`CORE-001`, `CORE-001`", "`OLD-001`"):
            text = self.BASE.replace("`CORE-001`.\n", f"{replacement}.\n")
            text += "- **OLD-002** Retired. Reason: Dropped. Replacement: none.\n"
            with self.subTest(replacement=replacement), self.assertRaises(roadmap.RoadmapError):
                graph(text)


class Dependencies(unittest.TestCase):
    def test_strict_dependency_syntax(self):
        for deps in ("", "none", "`CORE-002`", "`none`.", "CORE-002.", "`CORE-002` `UI-001`."):
            with self.subTest(deps=deps), self.assertRaises(roadmap.RoadmapError):
                graph(activity(deps=deps) + activity("CORE-002") + activity("UI-001"))

    def test_missing_duplicate_and_self_dependencies(self):
        for deps in ("`MISSING-001`.", "`CORE-001`.", "`UI-001`, `UI-001`."):
            with self.subTest(deps=deps), self.assertRaises(roadmap.RoadmapError):
                graph(activity(deps=deps) + activity("UI-001"))

    def test_cycles_including_done_cycles_report_members(self):
        for status in ("Not started", "Done"):
            text = activity("ROOT-001") + activity("TAIL-001", deps="`CORE-001`.")
            if status == "Done":
                text += done("CORE-001", "`UI-001`.") + done("UI-001", "`CORE-001`.")
            else:
                text += activity("CORE-001", deps="`UI-001`.") + activity("UI-001", deps="`CORE-001`.")
            with self.subTest(status=status), self.assertRaises(roadmap.RoadmapError) as caught:
                graph(text)
            self.assertIn("cycle among: CORE-001, UI-001", str(caught.exception))

    def test_diamond_layers_dependents_and_incomplete_prerequisites(self):
        text = done("BASE-001") + activity("LEFT-001", deps="`BASE-001`.")
        text += activity("RIGHT-001", deps="`BASE-001`.")
        text += activity("END-001", deps="`LEFT-001`, `RIGHT-001`.")
        result = graph(text)
        self.assertEqual(result["topological_layers"], [["LEFT-001", "RIGHT-001"], ["END-001"]])
        self.assertEqual(result["tasks"][0]["dependents"], ["LEFT-001", "RIGHT-001"])
        self.assertEqual(result["tasks"][-1]["incomplete_prerequisites"], ["LEFT-001", "RIGHT-001"])
        self.assertEqual(result["tasks"][-1]["prerequisites"], ["LEFT-001", "RIGHT-001"])

    def test_ready_excludes_claimed_active_done_and_blocked(self):
        text = done("DONE-001") + activity("FREE-001", deps="`DONE-001`.")
        text += activity("TAKEN-001", owner="Codex") + progress("ACTIVE-001")
        text += activity("BLOCKED-001", deps="`FREE-001`.")
        result = graph(text)
        self.assertEqual(result["ready_ids"], ["FREE-001", "TAKEN-001"])
        self.assertEqual(result["ready_unassigned_ids"], ["FREE-001"])
        self.assertEqual(result["active_ids"], ["ACTIVE-001"])
        self.assertEqual(result["topological_layers"],
                         [["FREE-001", "TAKEN-001", "ACTIVE-001"], ["BLOCKED-001"]])

    def test_all_done_has_no_layers_or_ready_tasks(self):
        result = graph(done())
        self.assertEqual((result["topological_layers"], result["ready_ids"]), ([], []))


class States(unittest.TestCase):
    def test_checkbox_status_must_agree(self):
        for text in (activity().replace("[ ]", "[x]"), done().replace("[x]", "[ ]")):
            with self.subTest(text=text), self.assertRaisesRegex(roadmap.RoadmapError, "checkbox"):
                graph(text)

    def test_in_progress_needs_owner_and_matching_note(self):
        for owner, note in (("unassigned", "(owner: unassigned, testing)"), ("Codex", None),
                            ("Codex", "(owner: Other, testing)"), ("Codex", "(owner: Codex, )"),
                            ("Codex", "owner: Codex, testing"), ("Codex", "(owner: Codex)")):
            with self.subTest(owner=owner, note=note), self.assertRaises(roadmap.RoadmapError):
                graph(activity(status="In progress", owner=owner, note=note))
        self.assertEqual(graph(activity(status="In progress", owner="Claude Code",
                                        note="(owner: Claude Code, writing tests)."))["active_ids"],
                         ["CORE-001"])

    def test_done_needs_owner_and_dated_utc_verification(self):
        for owner, verification in (("Codex", None), ("unassigned", EVIDENCE),
                                    ("Codex", "Tests should pass."), ("Codex", "2026-11-18 tests ran."),
                                    ("Codex", "2026-13-40 UTC: impossible date."),
                                    ("Codex", "2026-11-18 UTC")):
            with self.subTest(owner=owner, verification=verification), self.assertRaises(roadmap.RoadmapError):
                graph(activity(status="Done", owner=owner, verification=verification))
        self.assertEqual(graph(activity(status="Done", owner="Codex",
                                        verification="2026-11-18 09:30 UTC: ran suite."))["ready_ids"], [])

    def test_verification_only_on_done(self):
        for status in ("Not started", "In progress"):
            text = activity(status=status, owner="Codex", note="(owner: Codex, testing)", verification=EVIDENCE)
            with self.subTest(status=status), self.assertRaisesRegex(roadmap.RoadmapError, "only on Done"):
                graph(text)

    def test_active_or_done_cannot_depend_on_unfinished(self):
        for make in (progress, done):
            text = activity("BASE-001") + make("CORE-001", "`BASE-001`.")
            with self.subTest(make=make.__name__), self.assertRaisesRegex(roadmap.RoadmapError, "incomplete"):
                graph(text)


class FilesAndCLI(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        self.todo = Path(temp.name) / "TODO.md"
        self.todo.write_text(activity(), encoding="utf-8")
        self.index = self.todo.with_name("task-graph.json")

    def call(self, *args):
        output, errors = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(output), contextlib.redirect_stderr(errors):
            code = roadmap.main(["--todo", str(self.todo), *args])
        return code, output.getvalue(), errors.getvalue()

    def test_missing_then_write_then_current(self):
        self.assertEqual(self.call()[0], 2)
        self.assertEqual(self.call("--write-graph")[0], 0)
        code, out, _ = self.call()
        self.assertEqual(code, 0)
        self.assertIn("graph current", out)
        self.assertEqual(self.index.stat().st_mode & 0o777, 0o644)
        self.assertEqual(list(self.index.parent.glob(".task-graph.json.*")), [])

    def test_graph_content_and_hash(self):
        roadmap.check(self.todo, write=True)
        data = json.loads(self.index.read_text(encoding="utf-8"))
        self.assertEqual(data["todo_sha256"], hashlib.sha256(self.todo.read_bytes()).hexdigest())
        self.assertEqual(set(data), {"schema_version", "todo_path", "todo_sha256", "tasks", "retired_ids",
                                     "ready_ids", "ready_unassigned_ids", "active_ids", "topological_layers"})
        self.assertEqual(set(data["tasks"][0]),
                         {"id", "title", "status", "section", "source_line", "responsible", "owner", "scope",
                          "completion", "verification", "note", "prerequisites", "dependents",
                          "incomplete_prerequisites"})

    def test_stale_source_or_manually_edited_graph_detected(self):
        roadmap.check(self.todo, write=True)
        original = self.index.read_bytes()
        self.todo.write_bytes(self.todo.read_bytes() + b"\n")
        self.assertIn("Stale", self.call()[2])
        self.todo.write_text(activity(), encoding="utf-8")
        self.assertEqual(self.call()[0], 0)
        self.index.write_bytes(original.replace(b'"Not started"', b'"Done"'))
        self.assertEqual(self.call()[0], 2)

    def test_deterministic_and_invalid_source_never_overwrites(self):
        roadmap.check(self.todo, write=True)
        original = self.index.read_bytes()
        roadmap.check(self.todo, write=True)
        self.assertEqual(self.index.read_bytes(), original)
        self.assertEqual(roadmap.serialize(roadmap.build_graph(self.todo.read_bytes())), original)
        self.todo.write_text(activity() * 2, encoding="utf-8")
        code, _, errors = self.call("--write-graph")
        self.assertEqual(code, 2)
        self.assertIn("duplicates or reuses", errors)
        self.assertEqual(self.index.read_bytes(), original)

    def test_queries_require_current_graph_and_print_results(self):
        self.todo.write_text(done("BASE-001") + activity(deps="`BASE-001`.") +
                             activity("NEXT-001", deps="`CORE-001`."), encoding="utf-8")
        for mode in ("--ready", "--layers"):
            self.assertEqual(self.call(mode)[0], 2)
        roadmap.check(self.todo, write=True)
        self.assertEqual(self.call("--ready")[1], "CORE-001 [Agent] Expected result.\n")
        layers = self.call("--layers")[1]
        self.assertIn("Wave 1: CORE-001\nWave 2: NEXT-001\n", layers)
        self.assertIn("not executable", layers)

    def test_empty_query_results(self):
        self.todo.write_text(done(), encoding="utf-8")
        roadmap.check(self.todo, write=True)
        self.assertIn("No ready unassigned", self.call("--ready")[1])
        self.assertIn("No incomplete", self.call("--layers")[1])

    def test_missing_source_reports_controlled_error(self):
        self.todo.unlink()
        code, _, errors = self.call()
        self.assertEqual(code, 2)
        self.assertIn("Roadmap error", errors)


class Repository(unittest.TestCase):
    """The real roadmap stays valid, current and traceable to the specification."""

    @classmethod
    def setUpClass(cls):
        cls.graph = roadmap.build_graph(REAL_TODO.read_bytes())
        cls.todo_text = REAL_TODO.read_text(encoding="utf-8")
        cls.headings = set(re.findall(r"^#{2,6} (.+?)\s*$", PLAN.read_text(encoding="utf-8"), re.M))

    def test_committed_graph_is_current(self):
        expected = roadmap.serialize(self.graph)
        self.assertEqual(REAL_TODO.with_name("task-graph.json").read_bytes(), expected,
                         "run: python3 scripts/check_roadmap.py --write-graph")

    def test_every_scope_cites_existing_plan_sections(self):
        for task in self.graph["tasks"]:
            cited = re.findall(r'plan §"([^"]+)"', task["scope"])
            with self.subTest(task=task["id"]):
                self.assertTrue(cited or "plan (all sections)" in task["scope"], "Scope cites no plan section")
                self.assertLessEqual(set(cited), self.headings)

    def test_every_plan_section_is_covered(self):
        coverage = self.todo_text.split("## Specification coverage", 1)[1].split("\n## ", 1)[0]
        covered = set(re.findall(r'^- plan §"([^"]+)"', coverage, re.M))
        plan_sections = set(re.findall(r"^## (.+?)\s*$", PLAN.read_text(encoding="utf-8"), re.M))
        self.assertEqual(covered, plan_sections)

    def test_every_task_is_covered_by_some_section(self):
        coverage = self.todo_text.split("## Specification coverage", 1)[1].split("\n## ", 1)[0]
        listed = set(re.findall(rf"`({roadmap.ID})`", coverage))
        self.assertEqual({t["id"] for t in self.graph["tasks"]} - listed, set())


if __name__ == "__main__":
    unittest.main()
