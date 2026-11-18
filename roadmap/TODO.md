# Tauros roadmap

This is the only authoritative ledger of activities, scope, status, responsibility,
assignment and dependencies for the all-in-one Pokémon ROM hack editor.
Requirements come from the owner's specification
[romhack-editor-reuse-plan.md](../romhack-editor-reuse-plan.md), cited in Scope as
`plan §"Section"`; it is linked, never copied. Verified facts, accepted decisions and
contracts live in [docs/master.md](docs/master.md). The derived
[task-graph.json](task-graph.json) is never edited by hand.

## Working contract

- Responsible is the intended executor: `Agent` (a coding agent such as Codex or
  Claude Code) or `Owner` (Simone, the project owner). Owner is the current claim:
  `unassigned` until claimed, then the claimant's exact name (for example `Codex`,
  `Claude Code`, `Simone`).
- Ready means Not started with every prerequisite Done. `--ready` lists only ready
  activities whose Owner is unassigned. The graph is not a lock: reread Owner, claim
  here, regenerate and validate before working.
- In progress needs an Owner and `Note: (owner: <Owner>, <current activity>)`.
  Done needs `[x]`, an Owner and `Verification:` starting with a UTC date, listing the
  checks actually performed, evidence and limits for the whole Scope and Completion.
  Expected results and unexecuted tests are not Verification.
- Dependencies are real prerequisites, not priority or reading order. Independent
  ready tasks may proceed in parallel.
- Each field is one line; a blank line ends a task. Every backtick-quoted task ID in
  this file must exist (active or retired). Format examples belong in code fences.
- Record accepted requirements and newly needed work here before changing the affected
  implementation. After a claim, scope/dependency change, completion, handoff and
  before commits run `python3 scripts/check_roadmap.py --write-graph` then
  `python3 scripts/check_roadmap.py`.
- IDs are permanent: never renumber or reuse. Removed or merged work moves to Retired
  IDs; retirement is not completion, so rewrite dependants explicitly.

```markdown
- [ ] **Not started** **CORE-001** Concrete activity title.
  Depends on: none.
  Responsible: Agent
  Owner: unassigned
  Scope: Work boundaries, plan §"Section" references, constraints and exclusions.
  Completion: Observable, objectively verifiable acceptance criteria.

- **OLD-001** Retired. Reason: explanation. Replacement: `CORE-004`.
```

## Governance and roadmap

- [x] **Done** **GOV-001** Establish the authoritative roadmap, deterministic checker, tests and agent workflow.
  Depends on: none.
  Responsible: Agent
  Owner: Claude Code
  Scope: Create roadmap/TODO.md from plan (all sections), roadmap/docs/master.md, scripts/check_roadmap.py (validate, --write-graph, --ready, --layers, --todo; Python 3.10+ stdlib), tests/test_roadmap.py and the AGENTS.md workflow section. Excludes starting any product task and creating a Makefile/CI (none exist yet).
  Completion: The checker accepts this TODO and rejects every invalid case listed in the request; tests pass; task-graph.json is current; every plan section maps to tasks in Specification coverage.
  Verification: 2026-11-18 UTC: `python3 -m unittest discover -s tests -v` ran 34 tests, all OK, including repository checks that every Scope cites existing plan headings, every plan H2 section is in Specification coverage and every task is covered; 15 deliberate validator mutations (In progress note, Verification-only-on-Done, unknown references, retired dependency, replacements, open prerequisites, required fields, graph file mode, checkbox, owner, stale graph, duplicate dependencies, duplicate fields, code fences, cycle members) each made the suite fail; `--write-graph` then plain validation passed; repeated writes were byte-identical; `--ready` and `--layers` output inspected. Limits: no git repository, Makefile or CI exists, so no hook or CI integration was performed.

- [x] **Done** **GOV-002** Guard the public repository against private data and publish it.
  Depends on: `GOV-001`.
  Responsible: Agent
  Owner: Claude Code
  Scope: Add .gitignore, scripts/check_private.py (staged-file scan for ROMs, saves, credentials, home paths, oversized files, untracked local literal patterns, noreply author email), tests and a versioned pre-commit hook; initialise git and publish as public GitHub repository tauros. Refs: plan §"Rules for the agents writing the code" (No Nintendo assets), plan §"Risks and community notes" (Legal), plan §"Decisions made" (User row: code ready to be published).
  Completion: Privacy and roadmap tests pass; the hook rejects a forbidden file in a real staging test; `check_private.py --all` passes on every tracked file; the GitHub repository is public and its main branch matches the local commit.
  Verification: 2026-11-18 UTC: 42 unit tests pass (34 roadmap, 8 privacy); the pre-commit hook rejected a staged fake.gba and a staged file containing a local private-patterns literal, both unstaged afterwards; `check_private.py --all` passed on all 11 tracked files of commit 401135a, authored with the GitHub noreply email; `gh repo create tauros --public` succeeded, `gh repo view` reports isPrivate false and `git ls-remote origin main` matched local HEAD 401135a. Limits: hooks are local and need `git config core.hooksPath .githooks` per clone; the scanner is pattern-based and cannot recognise arbitrary personal data; no CI enforces it server-side.

