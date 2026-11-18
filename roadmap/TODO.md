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

- [ ] **Not started** **GOV-002** Guard the public repository against private data and publish it.
  Depends on: `GOV-001`.
  Responsible: Agent
  Owner: unassigned
  Scope: Add .gitignore, scripts/check_private.py (staged-file scan for ROMs, saves, credentials, home paths, oversized files, untracked local literal patterns, noreply author email), tests and a versioned pre-commit hook; initialise git and publish as public GitHub repository tauros. Refs: plan §"Rules for the agents writing the code" (No Nintendo assets), plan §"Risks and community notes" (Legal), plan §"Decisions made" (User row: code ready to be published).
  Completion: Privacy and roadmap tests pass; the hook rejects a forbidden file in a real staging test; `check_private.py --all` passes on every tracked file; the GitHub repository is public and its main branch matches the local commit.

## Open decisions and research

- [ ] **Not started** **RES-001** Analyse poryrom and porygon and decide what to reuse.
  Depends on: none.
  Responsible: Agent
  Owner: unassigned
  Scope: Clone both outside the repository and write roadmap/docs/res-001-poryrom-porygon.md covering verification approach, known engine constraints (map sizes, sprite counts), generated wiki, porygon's MCP tool design (add_sign, add_trigger, compile_poryscript), licences (MIT, Apache-2.0) and credit duties; end with a reuse decision proposal. Refs: plan §"Decisions made" (poryrom and porygon row), plan §"Young projects to take ideas from", plan §"Layered roadmap toward the beta" (Phase 0). No code is copied in this task.
  Completion: Report exists with pinned commit hashes for both repos, a per-component reuse/skip/learn recommendation with reasons, and the Owner-accepted decision recorded in master.md.

- [ ] **Not started** **RES-002** Verify Agent SDK usage with a Claude subscription against official documentation.
  Depends on: none.
  Responsible: Agent
  Owner: unassigned
  Scope: Check official Anthropic documentation (not the dev.to guide) for whether the TypeScript Agent SDK may authenticate with a subscription plan, which terms apply to personal use and to a public release where each user brings credentials. Refs: plan §"Decisions made" (Agent row), plan §"Next steps".
  Completion: master.md records the verified answer with source URLs and access date, or explicitly records that it is unverifiable; if the assumption fails, a follow-up task for the alternative auth path is added here.

- [ ] **Not started** **DEC-001** Choose the project licence for a future open-source release.
  Depends on: none.
  Responsible: Owner
  Owner: unassigned
  Scope: Pick an OSI licence compatible with MPL-2.0 libmgba and with possible MIT (poryrom) and Apache-2.0 (porygon) reuse; decide attribution/NOTICE handling. Refs: plan §"Decisions made" (User row), plan §"Rules for the agents writing the code" (Licenses), plan §"Risks and community notes" (Licenses).
  Completion: Decision with rationale recorded in master.md; LICENSE text identified for FND-001 to add.

- [ ] **Not started** **DEC-002** Decide how to consume libmgba: mgba-rs as is, a fork, or own bindings, and the linking model.
  Depends on: none.
  Responsible: Agent
  Owner: unassigned
  Scope: Evaluate mgba-rs 0.1 (single commit) coverage of framebuffer, audio, input, savestates, memory and Lua; compare forking against own bindings; choose static versus dynamic linking and document the MPL-2.0 source-availability obligation. Resolves the tension between plan §"Decisions made" (Core row, mgba-rs) and plan §"Young projects to take ideas from" (no young projects as dependencies) via an own interface. Refs: plan §"Risks and community notes" (Licenses, Young dependencies).
  Completion: master.md records the decision, pinned upstream revision, linking model and licence obligations; a prototype builds and runs a ROM-less init/teardown on the dev machine, with the command used.

- [ ] **Not started** **DEC-003** Decide whether and how to label AI-generated versus hand-made content.
  Depends on: none.
  Responsible: Owner
  Owner: unassigned
  Scope: Decide on the provenance label suggested in plan §"Risks and community notes" (Positioning), considering PokéCommunity's AI ban and pret's AI-code policy; define what is recorded (scripts, wiring, text, art) and where. Decision only; implementation, if any, becomes a new task.
  Completion: Decision recorded in master.md; if adopted, an implementation task referencing this ID is added here.

## Phase 0 — Foundations

- [ ] **Not started** **FND-001** Create the Rust crate workspace with one crate per layer.
  Depends on: none.
  Responsible: Agent
  Owner: unassigned
  Scope: Cargo workspace with crates model, connector-gen3, emu, mcp, agent-bridge, app (the Tauri app crate may be a placeholder until EMU-004); dependency direction follows the layer diagram (connectors depend on model; mcp on model and connector/emulator traits). Add .gitignore excluding decomp clones, ROMs, savestates and build output; add README skeleton. Refs: plan §"Layered roadmap toward the beta" (Phase 0), plan §"Layered architecture", plan §"Rules for the agents writing the code" (No Nintendo assets). Licence file is added once DEC-001 is decided, not here.
  Completion: `cargo build --workspace` and `cargo test --workspace` succeed; the crate dependency graph contains no upward edges (checked with `cargo tree`); no ROM or decomp file is tracked.