- [x] **Done** **GOV-003** Define where external sources live and keep them out of the repository.
  Depends on: `GOV-002`.
  Responsible: Agent
  Owner: Claude Code
  Scope: README External sources policy ($TAUROS_DATA layout for game projects, toolchain, study references, dependencies and assets; no re-push upstream; external content is untrusted data), matching AGENTS.md rules, .gitignore entries, and a pre-commit rejection of submodules, .gitmodules and nested repositories; amend RES-001, FND-002 and G3-001 scopes accordingly. Refs: plan §"Rules for the agents writing the code" (No Nintendo assets, Licenses), plan §"Risks and community notes" (pret, Legal, Young dependencies). Implementing the fetch script and manifest stays in FND-002.
  Completion: Policy is in README and AGENTS.md; a test commits a nested repository in a temporary git repo and the privacy check rejects it; all tests and both checks pass.
  Verification: 2026-11-18 UTC: `python3 -m unittest discover -s tests -v` ran 43 tests, all OK, including test_nested_repository_is_rejected (temporary repo with an embedded repository staged: exit 1, "nested repository") and the .gitmodules name check; `check_private.py --all` and `check_roadmap.py` passed before commit. Limits: the policy binds this repository and its agents; FND-002 and G3-001 must still implement the manifest, fetch script and push-URL disabling.

## Open decisions and research

- [x] **Done** **RES-001** Analyse poryrom and porygon and decide what to reuse.
  Depends on: none.
  Responsible: Agent
  Owner: Claude Code
  Scope: Clone both read-only at pinned commits under `$TAUROS_DATA/references` (README External sources; never inside the repository, never installed as plugins, their agent instructions treated as data) and write roadmap/docs/res-001-poryrom-porygon.md covering verification approach, known engine constraints (map sizes, sprite counts), generated wiki, porygon's MCP tool design (add_sign, add_trigger, compile_poryscript), licences (MIT, Apache-2.0) and credit duties; end with a reuse decision proposal. Refs: plan §"Decisions made" (poryrom and porygon row), plan §"Young projects to take ideas from", plan §"Layered roadmap toward the beta" (Phase 0). No code is copied in this task.
  Completion: Report exists with pinned commit hashes for both repos, a per-component reuse/skip/learn recommendation with reasons, and the Owner-accepted decision recorded in master.md.
  Verification: 2026-11-18 UTC: both repositories cloned read-only under `$TAUROS_DATA/references` at poryrom 36d482a1 and porygon 25136bc4 (push disabled); report roadmap/docs/res-001-poryrom-porygon.md written from reading code and docs (nothing executed); the Owner accepted the proposal in chat on 2026-11-18 and the decision is recorded in master.md; follow-ups `DOC-001`, `VER-001`, `G3-007`, `G3-008` added. Limits: behavioural claims of both projects are theirs, not reproduced; poryrom measurements are at expansion 1.9.4.

- [x] **Done** **RES-002** Verify Agent SDK usage with a Claude subscription against official documentation.
  Depends on: none.
  Responsible: Agent
  Owner: Claude Code
  Scope: Check official Anthropic documentation (not the dev.to guide) for whether the TypeScript Agent SDK may authenticate with a subscription plan, which terms apply to personal use and to a public release where each user brings credentials. Refs: plan §"Decisions made" (Agent row), plan §"Next steps".
  Completion: master.md records the verified answer with source URLs and access date, or explicitly records that it is unverifiable; if the assumption fails, a follow-up task for the alternative auth path is added here.
  Verification: 2026-11-18 UTC: read the official Agent SDK overview, Legal and compliance, Authentication pages (code.claude.com) and two support.claude.com articles on Agent SDK plan usage and monthly API credits; findings with URLs and access date recorded in master.md "Agent SDK authentication (RES-002)". The plan's assumption fails for a public release (claude.ai login not permitted for third-party apps without approval), so follow-up `AGT-005` was added and `AGT-002` scope amended. Limits: documentation reading only, no SDK run; the owner's plan tier is unknown; policy may change.

- [x] **Done** **RES-003** Verify which OpenAI (ChatGPT/Codex) and Google (Gemini/Antigravity) agent runtimes Tauros can connect to.
  Depends on: `RES-002`.
  Responsible: Agent
  Owner: Claude Code
  Scope: Extend the RES-002 method to OpenAI and Google using official documentation only: embeddable agent SDKs or CLIs usable from a sidecar, authentication (subscription sign-in versus API key), terms for personal use and for third-party apps, MCP client support for connecting to the Tauros MCP, and fit with the swappable agent bridge. Refs: plan §"Decisions made" (Agent row, Swappable agent row). Excludes choosing providers or implementing adapters.
  Completion: master.md records, per provider, the verified answer with source URLs and access date, or states what could not be verified; any adapter work found to be needed is added here as tasks.
  Verification: 2026-11-18 UTC: read official OpenAI pages (Codex auth, Codex SDK, Codex MCP, Sign in with ChatGPT overview, cookbook, Codex app-server and preview-limitations pages) and Google pages (Gemini CLI authentication and terms, Antigravity authentication, SDK overview and MCP); findings with URLs recorded in master.md "OpenAI and Google agent runtimes (RES-003)"; follow-ups `AGT-006`, `AGT-007` added and `MCP-001` Completion extended. Limits: documentation only, nothing run; some OpenAI pages returned 404/403; the Google third-party subscription restriction is sourced only from the Gemini CLI terms and forums; preview terms may change.

- [x] **Done** **DOC-001** Credit poryrom and porygon in the README.
  Depends on: `RES-001`.
  Responsible: Agent
  Owner: Claude Code
  Scope: Add a credits and prior-art section naming poryrom (MIT) and porygon (Apache-2.0), with links, pinned commits studied and what was learned; state that no code is copied. Refs: plan §"Rules for the agents writing the code" (Licenses), plan §"Risks and community notes" (Licenses).
  Completion: README section exists, links resolve, and its claims match master.md and the RES-001 report.
  Verification: 2026-11-18 UTC: README "Credits and prior art" added; both GitHub URLs returned HTTP 200; the stated commits match `git rev-parse` of the reference clones (36d482a, 25136bc); local links to the RES-001 report and master.md exist; licences and learned items match the report and the master.md decision. Limits: credits cover only these two projects; tool credits come with their integration tasks.

- [x] **Done** **VER-001** Design the Tauros verification-spec format.
  Depends on: `RES-001`.
  Responsible: Agent
  Owner: Claude Code
  Scope: Write docs/design/verification-specs.md: inert data specs, an assertion-free emulator driver with judgement outside it, mandatory counterfactuals, ROM and symbol table as one hashed pair, re-read (deref) of moving pointers, savestate-anchored RNG events asserted by outcome rather than replayed input, and a worked example for the director's scene. Learned from poryrom and porygon per the RES-001 report; no code copied. Refs: plan §"Layered roadmap toward the beta" (Phase 3 RAM-based checks), plan §"Young projects to take ideas from" (PokeMCP lesson), plan §"Rules for the agents writing the code" (Targeted tests).
  Completion: The document defines every spec field, check and counterfactual kind, rejection rules, and the director's-scene example with counterfactuals; every engine symbol it names is marked unverified until checked at our pin.
  Verification: 2026-11-18 UTC: docs/design/verification-specs.md written and reviewed against Completion: defines start, timeline, probes, assert and counterfactual fields, all check kinds and operators, four counterfactual kinds with the PROVEN/FAILED/INCONCLUSIVE/REJECTED verdicts, seven loader rejection rules, weak-assertion ranking, RNG policy steps, and the two-spec director's-scene example; every engine symbol is declared unverified pending G3-007; relative links resolve. Limits: design only, no implementation or run; symbol names and the no-whiteout mechanism are unconfirmed at our pin.

- [ ] **Not started** **DEC-001** Choose the project licence for a future open-source release.
  Depends on: none.
  Responsible: Owner
  Owner: unassigned
  Scope: Pick an OSI licence compatible with MPL-2.0 libmgba and with possible MIT (poryrom) and Apache-2.0 (porygon) reuse; decide attribution/NOTICE handling. Refs: plan §"Decisions made" (User row), plan §"Rules for the agents writing the code" (Licenses), plan §"Risks and community notes" (Licenses).
  Completion: Decision with rationale recorded in master.md; LICENSE text identified for FND-001 to add.

- [ ] **In progress** **DEC-002** Decide how to consume libmgba: mgba-rs as is, a fork, or own bindings, and the linking model.
  Depends on: none.
  Responsible: Agent
  Owner: Claude Code
  Scope: Evaluate mgba-rs 0.1 (single commit) coverage of framebuffer, audio, input, savestates, memory and Lua; compare forking against own bindings; choose static versus dynamic linking and document the MPL-2.0 source-availability obligation. Resolves the tension between plan §"Decisions made" (Core row, mgba-rs) and plan §"Young projects to take ideas from" (no young projects as dependencies) via an own interface. Refs: plan §"Risks and community notes" (Licenses, Young dependencies).
  Completion: master.md records the decision, pinned upstream revision, linking model and licence obligations; a prototype builds and runs a ROM-less init/teardown on the dev machine, with the command used.
  Note: (owner: Claude Code, evaluating mgba-rs, alternatives and linking model)

- [ ] **Not started** **DEC-003** Decide whether and how to label AI-generated versus hand-made content.
  Depends on: none.
  Responsible: Owner
  Owner: unassigned
  Scope: Decide on the provenance label suggested in plan §"Risks and community notes" (Positioning), considering PokéCommunity's AI ban and pret's AI-code policy; define what is recorded (scripts, wiring, text, art) and where. Decision only; implementation, if any, becomes a new task.
  Completion: Decision recorded in master.md; if adopted, an implementation task referencing this ID is added here.

## Phase 0 — Foundations

- [x] **Done** **FND-001** Create the Rust crate workspace with one crate per layer.
  Depends on: none.
  Responsible: Agent
  Owner: Claude Code
  Scope: Cargo workspace with crates model, connector-gen3, emu, mcp, agent-bridge, app (the Tauri app crate may be a placeholder until EMU-004); dependency direction follows the layer diagram (connectors depend on model; mcp on model and connector/emulator traits). Add .gitignore excluding decomp clones, ROMs, savestates and build output; add README skeleton. Refs: plan §"Layered roadmap toward the beta" (Phase 0), plan §"Layered architecture", plan §"Rules for the agents writing the code" (No Nintendo assets). Licence file is added once DEC-001 is decided, not here.
  Completion: `cargo build --workspace` and `cargo test --workspace` succeed; the crate dependency graph contains no upward edges (checked with `cargo tree`); no ROM or decomp file is tracked.
  Verification: 2026-11-18 UTC: Cargo workspace with crates tauros-model, -connector-gen3, -emu, -mcp, -agent-bridge and -app (placeholder binary; Tauri arrives with EMU-004), toolchain pinned to Rust 1.94.1, all crates publish = false; `cargo build --workspace` and `cargo test --workspace` succeed and `cargo run -p tauros-app` prints the version; `cargo tree --workspace` shows only downward edges; tests/test_layers.py enforces the allowed-dependency matrix via `cargo metadata` and failed as expected when a model→emu edge was added; `git ls-files` contains no ROM or decomp file and `target/` is ignored. Limits: crates contain only documentation until their tasks; no licence until DEC-001.