- [ ] **Not started** **FND-002** Toolchain setup script for devkitARM, Poryscript, Porytiles and the decomp base.
  Depends on: none.
  Responsible: Agent
  Owner: unassigned
  Scope: Idempotent script that installs or verifies devkitARM, Poryscript and Porytiles at pinned versions and clones pokeemerald-expansion (pinned 1.16.x tag) or pret/pokeemerald onto the user's machine outside the repository. Linux (the owner's machine) only. Refs: plan §"Layered roadmap toward the beta" (Phase 0), plan §"Decisions made" (User row: no installer or multi-OS), plan §"Mature tools to integrate as they are", plan §"Rules for the agents writing the code" (No Nintendo assets).
  Completion: On a clean run the script reports tool versions, a second run changes nothing, and `make` in the cloned expansion produces a ROM; the pinned versions are recorded in master.md.

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
  Scope: Every connector write goes through a guard that creates a git commit in the user's game project first and refuses to write if the snapshot fails; never pushes or contributes upstream. Refs: plan §"Rules for the agents writing the code" (Never destructive), plan §"Layered roadmap toward the beta" (Phase 1, automatic git commit), plan §"Risks and community notes" (pret).
  Completion: Tests show a write creates a commit before modification, a failed commit aborts the write leaving files unchanged, and no remote operation is invoked.

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
  Scope: Invoke `make` incrementally with structured error capture, and run the expansion's headless C test runner with parsed results. Refs: plan §"Layered roadmap toward the beta" (Phase 1, incremental build), plan §"Mature tools to integrate as they are" (Expansion test runner).
  Completion: A no-change rebuild is measurably faster than a clean build (timings recorded); build errors and test results are returned as structured values in tests.

- [ ] **Not started** **G3-006** Phase 1 integration test and README update.
  Depends on: `G3-002`, `G3-003`, `G3-004`, `G3-005`.
  Responsible: Agent
  Owner: unassigned
  Scope: One integration test: open the pinned project, change a trainer from Rust, build, and confirm the ROM works; run the connector contract suite from MOD-001. Update README with what works and what is missing. Refs: plan §"Layered roadmap toward the beta" (Phase 1 done-when), plan §"Rules for the agents writing the code" (Targeted tests, End of phase).
  Completion: Integration and contract tests pass; the changed trainer is observed in the built ROM (standalone mGBA or test runner, method recorded); README updated.

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
  Completion: A trivial tool is listed and callable from Claude Code and from an MCP test client (commands recorded).

- [ ] **Not started** **MCP-002** Domain tools over the semantic model.
  Depends on: `MCP-001`, `G3-002`, `G3-003`, `G3-004`, `G3-005`.
  Responsible: Agent
  Owner: unassigned
  Scope: create_trainer, place_npc, add_scene, build and run_test operating only on the common model and connector trait, with validation; no free-form file edits. Informed by RES-001 when available. Refs: plan §"Layered architecture" (layer 2), plan §"Layered roadmap toward the beta" (Phase 3).
  Completion: Each tool has a contract test against the Gen 3 connector; every write goes through the G3-001 snapshot guard (tested).

- [ ] **Not started** **MCP-003** Emulator tools and RAM-based checks.
  Depends on: `MCP-001`, `EMU-003`.
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
  Scope: Small TypeScript sidecar using the Agent SDK with the auth model verified in RES-002, connected to the in-process MCP, launched and supervised by the app. Refs: plan §"Decisions made" (Agent row), plan §"Layered roadmap toward the beta" (Phase 4).
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
- plan §"Decisions made": `RES-001`, `RES-002`, `DEC-001`, `DEC-002`, `FND-001`, `MCP-001`, `AGT-001`, `AGT-002`, `AGT-004`, `EMU-004`.
- plan §"Layered roadmap toward the beta": Phase 0 `FND-001`, `FND-002`, `RES-001`; Phase 1 `MOD-001`, `G3-001`, `G3-002`, `G3-003`, `G3-004`, `G3-005`, `G3-006`; Phase 2 `EMU-001`, `EMU-002`, `EMU-003`, `EMU-004`, `EMU-005`; Phase 3 `MCP-001`, `MCP-002`, `MCP-003`, `MCP-004`; Phase 4 `AGT-001`, `AGT-002`, `AGT-003`, `AGT-004`; Phase 5 `APP-001`, `APP-002`, `APP-003`, `APP-004`, `APP-005`; Phase 6 `GFX-001`, `GFX-002`, `GFX-003`, `GFX-004`.
- plan §"Rules for the agents writing the code": `GOV-002`, `G3-001`, `G3-002`, `G3-003`, `G3-006`, `EMU-005`, `MCP-004`, `APP-005`, `FND-001`, `DEC-001`.
- plan §"Mature tools to integrate as they are": `FND-002`, `G3-003`, `G3-004`, `G3-005`, `GB-001`.
- plan §"Young projects to take ideas from": `RES-001`, `DEC-002`, `MCP-003`, `GFX-003`.
- plan §"Graphics assets and visual enhancements": `GFX-001`, `GFX-002`, `GFX-004`.
- plan §"Beyond Gen 3: Gen 1-2 and DS": `MOD-001`, `GB-001`, `GB-002`, `NDS-001`.
- plan §"Risks and community notes": `DEC-001`, `DEC-002`, `DEC-003`, `G3-001`, `GFX-004`.
- plan §"Next steps": `GOV-001`, `FND-001`, `FND-002`, `RES-001`, `RES-002`.
- plan §"Sources": reference list only; cited by the tasks above, no own work.

## Retired IDs

None yet.