- [x] **Done** **FND-002** Toolchain setup script for devkitARM, Poryscript, Porytiles and the decomp base.
  Depends on: none.
  Responsible: Agent
  Owner: Claude Code
  Scope: Per master.md "GBA toolchain in Docker": a Dockerfile on devkitpro/devkitarm pinned by digest with libpng-dev, Poryscript and Porytiles at pinned versions verified by sha256; a versioned manifest toolchain/sources.json; and an idempotent stdlib script that builds the image, makes a shallow clone of pokeemerald-expansion pinned to `expansion/1.16.4` under `$TAUROS_DATA/engines` (push disabled, never built in place), and builds a base ROM from a `git archive` copy in a hardened container (no network, host user, all capabilities dropped, read-only root). Nothing is installed on the host and no sudo is needed. Refs: plan §"Layered roadmap toward the beta" (Phase 0), plan §"Decisions made" (User row: no installer or multi-OS), plan §"Mature tools to integrate as they are", plan §"Rules for the agents writing the code" (No Nintendo assets).
  Completion: A clean run reports tool versions and builds the base ROM with its .sym, recording their sha256; a second run changes nothing; the engine clone stays clean at the pinned commit; `git status` of this repository is unchanged by the run; pins are recorded in the manifest and master.md.
  Verification: 2026-11-18 UTC: `python3 scripts/setup_toolchain.py install` built image tauros-gba-toolchain:349743719ad7 (devkitARM GCC 15.2.0, Poryscript 3.6.1, Porytiles 2.0.0, libpng-dev 1.6.39-2+deb12u6, downloads verified by sha256 via ADD --checksum), shallow-cloned pokeemerald-expansion at 3796c72 with push DISABLED, and built the base ROM and .sym in one make invocation in the hardened container (one ELF link; pair check passed): rom sha256 2678195…, sym sha256 8fb8f2b…; a second install and `check` reported changes: none; engine clone clean; no root-owned files; this repository's git status unchanged by the run; the container resolved no DNS, ran as uid 1000 and had a read-only root. Two defects found and fixed during the task: the expansion refuses builds without git history (`.histignore` added to the archive copy) and a separate `make syms` relinked the ELF after the ROM. 54 unit tests pass. Limits: the ROM was built, not run (booting is G3-006/EMU work); apt packages inside the image are not version-pinned beyond the base digest; amd64 only.

## Phase 1 — Semantic model and Gen 3 connector

- [ ] **Not started** **MOD-001** Define the common semantic model and the decomp connector contract.
  Depends on: `FND-001`.
  Responsible: Agent
  Owner: unassigned
  Scope: Rust types for Trainer, Party, Map, Event, Scene, Species and Flag shared by every game, plus the connector trait (load, save, build, change-set) in crate model. Design must accommodate Gen 1-2 (.asm macros) and DS (JSON per trainer, NARC, 3D maps kept outside the shared map type) without implementing them. Refs: plan §"Layered architecture" (layer 3, layer 4), plan §"Beyond Gen 3: Gen 1-2 and DS".
  Completion: Model and trait compile with rustdoc; a written mapping table in master.md shows each type against Gen 3, Gen 1-2 and DS sources; a contract test suite exists that any connector can run.

- [ ] **Not started** **G3-001** Snapshot-before-write guard for game projects.
  Depends on: `FND-001`.
  Responsible: Agent
  Owner: unassigned
  Scope: Every connector write goes through a guard that creates a git commit in the user's game project first and refuses to write if the snapshot fails; never pushes or contributes upstream, and disables push URLs of the game project's remotes when it takes a project over. Refs: plan §"Rules for the agents writing the code" (Never destructive), plan §"Layered roadmap toward the beta" (Phase 1, automatic git commit), plan §"Risks and community notes" (pret).
  Completion: Tests show a write creates a commit before modification, a failed commit aborts the write leaving files unchanged, no remote operation is invoked, and a push attempt from a taken-over project fails.

- [ ] **Not started** **G3-002** Lossless read/write of trainers and parties in pokeemerald-expansion.
  Depends on: `MOD-001`, `G3-001`, `FND-002`.
  Responsible: Agent
  Owner: unassigned
  Scope: Parse and emit the expansion's trainer and party data into the model, preserving unrelated bytes and formatting. Refs: plan §"Layered roadmap toward the beta" (Phase 1), plan §"Rules for the agents writing the code" (Targeted tests, Compatible formats).
  Completion: Roundtrip test (read then write unchanged) is byte-identical on the pinned expansion; editing one trainer changes only that trainer's lines; the project still builds with `make`.

- [ ] **Not started** **G3-003** Lossless read/write of maps, layouts and events in Porymap format.
  Depends on: `MOD-001`, `G3-001`, `FND-002`.
  Responsible: Agent
  Owner: unassigned
  Scope: Reimplement reading and writing of Porymap's map JSON, layouts, metatiles and events (warps, NPCs, triggers, signs); do not embed Porymap. Refs: plan §"Mature tools to integrate as they are" (Porymap note), plan §"Layered roadmap toward the beta" (Phase 1), plan §"Rules for the agents writing the code" (Compatible formats).
  Completion: Byte-identical roundtrip over every map of the pinned expansion; an edited map opens in Porymap and the project builds with `make`.

- [ ] **Not started** **G3-004** Scripts via Poryscript and flag management.
  Depends on: `MOD-001`, `G3-001`, `FND-002`.
  Responsible: Agent
  Owner: unassigned
  Scope: Represent scenes as Poryscript sources compiled by the Poryscript CLI; read and allocate flags without colliding with existing ones. Refs: plan §"Layered roadmap toward the beta" (Phase 1), plan §"Mature tools to integrate as they are" (Poryscript).
  Completion: A generated scene compiles with Poryscript and builds; flag allocation is tested against the pinned expansion's existing flags; roundtrip of existing flags is lossless.

- [ ] **Not started** **G3-005** Incremental build and expansion test-runner wrapper.
  Depends on: `FND-002`, `MOD-001`.
  Responsible: Agent
  Owner: unassigned
  Scope: Invoke `make` incrementally inside the FND-002 toolchain container with the same hardened run options, with structured error capture, and run the expansion's headless C test runner with parsed results. Refs: plan §"Layered roadmap toward the beta" (Phase 1, incremental build), plan §"Mature tools to integrate as they are" (Expansion test runner).
  Completion: A no-change rebuild is measurably faster than a clean build (timings recorded); build errors and test results are returned as structured values in tests.

- [ ] **Not started** **G3-006** Phase 1 integration test and README update.
  Depends on: `G3-002`, `G3-003`, `G3-004`, `G3-005`.
  Responsible: Agent
  Owner: unassigned
  Scope: One integration test: open the pinned project, change a trainer from Rust, build, and confirm the ROM works; run the connector contract suite from MOD-001. Update README with what works and what is missing. Refs: plan §"Layered roadmap toward the beta" (Phase 1 done-when), plan §"Rules for the agents writing the code" (Targeted tests, End of phase).
  Completion: Integration and contract tests pass; the changed trainer is observed in the built ROM (standalone mGBA or test runner, method recorded); README updated.

- [x] **Done** **G3-007** Re-measure engine limits at our pinned expansion.
  Depends on: `FND-002`, `RES-001`.
  Responsible: Agent
  Owner: Claude Code
  Scope: Check against the pinned pokeemerald-expansion source the limits poryrom measured at 1.9.4 that the model must enforce: map-size ceiling, object-event slots and NPC count, dynamic sprite palettes, metatile-behaviour id space, shared tileset capacity, item-id renumbering and save layout hazards. Record each as confirmed, changed or not found, with file and line; runtime confirmation waits for the emulator. Refs: plan §"Layered roadmap toward the beta" (Phase 1), plan §"Young projects to take ideas from" (poryrom constraints). Credit poryrom.
  Completion: master.md lists each limit with value, source location at our pin and status; nothing from 1.9.4 is carried over unchecked.
  Verification: 2026-11-18 UTC: read the pinned expansion source at 3796c72 (fieldmap.h/.c, global.h, overworld config, sprite.c, event_object_movement.c, field_weather.c, metatile_behaviors.h, items.h, pokeball.h/.c, pokemon.h, save.h/.c) and scanned all 134 metatile_attributes.bin files and the general tileset PNG/metatiles; each limit recorded in master.md with value, file:line and status (6 confirmed, 3 changed, 1 new). Limits: static reading only, no runtime measurement; the NPC spawn-window behaviour and palette overflow rendering are inferred from code, not observed.

- [ ] **Not started** **G3-008** Static pre-build validation in the connector contract.
  Depends on: `MOD-001`, `G3-003`, `G3-007`.
  Responsible: Agent
  Owner: unassigned
  Scope: Add a validate step to the connector trait that rejects, before any write or build, changes violating the limits recorded by G3-007 (silent-failure classes first: map size, NPC slots, palette slots). Refs: plan §"Rules for the agents writing the code" (a bug that corrupts data is not acceptable), plan §"Layered architecture" (layer 4).
  Completion: Tests with fixtures just inside and just outside each limit pass and fail respectively; the pinned project's own maps validate without false positives (count recorded).

## Phase 2 — Emulator

- [ ] **Not started** **EMU-001** Emulator connector interface and libmgba core.
  Depends on: `DEC-002`, `FND-001`.
  Responsible: Agent
  Owner: unassigned
  Scope: Own emulator trait (so melonDS can implement it later) and an mGBA implementation per DEC-002: load ROM, run frames, framebuffer, input, audio. Refs: plan §"Layered roadmap toward the beta" (Phase 2), plan §"Layered architecture" (layer 5), plan §"Risks and community notes" (Young dependencies).
  Completion: Headless test boots a homebrew or user-built ROM, advances frames, captures a framebuffer and applies input; no Nintendo asset is committed.

- [ ] **Not started** **EMU-002** Savestates and RAM reading.
  Depends on: `EMU-001`.
  Responsible: Agent
  Owner: unassigned
  Scope: Save/load state and read memory regions through the emulator trait. Refs: plan §"Layered roadmap toward the beta" (Phase 2), plan §"Young projects to take ideas from" (PokeMCP lesson: test via RAM and savestates).
  Completion: Tests show a saved state restored produces an identical framebuffer and RAM snapshot; reads of known addresses match expected values.

- [ ] **Not started** **EMU-003** Boot straight into a chosen map or scene via warps and flags.
  Depends on: `EMU-002`, `G3-003`, `G3-004`.
  Responsible: Agent
  Owner: unassigned
  Scope: Gen 3 specific mechanism to start the ROM at a chosen map/position with chosen flags set, using connector map and flag data. Refs: plan §"Layered roadmap toward the beta" (Phase 2).
  Completion: Headless test boots into a named map and verifies location and flags from RAM.

- [ ] **Not started** **EMU-004** Minimal Tauri window showing the emulator.
  Depends on: `EMU-001`, `FND-001`.
  Responsible: Agent
  Owner: unassigned
  Scope: Tauri app shell rendering the framebuffer with keyboard input and audio; no editor features. Refs: plan §"Decisions made" (Core row), plan §"Layered roadmap toward the beta" (Phase 2).
  Completion: The app runs a ROM interactively with audio on the owner's Linux machine (manual check by the Owner recorded).

- [ ] **Not started** **EMU-005** Phase 2 integration test and README update.
  Depends on: `EMU-003`, `EMU-004`.
  Responsible: Agent
  Owner: unassigned
  Scope: The Tauri window shows the ROM and starts from a chosen point; one integration test for the boot-to-point path; README update. Refs: plan §"Layered roadmap toward the beta" (Phase 2 done-when), plan §"Rules for the agents writing the code" (End of phase).
  Completion: Integration test passes; Owner confirms the window starts at the chosen point; README updated.

## Phase 3 — MCP server

- [ ] **Not started** **MCP-001** rmcp server skeleton reachable by external clients.
  Depends on: `FND-001`.
  Responsible: Agent
  Owner: unassigned
  Scope: MCP server in Rust using rmcp, in the same process as model and connectors, exposed so external Claude Code and Codex can connect. Refs: plan §"Decisions made" (MCP server row, Agent row).
  Completion: A trivial tool is listed and callable over stdio from Claude Code, Codex and Gemini CLI or Antigravity (config snippets in the README), and from an MCP test client (commands recorded).

- [ ] **Not started** **MCP-002** Domain tools over the semantic model.
  Depends on: `MCP-001`, `G3-002`, `G3-003`, `G3-004`, `G3-005`, `G3-008`.
  Responsible: Agent
  Owner: unassigned
  Scope: create_trainer, place_npc, add_scene, build and run_test operating only on the common model and connector trait, with validation; no free-form file edits. Informed by RES-001 when available. Refs: plan §"Layered architecture" (layer 2), plan §"Layered roadmap toward the beta" (Phase 3).
  Completion: Each tool has a contract test against the Gen 3 connector; every write goes through the G3-001 snapshot guard (tested).

- [ ] **Not started** **MCP-003** Emulator tools and RAM-based checks.
  Depends on: `MCP-001`, `EMU-003`, `VER-001`.
  Responsible: Agent
  Owner: unassigned
  Scope: Tools to boot to a point, press input, take screenshots, save/load state and assert game state from RAM (e.g. battle outcome, flags, money). mgba-live-mcp and mcp-mgba are design references only. Refs: plan §"Layered roadmap toward the beta" (Phase 3), plan §"Young projects to take ideas from".
  Completion: Tests show RAM assertions detect a known state and its negation; tools are callable via MCP.

- [ ] **Not started** **MCP-004** Phase 3 end-to-end director's scene and README update.
  Depends on: `MCP-002`, `MCP-003`.
  Responsible: Agent
  Owner: unassigned
  Scope: Claude Code on the PC, connected to the MCP, builds the director's scene end to end: trainer with 3 Pokémon, lost battle without whiteout, RAM verification, preview. Refs: plan §"Layered roadmap toward the beta" (Phase 3 done-when), plan §"Rules for the agents writing the code" (End of phase).
  Completion: Recorded session transcript and RAM-check output show the scene built and verified; Owner confirms the preview; README updated.

## Phase 4 — Agent bridge and voice

- [ ] **Not started** **AGT-001** Swappable agent bridge interface.
  Depends on: `FND-001`.
  Responsible: Agent
  Owner: unassigned
  Scope: agent-bridge crate interface (send message, stream events, cancel) independent of vendor so an Agent SDK adapter now and Codex/API adapters later fit. Refs: plan §"Decisions made" (Swappable agent row).
  Completion: Interface documented and exercised by a fake adapter in tests.

- [ ] **Not started** **AGT-002** TypeScript Agent SDK sidecar connected to the internal MCP.
  Depends on: `AGT-001`, `RES-002`, `MCP-002`.
  Responsible: Agent
  Owner: unassigned
  Scope: Small TypeScript sidecar using the Agent SDK with the auth model verified in RES-002 (owner's personal use: the unmodified Claude Code binary's own sign-in; the app never collects or stores claude.ai credentials or tokens), connected to the in-process MCP, launched and supervised by the app. Refs: plan §"Decisions made" (Agent row), plan §"Layered roadmap toward the beta" (Phase 4).
  Completion: From a Rust test or the app, a prompt reaches the sidecar and a domain tool call is executed and streamed back.

- [ ] **Not started** **AGT-003** Streaming chat panel in the app.
  Depends on: `AGT-002`, `EMU-004`.
  Responsible: Agent
  Owner: unassigned
  Scope: Chat UI in the Tauri app with streamed responses and visible tool calls. Refs: plan §"Layered roadmap toward the beta" (Phase 4), plan §"Layered architecture" (layer 1).
  Completion: Owner sends a request in the app and sees streamed output and tool activity (manual check recorded).

- [ ] **Not started** **AGT-004** Microphone input with local transcription.
  Depends on: `AGT-003`.
  Responsible: Agent
  Owner: unassigned
  Scope: Local speech-to-text (whisper.cpp suggested; final engine chosen and recorded in master.md here) feeding text to the agent; no cloud transcription. Refs: plan §"Decisions made" (Voice row), plan §"Layered roadmap toward the beta" (Phase 4).
  Completion: Spoken request transcribed offline and delivered to the chat (manual check recorded); engine decision in master.md.

- [ ] **Not started** **AGT-005** API-key authentication path for a public release.
  Depends on: `AGT-002`.
  Responsible: Agent
  Owner: unassigned
  Scope: Per master.md "Agent SDK authentication (RES-002)", let each user supply their own Anthropic API key or cloud-provider credential (stored locally, never sent elsewhere), with no claude.ai login offered by the app and no "Claude Code" branding. Refs: plan §"Decisions made" (Agent row: each user needs their own credentials; User row: possible open-source release). Excludes any shared or bundled key.
  Completion: Tests show the sidecar authenticates with a user-provided API key and refuses to start without one in release mode; the key is absent from logs and from the repository; master.md records the release auth model.

- [ ] **Not started** **AGT-006** Codex adapter for the agent bridge.
  Depends on: `AGT-001`, `MCP-002`.
  Responsible: Agent
  Owner: unassigned
  Scope: Implement the agent-bridge interface over the Codex SDK or app-server, connected to the Tauros MCP, authenticating per master.md RES-003: Sign in with ChatGPT plan usage (preview; Plus/Pro; local or open-source app) or a user-supplied API key; the app never stores tokens outside the documented flow. Refs: plan §"Decisions made" (Swappable agent row, Agent row).
  Completion: A prompt sent through the bridge reaches Codex, a Tauros domain tool call executes and streams back; both auth paths tested or the untested one recorded as a limit.

- [ ] **Not started** **AGT-007** Gemini adapter for the agent bridge.
  Depends on: `AGT-001`, `MCP-002`.
  Responsible: Agent
  Owner: unassigned
  Scope: Implement the agent-bridge interface over a Gemini runtime (Antigravity SDK, Gemini API or Vertex AI, chosen and recorded) using only a user-supplied Gemini API key or Vertex credentials, never a consumer Google AI subscription, per master.md RES-003. Refs: plan §"Decisions made" (Swappable agent row).
  Completion: A prompt sent through the bridge reaches Gemini, a Tauros domain tool call executes and streams back; refusal without an API key or Vertex credentials is tested.

## Phase 5 — Full app (beta)

- [ ] **Not started** **APP-001** Map editor and viewer.
  Depends on: `G3-003`, `EMU-004`.
  Responsible: Agent
  Owner: unassigned
  Scope: View and edit maps, metatiles and events through the model; saves remain Porymap-compatible. Refs: plan §"Layered roadmap toward the beta" (Phase 5), plan §"Rules for the agents writing the code" (Compatible formats).
  Completion: An edit made in the app opens correctly in Porymap and builds.

- [ ] **Not started** **APP-002** Trainer and scene editor.
  Depends on: `G3-002`, `G3-004`, `EMU-004`.
  Responsible: Agent
  Owner: unassigned
  Scope: Forms for trainers, parties and Poryscript scenes through the model. Refs: plan §"Layered roadmap toward the beta" (Phase 5).
  Completion: Edits roundtrip and build; scene compiles with Poryscript.

- [ ] **Not started** **APP-003** Emulator panel integrated with the editor.
  Depends on: `EMU-003`, `EMU-004`, `G3-005`.
  Responsible: Agent
  Owner: unassigned
  Scope: Build-and-preview from the editor in the same window, boot to the selected map or scene. Refs: plan §"Layered roadmap toward the beta" (Phase 5), plan §"Layered architecture" (layer 1).
  Completion: From an open map, one action builds and boots there (manual check recorded).

- [ ] **Not started** **APP-004** Change history with undo.
  Depends on: `G3-001`, `EMU-004`.
  Responsible: Agent
  Owner: unassigned
  Scope: History view over the snapshot commits with undo/restore of agent and manual changes. Refs: plan §"Layered roadmap toward the beta" (Phase 5), plan §"Rules for the agents writing the code" (Never destructive).
  Completion: Tests show undo restores the previous snapshot exactly; the view lists changes with their origin.

- [ ] **Not started** **APP-005** Beta integration test and README update.
  Depends on: `APP-001`, `APP-002`, `APP-003`, `APP-004`, `AGT-003`, `AGT-004`, `MCP-004`.
  Responsible: Agent
  Owner: unassigned
  Scope: One end-to-end check covering map, trainer/scene editing, chat, voice, preview and undo; README documents beta capabilities and gaps. Refs: plan §"Layered roadmap toward the beta" (Phase 5 beta), plan §"Rules for the agents writing the code" (End of phase).
  Completion: Integration test passes; Owner confirms the beta flow; README updated.

## Phase 6 — Graphics

- [ ] **Not started** **GFX-001** Asset pipeline with palette and VRAM validator.
  Depends on: `FND-002`, `G3-003`.
  Responsible: Agent
  Owner: unassigned
  Scope: Pipeline source → conversion (Porypal for sprites, Porytiles for tilesets) → validator (dimensions, 16-colour palettes, 8x8 tiles, duplicate tiles, VRAM budget) before build. Porytiles cannot decompile animated tiles: report, do not attempt. Refs: plan §"Graphics assets and visual enhancements" (proposed pipeline), plan §"Layered roadmap toward the beta" (Phase 6).
  Completion: Validator tests catch each violation class on synthetic fixtures; a valid tileset compiles and builds.

- [ ] **Not started** **GFX-002** Pixel panel in the app.
  Depends on: `GFX-001`, `EMU-004`.
  Responsible: Agent
  Owner: unassigned
  Scope: Palette-aware pixel editing of sprites and tiles with validator feedback and build-and-preview. Refs: plan §"Graphics assets and visual enhancements" (step 4), plan §"Layered roadmap toward the beta" (Phase 6).
  Completion: Owner retouches an asset in the panel, it validates, builds and appears in the preview.

- [ ] **Not started** **GFX-003** Aseprite MCP integration for agent sketches.
  Depends on: `GFX-001`, `MCP-001`.
  Responsible: Agent
  Owner: unassigned
  Scope: Connect an Aseprite MCP (diivi/aseprite-mcp or pixel-mcp, chosen and recorded) so an agent sketches and the human retouches the same file, feeding the pipeline. Refs: plan §"Young projects to take ideas from" (Aseprite MCPs), plan §"Layered roadmap toward the beta" (Phase 6).
  Completion: An agent-created Aseprite file passes the validator and builds.

- [ ] **Not started** **GFX-004** Catalog of graphical upgrades.
  Depends on: `GFX-001`.
  Responsible: Agent
  Owner: unassigned
  Scope: Enable/customize upgrades as catalog entries (DS-style sprites, 80x80 sprites, animated battle sprites, shared tilesets with author credits, new weather effects on field_weather_effect.c); assets are fetched to the user's machine, never committed. Refs: plan §"Graphics assets and visual enhancements" (what already exists), plan §"Risks and community notes" (Legal).
  Completion: At least one entry per listed kind can be applied to the pinned project, builds and shows in preview; credits recorded per asset.

## Beyond Gen 3

- [ ] **Not started** **GB-001** Gen 1-2 connector format study.
  Depends on: `MOD-001`.
  Responsible: Agent
  Owner: unassigned
  Scope: Map pokered/pokecrystal .asm macro data and Polished Map/Tilemap Studio formats onto the semantic model; mGBA covers GB/GBC. Refs: plan §"Beyond Gen 3: Gen 1-2 and DS", plan §"Mature tools to integrate as they are" (Polished Map).
  Completion: Written mapping report with model gaps; any needed model change is added here as a task.

- [ ] **Not started** **GB-002** Gen 1-2 connector with lossless roundtrip.
  Depends on: `GB-001`, `G3-001`.
  Responsible: Agent
  Owner: unassigned
  Scope: Implement the connector trait for pokered/pokecrystal trainers and maps. Refs: plan §"Beyond Gen 3: Gen 1-2 and DS", plan §"Rules for the agents writing the code" (Targeted tests).
  Completion: Byte-identical roundtrip and the MOD-001 contract suite pass; the project builds with its own `make`.

- [ ] **Not started** **NDS-001** DS (Gen 4) connector and melonDS feasibility study.
  Depends on: `MOD-001`, `EMU-001`.
  Responsible: Agent
  Owner: unassigned
  Scope: Assess pokeplatinum/pokeheartgold data (per-trainer JSON, NARC), 3D maps needing a separate editor, and a melonDS implementation of the emulator trait. Built last. Refs: plan §"Beyond Gen 3: Gen 1-2 and DS", plan §"Layered architecture" (layers 4-5).
  Completion: Feasibility report with the proposed task breakdown added here.

## Specification coverage

Each H2 section of the plan maps to the tasks that satisfy it.

- plan §"Summary": `FND-002`, `RES-001`, `MOD-001`, `MCP-002`, `AGT-002`, `APP-005`.
- plan §"Layered architecture": `FND-001`, `MOD-001`, `MCP-002`, `EMU-001`, `APP-003`.
- plan §"Decisions made": `RES-001`, `RES-002`, `RES-003`, `DEC-001`, `DEC-002`, `FND-001`, `MCP-001`, `AGT-001`, `AGT-002`, `AGT-004`, `EMU-004`.
- plan §"Layered roadmap toward the beta": Phase 0 `FND-001`, `FND-002`, `RES-001`; Phase 1 `MOD-001`, `G3-001`, `G3-002`, `G3-003`, `G3-004`, `G3-005`, `G3-006`, `G3-007`, `G3-008`; Phase 3 design `VER-001`; Phase 2 `EMU-001`, `EMU-002`, `EMU-003`, `EMU-004`, `EMU-005`; Phase 3 `MCP-001`, `MCP-002`, `MCP-003`, `MCP-004`; Phase 4 `AGT-001`, `AGT-002`, `AGT-003`, `AGT-004`, `AGT-005`, `AGT-006`, `AGT-007`; Phase 5 `APP-001`, `APP-002`, `APP-003`, `APP-004`, `APP-005`; Phase 6 `GFX-001`, `GFX-002`, `GFX-003`, `GFX-004`.
- plan §"Rules for the agents writing the code": `GOV-002`, `G3-001`, `G3-002`, `G3-003`, `G3-006`, `EMU-005`, `MCP-004`, `APP-005`, `FND-001`, `DEC-001`.
- plan §"Mature tools to integrate as they are": `FND-002`, `G3-003`, `G3-004`, `G3-005`, `GB-001`.
- plan §"Young projects to take ideas from": `RES-001`, `DOC-001`, `VER-001`, `G3-007`, `DEC-002`, `MCP-003`, `GFX-003`.
- plan §"Graphics assets and visual enhancements": `GFX-001`, `GFX-002`, `GFX-004`.
- plan §"Beyond Gen 3: Gen 1-2 and DS": `MOD-001`, `GB-001`, `GB-002`, `NDS-001`.
- plan §"Risks and community notes": `GOV-003`, `DEC-001`, `DEC-002`, `DEC-003`, `G3-001`, `GFX-004`.
- plan §"Next steps": `GOV-001`, `FND-001`, `FND-002`, `RES-001`, `RES-002`.
- plan §"Sources": reference list only; cited by the tasks above, no own work.

## Retired IDs

None yet.
