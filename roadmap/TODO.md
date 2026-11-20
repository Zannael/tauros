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

- [x] **Done** **DEC-001** Choose the project licence for a future open-source release.
  Depends on: none.
  Responsible: Owner
  Owner: Simone
  Scope: Pick an OSI licence compatible with MPL-2.0 libmgba and with possible MIT (poryrom) and Apache-2.0 (porygon) reuse; decide attribution/NOTICE handling. Refs: plan §"Decisions made" (User row), plan §"Rules for the agents writing the code" (Licenses), plan §"Risks and community notes" (Licenses).
  Completion: Decision with rationale recorded in master.md; LICENSE text identified for FND-001 to add.
  Verification: 2026-11-18 UTC: the Owner chose MIT in chat ("for the community"); recorded in master.md; LICENSE (MIT, Tauros contributors) added, workspace and crates declare license = "MIT", README License section states third-party notice duties from DEC-002 and that hacks do not inherit the license. Limits: no third-party notices file yet; it is needed only with the first binary release.

- [x] **Done** **DEC-002** Decide how to consume libmgba: mgba-rs as is, a fork, or own bindings, and the linking model.
  Depends on: none.
  Responsible: Agent
  Owner: Claude Code
  Scope: Evaluate mgba-rs 0.1 (single commit) coverage of framebuffer, audio, input, savestates, memory and Lua; compare forking against own bindings; choose static versus dynamic linking and document the MPL-2.0 source-availability obligation. Resolves the tension between plan §"Decisions made" (Core row, mgba-rs) and plan §"Young projects to take ideas from" (no young projects as dependencies) via an own interface. Refs: plan §"Risks and community notes" (Licenses, Young dependencies).
  Completion: master.md records the decision, pinned upstream revision, linking model and licence obligations; a prototype builds and runs a ROM-less init/teardown on the dev machine, with the command used.
  Verification: 2026-11-18 UTC: mgba-rs cloned read-only at f8c4c81 and reviewed (vendored 0.11-dev without upstream commit, limited safe API); decision, pinned revision (mGBA 0.10.5 at 26b7884), static linking model and MPL-2.0/LGPL-2.1/BSD obligations recorded in master.md "libmgba consumption (DEC-002)"; prototype crate tauros-mgba-sys built libmgba from the pinned source; `cargo test -p tauros-mgba-sys` ran the ROM-less init/teardown test (pass), and the ignored test with TAUROS_BASE_ROM ran the base ROM 120 frames, read game code BPEE from 0x080000AC, saved and reloaded a savestate (pass). Limits: audio and input untested; no safe wrapper yet (EMU-001); amd64 Linux only.

- [x] **Done** **DEC-003** Decide whether and how to label AI-generated versus hand-made content.
  Depends on: none.
  Responsible: Owner
  Owner: Simone
  Scope: Decide on the provenance label suggested in plan §"Risks and community notes" (Positioning), considering PokéCommunity's AI ban and pret's AI-code policy; define what is recorded (scripts, wiring, text, art) and where. Decision only; implementation, if any, becomes a new task.
  Completion: Decision recorded in master.md; if adopted, an implementation task referencing this ID is added here.
  Verification: 2026-11-18 UTC: the Owner decided in chat to label everything AI may touch, with three classes defined by the Owner (AI-Generated, AI-Assisted, Human); decision and definitions recorded in master.md "Provenance labels (DEC-003)"; implementation split into `MOD-001` (provenance type and change-set field), `G3-001` (provenance in snapshot commits) and `PROV-001` (app display and export). Limits: the approval-only edge case and aggregation rule are recorded as interpretations for the Owner to confirm.

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

- [x] **Done** **MOD-001** Define the common semantic model and the decomp connector contract.
  Depends on: `FND-001`.
  Responsible: Agent
  Owner: Claude Code
  Scope: Rust types for Trainer, Party, Map, Event, Scene, Species and Flag shared by every game, plus the connector trait (load, save, build, change-set) in crate model. Every change set carries a provenance label (AI-Generated, AI-Assisted, Human) per `DEC-003`. Design must accommodate Gen 1-2 (.asm macros) and DS (JSON per trainer, NARC, 3D maps kept outside the shared map type) without implementing them. Refs: plan §"Layered architecture" (layer 3, layer 4), plan §"Beyond Gen 3: Gen 1-2 and DS".
  Completion: Model and trait compile with rustdoc; a written mapping table in master.md shows each type against Gen 3, Gen 1-2 and DS sources; a contract test suite exists that any connector can run.
  Verification: 2026-11-18 UTC: crates/model defines ids, Trainer/Party/PartyMember, Map/MapGeometry/Event/Connection, Scene, Species, Flag, GameInfo/Capabilities, ProjectSnapshot, ChangeSet with Provenance, Violation, the Connector trait, generic validation, an in-memory reference connector and the 8-check contract suite; `cargo test -p tauros-model` passes 9 tests (reference connector satisfies the contract; a deliberately lossy, unvalidated connector fails 6 named checks; validation rules; Gen 1 capabilities; provenance history; JSON round trip of change sets and snapshots); `RUSTDOCFLAGS=-D warnings cargo doc` and clippy are clean; mapping table recorded in master.md "Semantic model mapping (MOD-001)" with Gen 1-2/DS cells not opened marked to verify. Limits: no real connector runs the suite yet (G3-006); Gen 1-2/DS fit is checked against file listings and one DS trainer file only.

- [x] **Done** **G3-001** Snapshot-before-write guard for game projects.
  Depends on: `FND-001`.
  Responsible: Agent
  Owner: Claude Code
  Scope: Every connector write goes through a guard that creates a git commit in the user's game project first and refuses to write if the snapshot fails; never pushes or contributes upstream, and disables push URLs of the game project's remotes when it takes a project over. Each change commit records its provenance label (`DEC-003`) as a commit trailer. Refs: plan §"Rules for the agents writing the code" (Never destructive), plan §"Layered roadmap toward the beta" (Phase 1, automatic git commit), plan §"Risks and community notes" (pret).
  Completion: Tests show a write creates a commit before modification, a failed commit aborts the write leaving files unchanged, no remote operation is invoked, and a push attempt from a taken-over project fails.
  Verification: 2026-11-18 UTC: crates/connector-gen3/src/snapshot.rs implements GitGuard (take_over, snapshot, write with restore); `cargo test -p tauros-connector-gen3` passes 8 tests in temporary repositories: dirty work is committed before the write and the change commit's parent is that snapshot with Tauros-Provenance and Tauros-Snapshot trailers; a clean tree uses HEAD; a failing pre-commit hook aborts before the write runs and leaves files and history unchanged; a failing write restores tracked and new files while keeping ignored files; a no-op write records no commit; after take-over both remotes have the disabled push URL, push via remote and via direct URL both fail and the bare upstream receives no refs; the audited command log contains no network subcommand; an existing pre-push hook is preserved. Mutations removing the restore or the hook each failed one test. Limits: the guard lives in connector-gen3 for now (GB-002 will need it shared); a user's global core.hooksPath bypasses the installed hook, leaving the disabled push URL as the only barrier.

- [x] **Done** **G3-002** Lossless read/write of trainers and parties in pokeemerald-expansion.
  Depends on: `MOD-001`, `G3-001`, `FND-002`.
  Responsible: Agent
  Owner: Claude Code
  Scope: Parse and emit the expansion's trainer and party data into the model, preserving unrelated bytes and formatting. Refs: plan §"Layered roadmap toward the beta" (Phase 1), plan §"Rules for the agents writing the code" (Targeted tests, Compatible formats).
  Completion: Roundtrip test (read then write unchanged) is byte-identical on the pinned expansion; editing one trainer changes only that trainer's lines; the project still builds with `make`.
  Verification: 2026-11-18 UTC: `cargo test --workspace` (connector-gen3 tests pinned_formats, connector_contract, snapshot and unit tests) on `git archive` copies of the pinned expansion 3796c72, and `TAUROS_DOCKER_TESTS=1 cargo test -p tauros-connector-gen3 --test toolchain_build`. Roundtrip: all four .party files (trainers 855, trainers_frlg 624, battle_partners 2, debug_trainers 2 trainers) split and re-emitted, and every trainer rewritten unchanged, are byte-identical; every trainer removed and rewritten from the model alone reads back equal; every species, item, move, ability and AI-flag name resolves by trainerproc's own rules to a constant the project defines (Geodude to SPECIES_GEODUDE, Oran Berry to ITEM_ORAN_BERRY). Local edit: TRAINER_ROXANNE_1 level +1 and an Oran Berry changed exactly two lines (`Level: 12` to `13`, `Geodude` to `Geodude @ Oran Berry`), committed with `Tauros-Provenance: ai-assisted`; rewriting all 855 trainers unchanged touched no file and made no commit; `contract::assert_contract` passes. Build: the edited copy built with `make rom syms` in the hardened container and the generated src/data/trainers.h holds the new `.lvl`. Fixed on the way: the G3-001 guard now runs git with `core.autocrlf=false` (the owner's global `input` setting would have normalised CRLF palettes on snapshot and restore; regression test added and seen failing without the fix), and generic validation no longer re-checks an upsert identical to the current entity (TRAINER_NONE has no Pokémon). Limits: the connector exposes src/data/trainers.party only (the other .party files are roundtrip-tested, not loaded); new trainer constants are G3-009; blocks containing C comments are read-only; the trainer is not yet observed in the running ROM (G3-006).

- [x] **Done** **G3-003** Lossless read/write of maps, layouts and events in Porymap format.
  Depends on: `MOD-001`, `G3-001`, `FND-002`.
  Responsible: Agent
  Owner: Codex
  Scope: Reimplement reading and writing of Porymap's map JSON, layouts, metatiles and events (warps, NPCs, triggers, signs); do not embed Porymap. Refs: plan §"Mature tools to integrate as they are" (Porymap note), plan §"Layered roadmap toward the beta" (Phase 1), plan §"Rules for the agents writing the code" (Compatible formats).
  Completion: Byte-identical roundtrip over every map of the pinned expansion; an edited map opens in Porymap and the project builds with `make`.
  Verification: 2026-11-18 UTC: Codex reran `cargo test -p tauros-connector-gen3 --test pinned_formats every_map_and_layout_roundtrips -- --nocapture` and `cargo test -p tauros-connector-gen3 --test pinned_formats every_block_and_metatile_file_roundtrips -- --nocapture`: both passed against pinned expansion 3796c72, covering 939 maps, 785 layouts, 9058 events, 1570 map/border binary files and 134 tilesets with byte-identical roundtrips. Build evidence: the edited Route 101 project built with `make rom syms` in the hardened container in the earlier integration run recorded under G3-004/G3-005; that build was not rerun for this documentation update. Manual compatibility: the owner opened the prepared `porymap-check-g3-003` project (edit commit 5c7d71e9) in Porymap 6.3.1 from Flathub on Linux, choosing pokeemerald; the screenshot supplied in chat shows Route101 rendered and the added NPC selected at x=12, y=10, elevation=3, graphics OBJ_EVENT_GFX_BOY_1, movement MOVEMENT_TYPE_FACE_DOWN and script Route101_EventScript_Boy. Limits: GUI verification covers opening and inspecting this edited map, not a Porymap save/reopen or every map in the GUI; new maps and layout resizing remain G3-010.

- [x] **Done** **G3-004** Scripts via Poryscript and flag management.
  Depends on: `MOD-001`, `G3-001`, `FND-002`.
  Responsible: Agent
  Owner: Claude Code
  Scope: Represent scenes as Poryscript sources compiled by the Poryscript CLI; read and allocate flags without colliding with existing ones. Refs: plan §"Layered roadmap toward the beta" (Phase 1), plan §"Mature tools to integrate as they are" (Poryscript).
  Completion: A generated scene compiles with Poryscript and builds; flag allocation is tested against the pinned expansion's existing flags; roundtrip of existing flags is lossless.
  Verification: 2026-11-18 UTC: `cargo test --workspace` and `TAUROS_DOCKER_TESTS=1 cargo test -p tauros-connector-gen3 --test toolchain_build` on `git archive` copies of the pinned expansion. Scene: a Poryscript scene (Route101_EventScript_TaurosScene, using an allocated flag) was compiled by Poryscript 3.6.1 in the hardened container into data/maps/Route101/scenes/, included right after Route101's scripts.inc in data/event_scripts.s, wired to a new Route 101 NPC, and the project built with `make rom syms`; the label is in pokeemerald.sym and in the generated Route101 events.inc. A Poryscript syntax error comes back as `PORYSCRIPT ERROR: line 2: missing closing parenthesis for command 'msgbox'`. Assembly scenes: the 16 570 distinct global labels of map and shared scripts.inc files load as scenes; rewriting 16 569 of them unchanged (all but the label defined twice) touched no file; editing one changed one line; adding then removing a scene restored scripts.inc. Flags: flags.h's 2301 FLAG_ defines (Emerald branch) load with values evaluated from the C expressions; 1787 are referenced somewhere, 373 are FLAG_UNUSED_* placeholders (2 of them referenced). Allocating two flags took FLAG_UNUSED_0x020 and 0x021 (unreferenced, unshared, inside the 0x20..0x500 event range), rewrote exactly those two lines, left every other flag's value unchanged, and the allocated names read back as flags; a name already defined or referenced is rejected. Limits: existing scripts stay assembly scenes (no decompilation to Poryscript); compiled .inc files are written by Tauros at apply time and committed beside their .pory, not regenerated by `make`; the one label defined twice under `.if` (MtChimney_EventScript_BagIsFull) is read-only; scenes cannot move between maps; only FLAG_UNUSED_* placeholders are allocated (no flag-space growth).

- [x] **Done** **G3-005** Incremental build and expansion test-runner wrapper.
  Depends on: `FND-002`, `MOD-001`.
  Responsible: Agent
  Owner: Claude Code
  Scope: Invoke `make` incrementally inside the FND-002 toolchain container with the same hardened run options, with structured error capture, and run the expansion's headless C test runner with parsed results. Refs: plan §"Layered roadmap toward the beta" (Phase 1, incremental build), plan §"Mature tools to integrate as they are" (Expansion test runner).
  Completion: A no-change rebuild is measurably faster than a clean build (timings recorded); build errors and test results are returned as structured values in tests.
  Verification: 2026-11-18 UTC: `TAUROS_DOCKER_TESTS=1 cargo test -p tauros-connector-gen3 --test toolchain_build` and unit tests. Timings on a full `git archive` copy with 16 jobs: clean `make rom syms` 90.2 s, no-change rebuild 2.0 s; a second run on the final code 86.0 s and 2.0 s, producing the same edited ROM (sha256 f80a5b24…); an earlier manual probe 95.3 s and 2.1 s. Structured errors: a bad trainers.party key came back as file src/data/trainers.party, line 88 with trainerproc's message, a C syntax error as src/main.c:471:21, and make's own failure lines as Makefile or trainer_rules.mk diagnostics. Test runner: `make check TESTS='Trainer Party'` built the test ROM and ran mgba-rom-test-hydra in the container (68.5 s); 6 cases parsed, all PASS, totals PASSED 6 and TOTAL 6. The Rust hardened `docker run` arguments and image tag equal setup_toolchain.py's `hardened_run` and `image_tag` (test). Toolchain change: the expansion's bundled runner needs libelf.so.1, so libelf1 0.188-2.1 was added to the image (new tag tauros-gba-toolchain:d75267a55161); `setup_toolchain.py install` rebuilt the base ROM with the same sha256 as before. Limits: container tests run only with TAUROS_DOCKER_TESTS=1; only a filtered subset of the expansion's tests was run.

- [x] **Done** **G3-006** Phase 1 integration test and README update.
  Depends on: `G3-002`, `G3-003`, `G3-004`, `G3-005`.
  Responsible: Agent
  Owner: Claude Code
  Scope: One integration test: open the pinned project, change a trainer from Rust, build, and confirm the ROM works; run the connector contract suite from MOD-001. Update README with what works and what is missing. Refs: plan §"Layered roadmap toward the beta" (Phase 1 done-when), plan §"Rules for the agents writing the code" (Targeted tests, End of phase).
  Completion: Integration and contract tests pass; the changed trainer is observed in the built ROM (standalone mGBA or test runner, method recorded); README updated.
  Verification: 2026-11-18 UTC: `TAUROS_DOCKER_TESTS=1 cargo test -p tauros-connector-gen3 --test phase1_integration` passed (72.6 s): the MOD-001 `assert_contract` suite on fresh copies of pinned expansion 3796c72, then on a copy of the base build TRAINER_ROXANNE_1's lead raised from level 12 to 13 via `Gen3Connector::apply` (one file touched, committed), built with `make rom syms` in the hardened container. Method for observing the change in the ROM (byte-level test runner): the edited ROM has the base ROM's size and differs from it in exactly one byte, offset 0x479db2, 12 to 13, which lies in the first party member that `gTrainers[DIFFICULTY_NORMAL][TRAINER_ROXANNE_1]` (row address from the `.sym` symbol and header counts, 52-byte rows) points to. `cargo test --workspace` without containers and `python3 -m unittest discover -s tests` also passed. README now has a Status section (what works, what is missing) and the container test command. Limits: no emulator boot or battle was observed (EMU-001 not done, no standalone mGBA on this host); the first-member bound is heuristic (min of the row size and the distance to the next trainer's party).

- [x] **Done** **G3-007** Re-measure engine limits at our pinned expansion.
  Depends on: `FND-002`, `RES-001`.
  Responsible: Agent
  Owner: Claude Code
  Scope: Check against the pinned pokeemerald-expansion source the limits poryrom measured at 1.9.4 that the model must enforce: map-size ceiling, object-event slots and NPC count, dynamic sprite palettes, metatile-behaviour id space, shared tileset capacity, item-id renumbering and save layout hazards. Record each as confirmed, changed or not found, with file and line; runtime confirmation waits for the emulator. Refs: plan §"Layered roadmap toward the beta" (Phase 1), plan §"Young projects to take ideas from" (poryrom constraints). Credit poryrom.
  Completion: master.md lists each limit with value, source location at our pin and status; nothing from 1.9.4 is carried over unchecked.
  Verification: 2026-11-18 UTC: read the pinned expansion source at 3796c72 (fieldmap.h/.c, global.h, overworld config, sprite.c, event_object_movement.c, field_weather.c, metatile_behaviors.h, items.h, pokeball.h/.c, pokemon.h, save.h/.c) and scanned all 134 metatile_attributes.bin files and the general tileset PNG/metatiles; each limit recorded in master.md with value, file:line and status (6 confirmed, 3 changed, 1 new). Limits: static reading only, no runtime measurement; the NPC spawn-window behaviour and palette overflow rendering are inferred from code, not observed.

- [x] **Done** **G3-008** Static pre-build validation in the connector contract.
  Depends on: `MOD-001`, `G3-003`, `G3-007`.
  Responsible: Agent
  Owner: Codex
  Scope: Add a validate step to the connector trait that rejects, before any write or build, changes violating the limits recorded by G3-007 (silent-failure classes first: map size, NPC slots, palette slots). Refs: plan §"Rules for the agents writing the code" (a bug that corrupts data is not acceptable), plan §"Layered architecture" (layer 4).
  Completion: Tests with fixtures just inside and just outside each limit pass and fail respectively; the pinned project's own maps validate without false positives (count recorded).
  Verification: 2026-11-18 UTC: `cargo test -p tauros-connector-gen3 --test limits -- --nocapture` passed all 3 tests: 10240/next padded blocks, 64/65 templates, 15/16 simultaneous NPCs, separated NPC groups, shared/distinct palette tags at 16/17 estimated slots, stable item ids, dead/live behavior slots and appended ids 254/255. The pinned project's 939 maps validate with no blocking violations and 412 warnings. Cumulative AddEvent overflow is rejected without changing the fingerprint; manual NPC or item-id edits after open are rejected before build/Docker. `cargo test --workspace`, 56 Python tests, targeted Clippy with `-D warnings`, privacy scan of modified/new files and `git diff --check` passed. README and master document policy and limits: existing NPC pressure is advisory, new/worsened pressure blocks; visibility, palettes, moving actors and walkability are not proved statically; numeric stability is relative to connector open, not historical saves.

- [x] **Done** **G3-009** Create new trainers with their constants.
  Depends on: `G3-002`.
  Responsible: Agent
  Owner: Codex
  Scope: Let an UpsertTrainer create a trainer whose constant does not exist yet: add the `TRAINER_` define to include/constants/opponents.h, raise `TRAINERS_COUNT_EMERALD` within `MAX_TRAINERS_COUNT_EMERALD` (trainer flags share that space) and append the block to trainers.party. Found by G3-002, which rejects unknown constants (rule `trainer.constant`). Refs: plan §"Layered roadmap toward the beta" (Phase 3, create_trainer), plan §"Rules for the agents writing the code" (Compatible formats).
  Completion: A created trainer builds and appears in the generated trainers.h; exceeding the free slots is rejected before any write; existing trainer ids are unchanged.
  Verification: 2026-11-18 UTC: `cargo test -p tauros-connector-gen3 --test new_trainers -- --nocapture` passed append/roundtrip/no-op/capacity checks: new id 855 raises count to 856, only opponents.h and trainers.party change in one snapshot/provenance commit, all other header bytes and existing ids stay unchanged; an over-capacity batch leaves the fingerprint unchanged and a fitting batch fills exactly the nine available slots to count 864. `TAUROS_DOCKER_TESTS=1 cargo test -p tauros-connector-gen3 --test limits --test new_trainers -- --nocapture` passed, including generated `[TRAINER_TAUROS_NEW]` in trainers.h and a successful ROM build in the hardened container. Final `cargo test --workspace` and targeted Clippy passed; README and master updated. Limits: Emerald table only, literal decimal count required, no deleted-id reuse or automatic expansion of trainer flag storage.

- [ ] **Not started** **G3-010** Create maps and resize layouts.
  Depends on: `G3-003`.
  Responsible: Agent
  Owner: unassigned
  Scope: Create maps (map directory, map.json, map_groups.json entry, layout entry with blockdata and border) and resize existing layouts (map.bin and layouts.json) through the model. Found by G3-003, which rejects both (rules `map.create`, `map.layout_size`). Refs: plan §"Layered roadmap toward the beta" (Phase 5), plan §"Rules for the agents writing the code" (Compatible formats).
  Completion: A created map and a resized map build and open in Porymap; untouched maps stay byte-identical.

- [x] **Done** **G3-011** Faster container build tests.
  Depends on: `G3-005`.
  Responsible: Agent
  Owner: Claude Code
  Scope: Requested by the owner on 2026-11-18 to keep the development loop light: seed the container build-test fixtures from the FND-002 base build (copied with timestamps) so `make` rebuilds only edited files, and move the clean-build timing and the test-runner run into an explicitly ignored slow test. Refs: plan §"Layered roadmap toward the beta" (Phase 1, incremental build), plan §"Rules for the agents writing the code" (Targeted tests).
  Completion: The default container test run builds the edited project incrementally and takes well under the previous ~3 minutes (timings recorded); the slow test still checks clean versus no-change timing and the test runner when run with `--ignored`.
  Verification: 2026-11-18 UTC: `TAUROS_DOCKER_TESTS=1 cargo test -p tauros-connector-gen3 --test toolchain_build` now copies the base build (`cp -a`, timestamps kept; build outputs stay out of the fixture's git through the expansion's .gitignore) and took 105 s in total instead of 166–200 s; the edited project's incremental build took 30.4 s and produced the same ROM as the earlier clean build of the same edits (sha256 f80a5b24…), and the error-capture checks passed unchanged. `cargo test ... -- --ignored` ran the slow test: clean build 93.1 s, no-change rebuild 2.1 s, `make check TESTS='Trainer Party'` 6 PASS in 73.9 s. Without the opt-ins `cargo test` reports the slow test as ignored, not as passed. Limits: the default container test still needs TAUROS_DOCKER_TESTS=1 and a base build; editing flags.h makes the incremental build recompile most C files.

## Phase 2 — Emulator

- [x] **Done** **EMU-001** Emulator connector interface and libmgba core.
  Depends on: `DEC-002`, `FND-001`.
  Responsible: Agent
  Owner: Claude Code
  Scope: Own emulator trait (so melonDS can implement it later) and an mGBA implementation per DEC-002: load ROM, run frames, framebuffer, input, audio. Refs: plan §"Layered roadmap toward the beta" (Phase 2), plan §"Layered architecture" (layer 5), plan §"Risks and community notes" (Young dependencies).
  Completion: Headless test boots a homebrew or user-built ROM, advances frames, captures a framebuffer and applies input; no Nintendo asset is committed.
  Verification: 2026-11-18 UTC: `cargo test -p tauros-emu` passed. The unit test checks that the button bits match `GBAKey`. Four headless tests in crates/emu/tests/mgba_core.rs: (1) the original probe cartridge (tests/probe/probe.s assembled with devkitARM from the pinned image, 312 bytes, no Nintendo logo, embedded as bytes) boots and runs 60 frames. The 240x160 RGBA frame is uniform blue; holding A turns it uniform red within 5 frames and releasing restores blue within 5. Its square tone gives a non-silent audio peak, and the audio length is within 5% of the frame count at 32768 Hz (measured 37936 against about 38404). (2) Without a ROM, `NoRom` is returned; non-ROM bytes and a broken signature are rejected, after which the core stays usable. (3) Reset and reload restart the frame counter. (4) The user-built pokeemerald-expansion base ROM boots: twin cores stay identical for 400 frames, with audio peak 9249; pressing Start changes 38382 of 38400 pixels relative to the idle twin. Mutation: making the button bits always 0 failed tests (1) and (4). `cargo test --workspace` and `cargo clippy -p tauros-emu --tests` are clean. No ROM or Nintendo asset is committed. Limits: GBA core only; test (4) skips when the base ROM is missing; audio is checked by length and amplitude, not by listening; no real-time pacing or window (EMU-004); savestates and RAM reads are EMU-002.

- [x] **Done** **EMU-002** Savestates and RAM reading.
  Depends on: `EMU-001`.
  Responsible: Agent
  Owner: Claude Code
  Scope: Save/load state and read memory regions through the emulator trait. Refs: plan §"Layered roadmap toward the beta" (Phase 2), plan §"Young projects to take ideas from" (PokeMCP lesson: test via RAM and savestates).
  Completion: Tests show a saved state restored produces an identical framebuffer and RAM snapshot; reads of known addresses match expected values.
  Verification: 2026-11-18 UTC: `cargo test -p tauros-emu` passed; crates/emu/tests/state_memory.rs has three tests. (1) With the probe cartridge: the region list matches GBA EWRAM/IWRAM; ROM is read-only and I/O not writable. Known addresses read back: the ROM equals the 312 loaded bytes, `TAUR01` at 0x080000AC, DISPCNT 0x0403, SOUNDCNT_L 0x1177 and blue 0x7C007C00 in VRAM. EWRAM and VRAM writes read back; ROM writes, unmapped reads and region-straddling reads are refused. (2) On the probe, a state taken after 30 frames reproduces the EWRAM and IWRAM at save time. Replaying 1 frame after a 10-frame detour holding A gives the same framebuffer, EWRAM, IWRAM and VRAM as the original run; the detour alone differs. A wrong-sized state is refused and taking a state without a ROM is `NoRom`. (3) The same replay on the user-built Emerald base ROM (300 frames, 60-frame Start detour, 120-frame replay). The bus read of `gTrainers` equals the ROM file. `gSaveBlock2Ptr`/`gSaveBlock1Ptr` point into their blocks with the same offset, a multiple of 4 below 128 (observed 48 and 72 in different runs). Mutation: turning `load_state` into a no-op failed tests 2 and 3. `cargo test --workspace` and `cargo clippy -p tauros-emu --tests` are clean. Limits: GBA only; I/O registers cannot be written (libmgba stubs); test 3 skips without the base ROM; boots are not reproducible across processes (see master, EMU-002 observation).

- [x] **Done** **EMU-003** Boot straight into a chosen map or scene via warps and flags.
  Depends on: `EMU-002`, `G3-003`, `G3-004`.
  Responsible: Agent
  Owner: Claude Code
  Scope: Gen 3 specific mechanism to start the ROM at a chosen map/position with chosen flags set, using connector map and flag data. Refs: plan §"Layered roadmap toward the beta" (Phase 2).
  Completion: Headless test boots into a named map and verifies location and flags from RAM.
  Verification: 2026-11-18 UTC: `TAUROS_DOCKER_TESTS=1 cargo test -p tauros-connector-gen3 --test boot` passed (7.9 s) on a copy of the base build of pinned expansion 3796c72. `GameLayout::measure` compiled the offset probe against the project's headers in the hardened container: SaveBlock1 location 4, pos 0, flags 0x1270 (0x12C bytes); Main callback1 0, callback2 4, state 0x438; WarpData vanilla. `BootPlan` resolved maps from the generated map_groups.h, flags from the connector snapshot and addresses from the .sym. It rejected an unknown map, a position outside Petalburg and an unknown flag before running. Headless mGBA booted into MAP_PETALBURG_CITY (15,10) with FLAG_BADGE01_GET in 284 frames; RAM read back group 0, num 0, x 15, y 10 and the flag set. A reboot of the same core into MAP_ROUTE101 (10,10) without flags read back (0,16,10,10) with the badge flag clear. An earlier exploratory run of the same mechanism produced a screenshot showing the player in Petalburg in front of the gym. Unit tests cover .sym and map_groups.h parsing and layout validation. Mutation: skipping the flag writes made the boot fail with 'flag FLAG_BADGE01_GET was cleared'. `cargo test --workspace` and `cargo clippy --workspace --tests` are clean. Limits: the boot always goes through a fresh new game (empty player name, default gender, no party); whether the position is walkable is not checked; FRLG builds are untested; a warp-id target is not offered; boots are not bit-reproducible across processes (see master, EMU-002 observation).

- [x] **Done** **EMU-004** Minimal Tauri window showing the emulator.
  Depends on: `EMU-001`, `FND-001`.
  Responsible: Agent
  Owner: Claude Code
  Scope: Tauri app shell rendering the framebuffer with keyboard input and audio; no editor features. Refs: plan §"Decisions made" (Core row), plan §"Layered roadmap toward the beta" (Phase 2).
  Completion: The app runs a ROM interactively with audio on the owner's Linux machine (manual check by the Owner recorded).
  Verification: 2026-11-18 UTC: manual check by the Owner on his Linux machine (Pop!_OS 22.04, X11): `TAUROS_LOG_KEYS=1 cargo run --release -p tauros-app` with the base Emerald ROM; asked to check video, audio and keys, he answered "va una bomba" (works great). His log showed 59.6-59.8 fps, Web Audio running at 44100 Hz and 8 A presses reaching the core; replaying those presses headless at the same frames goes intro -> title (CB2_InitTitleScreen) -> main menu (CB2_MainMenu). Agent checks: `cargo test -p tauros-app` (button mask, GDK key names, minimum-hold latch), clippy clean, release and debug builds smoke-run at 59.7 fps, XTEST keys skipped the intro to the title screen (window screenshots). Fixed on the way: libmgba built without GCC LTO (release links failed); keyboard read on the GTK window (page key events never arrived from the Owner's keyboard); presses held 4 frames (GTK delivered press and release in the same frame). Limits: Linux/X11 only checked; audio quality judged by the Owner, not measured; no gamepad; keys fixed (no remapping UI).

- [x] **Done** **EMU-005** Phase 2 integration test and README update.
  Depends on: `EMU-003`, `EMU-004`.
  Responsible: Agent
  Owner: Codex
  Scope: The Tauri window shows the ROM and starts from a chosen point; one integration test for the boot-to-point path; README update. Refs: plan §"Layered roadmap toward the beta" (Phase 2 done-when), plan §"Rules for the agents writing the code" (End of phase).
  Completion: Integration test passes; Owner confirms the window starts at the chosen point; README updated.
  Verification: 2026-11-18 UTC: `TAUROS_DOCKER_TESTS=1 cargo test -p tauros-app --test phase2_integration -- --nocapture` passed on the pinned expansion base-build copy: CLI selection, project loading, BootPlan execution on the desktop emulator worker and binary Tauri frame delivery; Petalburg with FLAG_BADGE01_GET and Route 101 each reached the expected map/position in 284 frames and rendered different non-uniform RGBA images with bounded stereo audio. Invalid CLI/map/position and plain ROM loading were checked. `cargo test --workspace`, all 56 Python tests, `cargo clippy -p tauros-app --tests -- -D warnings` and `git diff --check` passed. Desktop smoke run: Petalburg at x=15 y=10, about 59.7 fps; local window capture showed the player in front of the gym. Owner explicitly confirmed the requested position and movement check in chat. README documents capabilities, CLI usage, prerequisites and limits; master updated. Limits: Linux and pinned Emerald expansion only; fresh game with no party; walkability and stale/mismatched build artifacts are not checked; project launches need the source checkout and Docker toolchain.

## Phase 3 — MCP server

- [x] **Done** **MCP-001** rmcp server skeleton reachable by external clients.
  Depends on: `FND-001`.
  Responsible: Agent
  Owner: Codex
  Scope: MCP server in Rust using rmcp, in the same process as model and connectors, exposed so external Claude Code and Codex can connect. Refs: plan §"Decisions made" (MCP server row, Agent row).
  Completion: A trivial tool is listed and callable over stdio from Codex and Gemini CLI or Antigravity, and from an MCP test client, with commands recorded and README config snippets for all clients. Owner amendment on 2026-11-18: document the expired Claude Code OAuth session and defer its live call to the Owner after login renewal; supply its configuration and retry command.
  Verification: 2026-11-18 UTC: rmcp pinned to 3.11.0 in Cargo.toml/lock; `cargo test -p tauros-mcp -- --nocapture` and `python3 scripts/check_mcp.py` passed initialization, tool/schema/annotation discovery, `tauros_status` returning `Tauros 0.1.0 ready`, unknown-tool rejection, ping and clean EOF exit over actual stdio pipes. Codex CLI emitted a completed mcp_tool_call for tauros/tauros_status with no error; isolated checksum-pinned Antigravity CLI 1.5.11 emitted call_mcp_tool DONE with the same result (temporary permission only for that tool). Commands, config snippets, dependency pin and sanitized evidence are recorded in README/master/toolchain/mcp-clients.json; raw transcripts stay local. `cargo test --workspace`, 56 Python tests, targeted Clippy, privacy and diff checks passed. Owner accepted the Completion amendment: Claude Code live call is deferred because OAuth refresh failed; no successful Claude call is claimed. Limits: status tool only; domain/emulator tools remain MCP-002/003; no HTTP transport.

- [x] **Done** **MCP-002** Domain tools over the semantic model.
  Depends on: `MCP-001`, `G3-002`, `G3-003`, `G3-004`, `G3-005`, `G3-008`, `G3-009`.
  Responsible: Agent
  Owner: Antigravity
  Scope: create_trainer, place_npc, add_scene, build and run_test operating only on the common model and connector trait, with validation; no free-form file edits. Informed by RES-001 when available. Refs: plan §"Layered architecture" (layer 2), plan §"Layered roadmap toward the beta" (Phase 3).
  Completion: Each tool has a contract test against the Gen 3 connector; every write goes through the G3-001 snapshot guard (tested).
  Verification: 2026-11-18 UTC: extended `tauros_model::Connector` with `run_tests` (`TestOutput`, `TestCaseOutput`), implemented it on `Gen3Connector` (pre-test validation + G3-001 snapshot + toolchain runner), and exposed `create_trainer`, `place_npc`, `add_scene`, `build` and `run_test` on `TaurosServer` (`crates/mcp/src/domain.rs`, `src/lib.rs`) operating exclusively through `Connector` with `Provenance` support and no free-form file edits. `cargo test -p tauros-mcp -- --nocapture` passed all 3 tests (stdio contract + in-process JSON-RPC tests over `tokio::io::duplex` for tool discovery, all 5 tools, validation rejection and no-connector error). `TAUROS_DOCKER_TESTS=1 cargo test -p tauros-app --test mcp_domain_gen3 -- --nocapture` passed all 3 Gen 3 contract tests on `git archive` copies of pinned expansion 3796c72 (109.9 s): `create_trainer` (appending `TRAINER_TAUROS_MCP`), `add_scene` (with atomic `FLAG_TAUROS_MCP_SEEN` allocation) and `place_npc` each produced G3-001 snapshot commits with `Tauros-Provenance` and `Tauros-Snapshot` trailers and reloaded cleanly; invalid map placement was rejected with zero commits; a failing pre-commit hook aborted `create_trainer` with `snapshot_failed` leaving fingerprint and commit count unchanged; hardened Docker `build` and `run_test` (`Trainer Party`) both succeeded over MCP JSON-RPC. `cargo clippy -p tauros-model -p tauros-mcp --tests -- -D warnings` and all 56 Python tests (including `test_layers.py`) passed. Limits: the `tauros-mcp` CLI binary without project wiring still starts unattached (returns `no_connector` on domain tools); Poryscript scene compilation via `add_scene` relies on `Gen3Connector`'s existing G3-004 path (assembly scene + flag allocation exercised in the non-Docker test).

- [x] **Done** **MCP-003** Emulator tools and RAM-based checks.
  Depends on: `MCP-001`, `EMU-003`, `VER-001`.
  Responsible: Agent
  Owner: Antigravity
  Scope: Tools to boot to a point, press input, take screenshots, save/load state and assert game state from RAM (e.g. battle outcome, flags, money). mgba-live-mcp and mcp-mgba are design references only. Refs: plan §"Layered roadmap toward the beta" (Phase 3), plan §"Young projects to take ideas from".
  Completion: Tests show RAM assertions detect a known state and its negation; tools are callable via MCP.
  Verification: 2026-11-18 UTC: implemented `crates/mcp/src/emulator.rs` (`EmuSession`, `BootHook`, `SymbolTable`, PNG screenshot encoder, named ROM-bound savestates, typed RAM reader with `deref` re-read and `flag_bit` decoding, and the `VER-001` inert-spec loader/driver/evaluator with `shift`, `wrong_value`, `cold_boot` and `base_rom` counterfactuals) and exposed `emu_boot`, `emu_press_input`, `emu_screenshot`, `emu_save_state`, `emu_load_state`, `emu_assert_ram` and `run_verification` on `TaurosServer`. `cargo test -p tauros-mcp -- --nocapture` passed all 6 tests: mock-emulator MCP JSON-RPC tests verified tool discovery, `boot_to`, `emu_press_input`, PNG screenshot capture, savestate roundtrip, RAM assertions detecting both known states (`gBattleOutcome == B_OUTCOME_LOST`, dereferenced moving `gSaveBlock1Ptr` `flag_bit` and `money`) and their negations, rejection of raw hex addresses and undereferenced save-block pointers, and `run_verification` verdicts (`PROVEN`, `FAILED`, `INCONCLUSIVE`, `REJECTED`); `real_mgba_emerald_ram_checks_and_screenshot_via_mcp` booted the user-built `pokeemerald.gba` with `pokeemerald.sym` in libmgba 0.10.5 over MCP JSON-RPC, captured a valid 240x160 PNG, saved/reloaded a 397312-byte state, and verified `gBattleOutcome == 0` (pass) vs `== 2` (fail). `cargo clippy -p tauros-mcp --tests -- -D warnings` and 56 Python tests passed. Limits: `VER-001` RNG `policy` strategies and `symbol_at`/`symbol_sequence` checks are not yet implemented; `tauros-mcp` CLI starts without an attached emulator unless wired by Layer 1 (`MCP-004`).

- [x] **Done** **MCP-004** Phase 3 end-to-end director's scene and README update.
  Depends on: `MCP-002`, `MCP-003`.
  Responsible: Agent
  Owner: Antigravity
  Scope: Claude Code on the PC, connected to the MCP, builds the director's scene end to end: trainer with 3 Pokémon, lost battle without whiteout, RAM verification, preview. Refs: plan §"Layered roadmap toward the beta" (Phase 3 done-when), plan §"Rules for the agents writing the code" (End of phase).
  Completion: Recorded session transcript and RAM-check output show the scene built and verified; Owner confirms the preview; README updated.
  Verification: 2026-11-19 UTC: wired Layer 1 `tauros-mcp-project` (`crates/app/src/bin/tauros-mcp-project.rs`, `crates/app/src/mcp_host.rs`) with `Gen3Connector`, `Mgba` (`EmuSession` + `Gen3BootHook`) and post-build automatic ROM/`.sym` reload, added `VER-001` `"lose-fast"` timeline policy in `crates/mcp/src/emulator.rs`, and fixed missing `mapjson` defaults for new NPCs in `crates/connector-gen3/src/maps.rs`. `TAUROS_DOCKER_TESTS=1 cargo test -p tauros-app --test phase3_director_scene -- --nocapture` passed (64.44 s clean build, 20.01 s rerun with discriminating `pos` probe): via MCP JSON-RPC only, created 3-Pokémon `TRAINER_TAUROS_DIRECTOR`, added Poryscript scene `PetalburgCity_EventScript_TaurosDirector` (using Emerald's native `B_FLAG_NO_WHITEOUT = FLAG_TEMP_1` and atomically allocated `FLAG_TAUROS_DIRECTOR_SEEN = 32`), placed the Director NPC at `(15, 11)`, built `pokeemerald.gba` (`ffb0c7a6…`) and `pokeemerald.sym` (`3e0ebd82…`) in Docker, booted mGBA to `MAP_PETALBURG_CITY (15, 10)`, and passed `run_verification` with `verdict: PROVEN` (`gBattleOutcome == B_OUTCOME_LOST (2)`, `gSaveBlock1Ptr` deref location == `MAP_PETALBURG_CITY`, deref `pos == 655375` (`x=15, y=10`, discriminating), `FLAG_TAUROS_DIRECTOR_SEEN == 1`, counterfactuals `shift`, `wrong_value` and `cold_boot` all failing). Live Claude Code CLI (`claude` 2.1.304) connected over stdio to `tauros-mcp-project` on `director-scene-mcp004`, executed `tauros_status` -> `emu_boot` -> `emu_save_state` -> `run_verification`, and returned `verdict: PROVEN` (37.5 s; raw stream-json transcript stored locally under `$TAUROS_DATA/projects/director-scene-mcp004/build/tauros/claude-mcp004-transcript.jsonl`). Owner confirmed the desktop preview in `tauros-app` in chat on 2026-11-19. README and master.md updated; `cargo test --workspace`, `cargo clippy --workspace --tests -- -D warnings`, all 56 Python tests and `check_private.py --all` passed. Limits: `B_FLAG_NO_WHITEOUT = FLAG_TEMP_1` is a one-line project config in `include/config/battle.h` (because `trainerbattle_earlyrival` has an upstream `1 & 3` bug at `src/battle_setup.c:1328` at pin `3796c72`); Claude Code's live CLI session ran verification against the project built by the MCP test rather than re-running the 60 s Docker build from scratch.

## Phase 4 — Agent bridge and voice

- [x] **Done** **AGT-001** Swappable agent bridge interface.
  Depends on: `FND-001`.
  Responsible: Agent
  Owner: Antigravity
  Scope: agent-bridge crate interface (send message, stream events, cancel) independent of vendor so an Agent SDK adapter now and Codex/API adapters later fit. Refs: plan §"Decisions made" (Swappable agent row).
  Completion: Interface documented and exercised by a fake adapter in tests.
  Verification: 2026-11-18 UTC: crates/agent-bridge defines the object-safe `AgentBridge` trait (`capabilities`, `start_session`, `send_message`, `cancel`, `close_session`), typed IDs (`SessionId`, `TurnId`, `ToolCallId`), `AuthMode`, `McpServerConfig`, `SessionConfig`, `UserMessage`, `AgentEvent`, `EventStream`/`TurnStream`, `BridgeError` and the in-process `fake::FakeAdapter`. `cargo test -p tauros-agent-bridge -- --nocapture` passed 4 tests covering object safety (`Box<dyn AgentBridge>`), streaming text/thinking/status/tool-call events, deterministic mid-stream cancellation via `WaitForGate`, auth/session/turn lifecycle errors and JSON roundtrip; `cargo clippy -p tauros-agent-bridge --tests -- -D warnings`, `RUSTDOCFLAGS="-D warnings" cargo doc -p tauros-agent-bridge --no-deps` and 56 Python tests (including `test_layers.py`, confirming zero internal workspace dependencies) all passed. Limits: interface and fake adapter only; real vendor adapters belong to AGT-002, AGT-006 and AGT-007.

- [x] **Done** **AGT-002** TypeScript Agent SDK sidecar connected to the internal MCP.
  Depends on: `AGT-001`, `RES-002`, `MCP-002`.
  Responsible: Agent
  Owner: Antigravity
  Scope: Small TypeScript sidecar using the Agent SDK with the auth model verified in RES-002 (owner's personal use: the unmodified Claude Code binary's own sign-in; the app never collects or stores claude.ai credentials or tokens), connected to the in-process MCP, launched and supervised by the app. Refs: plan §"Decisions made" (Agent row), plan §"Layered roadmap toward the beta" (Phase 4).
  Completion: From a Rust test or the app, a prompt reaches the sidecar and a domain tool call is executed and streamed back.
  Verification: 2026-11-19 UTC: implemented `sidecar/claude-agent-sdk/` (TypeScript 5.8.3, `@anthropic-ai/claude-agent-sdk` 0.3.328, `src/index.ts`, `src/protocol.ts`, `src/mcp-bridge.ts`), `crates/agent-bridge/src/claude_sidecar.rs` (`ClaudeSidecarAdapter: AgentBridge` supervising the Node sidecar over NDJSON with `AuthMode::HostSignIn` enforcement per RES-002 and clean shutdown on drop), and `crates/app/src/mcp_host.rs` (`InProcessMcpHost` serving the in-process `TaurosServer` over a private `0700` Unix domain socket bridged via `mcp-bridge.js`). `npm --prefix sidecar/claude-agent-sdk run build` compiled cleanly; `cargo test -p tauros-app --test agent_sidecar_mcp -- --nocapture` passed (0.16 s), verifying a prompt reaching the compiled sidecar, executing `create_trainer` (3 Pokémon) on the shared in-process `TaurosServer`, streaming `TurnStarted`/`ToolCallStarted`/`ToolCallCompleted`/`TextDelta`/`TurnCompleted` back to Rust, rejecting `AuthMode::ApiKey`, and cancelling a delayed turn cleanly; `TAUROS_LIVE_CLAUDE=1 cargo test -p tauros-app --test agent_sidecar_mcp live_claude_sdk_sidecar_calls_in_process_mcp -- --ignored --nocapture` passed (7.38 s), confirming the live `@anthropic-ai/claude-agent-sdk` `query()` using the unmodified host Claude Code sign-in executed `create_trainer` (`TRAINER_CLAUDE_LIVE`) on the in-process `TaurosServer` and streamed events back. `cargo test --workspace`, `cargo clippy --workspace --tests -- -D warnings`, and all 56 Python tests (including `test_layers.py`) passed. Limits: personal-use `HostSignIn` only (public-release API-key auth is `AGT-005`).

- [x] **Done** **AGT-003** Streaming chat panel in the app.
  Depends on: `AGT-002`, `EMU-004`.
  Responsible: Agent
  Owner: Claude Code
  Scope: Chat UI in the Tauri app with streamed responses and visible tool calls. Refs: plan §"Layered roadmap toward the beta" (Phase 4), plan §"Layered architecture" (layer 1).
  Completion: Owner sends a request in the app and sees streamed output and tool activity (manual check recorded).
  Verification: 2026-11-19 UTC: `crates/app/src/chat.rs` (`Chat`, `claude_backend`), Tauri commands `chat_send`/`chat_cancel`/`chat_reset`/`chat_info`/`set_typing`, chat panel in `crates/app/ui/`; sidecar now persists sessions so `resume` works and releases the turn before `turn_completed`. `cargo test -p tauros-app --test chat_panel` passed 5 tests (event order text/tool/text/completion, overlapping turn refused, cancel ends with Cancelled, reset opens a new session, factory errors returned and retried, real sidecar `direct_mcp` driver running `create_trainer` on an in-process `TaurosServer`); `TAUROS_LIVE_CLAUDE=1 ... live_chat_remembers_the_previous_turn -- --ignored` passed (8.5 s; failed before the persistence fix with `No conversation found`). `cargo test --workspace`, `cargo clippy --workspace --tests -- -D warnings` and `python3 -m unittest discover -s tests` passed. UI checked in the built-in browser with a mocked Tauri API (streaming cursor, Stop, tool rows done/failed, usage line). Owner manual check: launched `tauros-app` on `director-scene-mcp004` (`MAP_PETALBURG_CITY` 15, 10), sent a request in Italian and reported a screenshot showing the live `tauros_status` tool call marked done and the completed streamed reply. Limits: a screenshot cannot show the streaming itself, and the typing-versus-game-keys check was not reported; replies show Markdown as raw text; the usage line counts only uncached input tokens; the agent's emulator is separate from the window's (`APP-003`).

- [ ] **Not started** **AGT-004** Microphone input with local transcription.
  Depends on: `AGT-003`.
  Responsible: Agent
  Owner: unassigned
  Scope: Local speech-to-text (whisper.cpp suggested; final engine chosen and recorded in master.md here) feeding text to the agent; no cloud transcription. Refs: plan §"Decisions made" (Voice row), plan §"Layered roadmap toward the beta" (Phase 4).
  Completion: Spoken request transcribed offline and delivered to the chat (manual check recorded); engine decision in master.md.

- [ ] **Not started** **AGT-005** API-key authentication path for a public release.
  Depends on: `AGT-002`, `AGT-006`.
  Responsible: Agent
  Owner: unassigned
  Note: Deferred by the Owner on 2026-11-20; no paid API checks until explicitly requested. Personal Claude subscription integration already has live evidence.
  Scope: Per master.md "Agent SDK authentication (RES-002)", let each user supply their own Anthropic API key or cloud-provider credential (stored locally, never sent elsewhere), with no claude.ai login offered by the app and no "Claude Code" branding. Refs: plan §"Decisions made" (Agent row: each user needs their own credentials; User row: possible open-source release). Excludes any shared or bundled key.
  Completion: Tests show the sidecar authenticates with a user-provided API key and refuses to start without one in release mode; the key is absent from logs and from the repository; master.md records the release auth model.

- [x] **Done** **AGT-006** Codex adapter for the agent bridge.
  Depends on: `AGT-001`, `MCP-002`.
  Responsible: Agent
  Owner: Codex
  Note: Owner accepted the working Codex adapter and requested closure on 2026-11-20. UI/UX refinements remain APP-003; further desktop mixed-edit/history/undo checks remain AGT-008 and beta chat Stop/history acceptance remains APP-005. API work stays deferred in AGT-009. Known untested auth renewal/expiry behavior is a documented limit, not a passed check.
  Scope: Implement the agent-bridge interface over the Codex SDK or app-server, connected to the Tauros MCP, authenticating through the official runtime-managed ChatGPT browser flow from Tauros, with pending/success/failure/cancel states and the dedicated credential home; API-key UI and paid validation are deferred to AGT-009 per the Owner on 2026-11-20; the app never stores tokens outside the documented flow. Include the hybrid workflow defined by `AGT-008`: combine native source inspection/editing/development commands and Tauros MCP in one request, reusing the host-owned snapshot/provenance/recovery/serialization contract and project confinement; implement the provider-specific execution interception rather than assuming Claude hooks are portable. Owner follow-up 2026-11-20: verify real image delivery from emulator screenshots to the model; native MCP image delivery is implemented and has synthetic real-runtime acceptance; the Owner subsequently supplied the real-game repaired-sign preview with native tool, build, boot, input and screenshot activity and accepted this implementation for closure. For visual map edits, support a before/after visual check alongside semantic and interaction checks, and report unverified results explicitly. Refs: plan §"Decisions made" (Swappable agent row, Agent row).
  Completion: A prompt sent through the bridge reaches Codex, a Tauros domain tool call executes and streams back; ChatGPT browser login can be started and cancelled in the app and its result is shown without exposing credentials. API auth is explicitly deferred to AGT-009 and is not required for closure. A mixed native-edit/domain-tool turn is verified with source changes in history and reversible through undo; snapshot failure prevents native writes.
  Verification: 2026-11-20 UTC: Owner explicitly accepted the working implementation and requested AGT-006 closure after successful browser login, real desktop Codex turns and the repaired-sign preview on director-scene-mcp004. Earlier checks actually run and recorded in master.md: deterministic mixed native edit plus real Tauros MCP create_trainer with history/undo and snapshot-failure rejection; login pending/success/failure/cancel protocol and installed-runtime start/cancel; real authenticated tauros_status with matching tool events/result after enabling Code Mode host; real before/after synthetic screenshot test with native images and correct retained-fragment detection. Targeted MCP/bridge/app tests, Clippy and app builds passed as recorded; no new runtime checks were run for this documentation-only closure. Owner screenshots show native edits, build, boot, input and screenshot rows and the repaired sign, but are not independent collision or provenance/undo assertions. Limits: token renewal and login expiry unexercised; further desktop mixed-history/undo and Stop checks retained in AGT-008/APP-005; selected/live map context and interface refinements retained in APP-003. No paid API test, credential inspection, commit or push.

- [x] **Done** **AGT-007** Gemini adapter for the agent bridge.
  Depends on: `AGT-001`, `MCP-002`, `AGT-006`.
  Responsible: Agent
  Owner: Claude Code
  Note: Official route selected: Antigravity CLI ACP server (`agy acp`) with the user's cached subscription sign-in; see master.md "Antigravity ACP adapter (AGT-007)". In-app Google sign-in is AGT-010. Desktop check in the Tauri window and Gemini image delivery are not covered.
  Scope: Owner sequencing 2026-11-20: start only after AGT-006 is complete; revise the implementation choice with evidence before starting. Select and implement an officially supported Antigravity subscription integration for the agent-bridge, investigating the editor/ACP route and recording current official evidence before implementation. Do not wrap consumer headless login contrary to provider restrictions; if no supported route satisfies Tauros safeguards, record the blocker. Gemini API/Vertex connections and paid checks are deferred to AGT-009 by the Owner on 2026-11-20, superseding the earlier API-only implementation choice in RES-003. Include the hybrid workflow defined by `AGT-008`: combine native source inspection/editing/development commands and Tauros MCP in one request, reusing the host-owned snapshot/provenance/recovery/serialization contract and project confinement; implement the provider-specific execution interception rather than assuming Claude hooks are portable. Refs: plan §"Decisions made" (Swappable agent row).
  Completion: An officially supported subscription route is documented and a prompt sent through the bridge reaches Gemini, a Tauros domain tool call executes and streams back; no API spending or unsupported consumer credential reuse. If no supported route exists, keep the task open with evidence of the blocker. A mixed native-edit/domain-tool turn is verified with source changes in history and reversible through undo; snapshot failure prevents native writes.
  Verification: 2026-11-20 UTC: official ACP pages read (antigravity.google docs/cli/acp and docs/ide/extensions/acp: "Any ACP client can launch Antigravity CLI", cached sign-in reused) plus a Google staff forum reply recommending the ACP server; recorded in master.md. Implemented `tauros_agent_bridge::antigravity::AntigravityAdapter` (ACP client of `agy acp`, dedicated `--gemini-dir`/empty workspace, credential env removed, HostSignIn only, permission policy allowing only attached MCP servers), `crates/app/src/native_mcp.rs` (`project` MCP server over the AGT-008 transaction socket), `chat::antigravity_backend` and an Antigravity choice in the app selector. Probed real `agy acp` 1.5.11: initialize, session/new with cached sign-in, permission requests for MCP, read and edit calls. `cargo test -p tauros-app --test antigravity_adapter` passed (deterministic ACP peer with real MCP servers: built-in tools denied, mixed native write plus `create_trainer`, AI-Generated history, continuity, undo, snapshot-hook rejection, cancel, crash, auth errors); a temporary permissive-policy mutation failed both tests. `TAUROS_LIVE_AGY=1 ... live_antigravity_mixed_turn_on_original_synthetic_data -- --ignored` passed in 23 s with the Owner's subscription (no API key) on original synthetic data: native read/write and Tauros `create_trainer` completed and streamed, history AI-Generated, undo restored. `cargo clippy --workspace --tests -- -D warnings`, `cargo test -p tauros-agent-bridge -p tauros-app` and 56 Python tests passed. Limits: no desktop run in the Tauri window; default model only; no in-app sign-in (sign in with `agy`); screenshot image delivery to Gemini not exercised; no namespace confinement of the runtime (permission boundary only); no commit.

- [ ] **In progress** **AGT-008** Hybrid agent workflow with native source editing and MCP tools.
  Depends on: `AGT-003`, `G3-001`.
  Responsible: Agent
  Owner: Codex
  Note: (owner: Codex, implemented; local hybrid build-preview and real Claude decomp hybrid turn passed; Owner accepted Codex native editing/build/preview at AGT-006 closure on 2026-11-20, while explicit desktop provenance/history/undo verification remains open)
  Scope: Accepted by the Owner on 2026-11-20: let the integrated agent combine Tauros MCP/domain tools with native project source inspection, editing and development commands in the same request, choosing the most suitable path without requiring a domain tool for every game capability. Cover scripts, C and project configuration; retain snapshot-before-write, AI provenance, reviewable diffs and non-destructive undo for native edits, including writes performed by commands. Keep operations within the game project and the external-sources policy, prevent unguarded mutations, refresh or invalidate connector/editor state after native edits, and reuse Tauros build, emulator and verification tools. A native-edit failure must leave recoverable changes and a usable session. Per master.md "Agent reach over the whole game (Owner direction)"; supersedes the MCP-only writing restriction for this workflow once implemented. Refs: plan §"Decisions made" (Agent row, Swappable agent row), plan §"Layered architecture" (layers 1-4), plan §"Rules for the agents writing the code" (Never destructive, Compatible formats).
  Completion: An integration check demonstrates one agent request combining a domain change with a native source edit, followed by build and shared-emulator preview; checks demonstrate snapshot failure prevents native mutations, source edits appear in provenance/history and can be undone, and subsequent connector reads/writes use refreshed state without overwriting native changes. The Owner confirms the mixed workflow in the app; README and master.md record capabilities and limits.

- [ ] **Not started** **AGT-009** Deferred Codex and Gemini API authentication and paid acceptance checks.
  Depends on: `AGT-006`.
  Responsible: Agent
  Owner: unassigned
  Note: Owner decision 2026-11-20: prioritize subscription sign-in, finish OpenAI before Antigravity, and defer all API connections and spending. Do not run paid checks until explicitly authorized. Anthropic release authentication remains AGT-005.
  Scope: Enable the Codex API-key choice and implement the selected Gemini API or Vertex authentication path with user-owned credentials; validate real paid turns only after the Owner authorizes costs. Preserve runtime-managed credential storage, secret-free logs and the same protected native/MCP boundaries as subscription sessions. Refs: plan §"Decisions made" (Agent row, Swappable agent row).
  Completion: Each enabled API path has an authorized real turn with a streamed Tauros tool result and protected mixed native/domain operation, or an explicitly recorded provider-specific blocker; no credentials in logs or repository. Document costs authorization and actual verification, with no claims based solely on dummy-key protocol tests.

- [x] **Done** **AGT-010** Antigravity Google sign-in inside the app.
  Depends on: `AGT-007`.
  Responsible: Agent
  Owner: Claude Code
  Note: A real completed sign-in from Tauros is left to the Owner's first use; automated checks never touch the real sign-in.
  Scope: Accepted by the Owner on 2026-11-20. When Antigravity is selected, Tauros checks the cached `agy` sign-in silently and shows only signed-in or not signed-in, with no account details. If not signed in, a **Sign in with Google** panel consistent with the Codex one starts the official ACP `authenticate` method `oauth-personal` and shows pending, success, failure and cancel states; cancel stops the login process. A send that fails for missing sign-in opens the panel instead of a bare error. Only `oauth-personal` is offered (API key, Vertex and Enterprise stay in AGT-009 or out of scope). Only a status enum crosses IPC; OAuth URLs and provider error text never reach the chat or logs. No sign-out control, because the Antigravity sign-in is shared with the user's other Antigravity surfaces. Refs: plan §"Decisions made" (Swappable agent row, Agent row).
  Completion: The ACP `authenticate` behaviour of the pinned CLI is recorded in master.md from an actual probe; tests cover the status check, start, success, failure and cancel paths and that no URL or provider text crosses IPC; the panel is checked in the UI; the Owner's real sign-in is not altered by automated checks.
  Verification: 2026-11-20 UTC: probed `agy acp` 1.5.11 `authenticate` signed in (immediate reply, nothing opened) and in a bubblewrap namespace hiding the home and user bus (`-32000` on session/new; pending authenticate; runtime opened the browser on accounts.google.com; no URL over ACP); recorded in master.md "Antigravity sign-in inside the app (AGT-010)". Implemented `AntigravityAdapter::{signed_in, start_google_login, google_login_result}`, `crates/app/src/antigravity_login.rs`, Tauri commands `antigravity_login_{check,start,status,cancel}`, a `signed_out` status, and a shared Codex/Antigravity sign-in panel in `crates/app/ui`. `cargo test -p tauros-app --test antigravity_login` passed (already/success/failure/pending, check never starts sign-in, duplicate start refused, cancel kills the runtime, no provider text in status); `TAUROS_AGY_BIN=... --test antigravity_login installed_ -- --ignored` passed in isolation (signed_out, pending, stub browser host accounts.google.com, cancelled, no leftover process); the Owner's real sign-in still opened a session afterwards. UI checked in the built-in browser with mocked IPC (silent check, signed-out prompt, send redirected to the panel, pending/cancel/signed-in states, Codex panel unchanged, hidden for Claude). `cargo clippy --workspace --tests -- -D warnings`, `cargo test -p tauros-agent-bridge -p tauros-app` and 56 Python tests passed. Limits: no real completed Google sign-in from Tauros, no browser-failure case, no token renewal, Tauri window not driven; no commit.

- [x] **Done** **AGT-011** Model selector for Claude, Codex and Antigravity in the app.
  Depends on: `AGT-003`, `AGT-006`, `AGT-007`.
  Responsible: Agent
  Owner: Claude Code
  Note: The choice lasts for the app run; persisting it across restarts is not in scope.
  Scope: Requested by the Owner on 2026-11-20. The chat panel offers a model selector for the active provider, filled from the runtime's own model list (Agent SDK `supportedModels()`, Codex app-server `model/list`, Antigravity ACP `session/new` `availableModels`), with the runtime default preselected. A change applies to the following turns of the current conversation without losing it (SDK `model` option per resumed turn, Codex `turn/start` `model`, ACP `session/set_model`) and is kept for new conversations of that provider. Listing and selection start no model turn and expose no account details; switching is refused while a reply or sign-in is in progress. The bridge gains provider-neutral `list_models`/`set_model` operations. Refs: plan §"Decisions made" (Swappable agent row, Agent row).
  Completion: Tests show, for each adapter against its deterministic peer, the listed models and that a selected model reaches the runtime for the next turn of the same session; the app keeps the choice across a new chat; the selector is checked in the UI; each real runtime's model list is read once without inference, and README and master.md record the behaviour and limits.
  Verification: 2026-11-20 UTC: runtime model APIs verified (SDK `supportedModels()`/per-query `model`; Codex schema `model/list` and `turn/start` `model`; ACP `session/new` models and `session/set_model`), recorded in master.md "Model selector (AGT-011)". Implemented `AgentBridge::{list_models, set_model}` and `ModelOption`, adapter support for Claude (sidecar `list_models`/`set_model`, ack `data`), Codex and Antigravity, `Chat::{models, set_model, preset_model}`, Tauri `chat_models`/`chat_set_model` with per-provider memory, and the Model selector row in `crates/app/ui`. `cargo test -p tauros-app --test model_selection` passed (3 adapters: listed ids and current, next turn reports the selected model, conversation kept, choice reapplied after reset, hidden Codex models excluded across pages, unoffered Antigravity id refused). `TAUROS_LIVE_MODELS=1 ... live_runtimes_list_models_without_inference -- --ignored` passed: real Claude 14, Codex 8 and Antigravity 23 models, one current each, no message sent. UI checked in the built-in browser with mocked IPC. `cargo clippy --workspace --tests -- -D warnings`, `cargo doc -p tauros-agent-bridge` with `-D warnings`, `cargo test -p tauros-agent-bridge -p tauros-app` and 56 Python tests passed. Limits: no real turn on a non-default model; Tauri window not driven; choice not persisted across restarts; no commit.

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

- [ ] **In progress** **APP-003** Emulator panel integrated with the editor.
  Depends on: `EMU-003`, `EMU-004`, `G3-005`.
  Responsible: Agent
  Owner: Codex
  Note: (owner: Codex, fixed map-switch bounds and added separate Preview of the existing built ROM without a ROM build. Picker remembers per-map points and uses bounded centre defaults; backend rejects bad targets before build/reload. Targeted synthetic backend test, actual UI handlers under a temporary mocked DOM, JS syntax and app Clippy passed. Real Mart boot/walkability/timing still needs Owner acceptance; no game changes, commits or paid tests. Automatic agent context and held-refresh label remain open.)
  Scope: Build-and-preview from the editor in the same window, boot to the selected map or scene. The window's emulator is the same session the agent's MCP emulator tools drive, so what the agent builds, boots and checks appears in the window (accepted 2026-11-19, see master.md "Streaming chat panel (AGT-003)"). Owner follow-up 2026-11-20: pass structured context for the selected preview/editor map and the actual emulator map/position to the agent, keeping selection, launch target and live location distinct and marking unavailable/stale state. Show an explicit agent-held state instead of presenting the 10 Hz frame refresh as gameplay FPS. After an agent build, provide a direct preview at the selected/current target without a manual New Game sequence; keep the target and supported flags explicit. Owner map-switch follow-up 2026-11-20: prevent stale out-of-bounds coordinates when changing maps and reject invalid targets before building; add a separate preview action using the existing built ROM/symbol pair without rebuilding the ROM. Document that this is a fresh boot-to-point, not instant live-state teleportation, and that bounded defaults do not guarantee walkability. Refs: plan §"Layered roadmap toward the beta" (Phase 5), plan §"Layered architecture" (layer 1).
  Completion: From an open map, one action builds and boots there; agent context distinguishes selected map from actual emulator location, unavailable context is explicit, and agent-held refresh is labelled clearly. An agent build can be previewed directly at the intended map without manual New Game navigation; record the Owner manual check and supported state-preservation limits. Invalid targets are rejected before build/reload, and the separate built-ROM preview does not invoke the ROM builder.

- [x] **Done** **APP-004** Change history with undo.
  Depends on: `G3-001`, `EMU-004`.
  Responsible: Agent
  Owner: Antigravity
  Scope: History view over the snapshot commits with undo/restore of agent and manual changes. Refs: plan §"Layered roadmap toward the beta" (Phase 5), plan §"Rules for the agents writing the code" (Never destructive).
  Completion: Tests show undo restores the previous snapshot exactly; the view lists changes with their origin.
  Verification: 2026-11-18 UTC: added `crates/app/src/history.rs` (`list_history`, `undo_to`, `ChangeOrigin`, `HistoryEntry`, `UndoReceipt`) reading G3-001 commits and `Tauros-Provenance`/`Tauros-Snapshot`/`Tauros-Undo-Target` trailers, wired `history_list` and `history_undo` Tauri commands in `crates/app/src/main.rs`, and added the History & Undo panel in `crates/app/ui/index.html` and `crates/app/ui/main.js` showing each commit's origin/provenance and Restore button. `cargo test -p tauros-app --test history_undo -- --nocapture` passed both tests: `lists_agent_and_manual_changes_and_restores_previous_snapshot_exactly` verified listing Human, AiGenerated, External, Snapshot and Undo entries, snapshotting pending dirty work before undo, restoring modified/CRLF files and removing post-snapshot files byte-for-byte while preserving ignored build outputs, reversible undo-of-undo and no-op undo; `undo_rejects_unknown_commit_without_modifying_tree` verified clean rejection. Full `cargo test --workspace`, `cargo clippy -p tauros-app --tests -- -D warnings` and 56 Python tests all passed. Limits: the UI History panel was compiled and wired into Tauri, not visually clicked in a live window.

- [ ] **Not started** **APP-005** Beta integration test and README update.
  Depends on: `APP-001`, `APP-002`, `APP-003`, `APP-004`, `AGT-003`, `AGT-004`, `MCP-004`, `MCP-005`.
  Responsible: Agent
  Owner: unassigned
  Note: Owner accepted AGT-006 on 2026-11-20 and deferred interface polish. Retain explicit desktop Codex Stop/retry, provider switching and history/partial-edit checks here as beta integration acceptance; the adapter closure does not claim these manual checks passed.
  Scope: One end-to-end check covering map, trainer/scene editing, chat, voice, preview and undo, including a narrated request (a sequence of story events told in the chat, per master.md "Agent reach over the whole game (Owner direction)") that the agent turns into a built, verified and previewed scene; README documents beta capabilities and gaps. Refs: plan §"Layered roadmap toward the beta" (Phase 5 beta), plan §"Rules for the agents writing the code" (End of phase).
  Completion: Integration test passes; Owner narrates a sequence of events in the chat and confirms the resulting scene in the preview, with its verification verdict recorded; Owner confirms the beta flow; README updated.

- [ ] **Not started** **MCP-005** Generic query surface over the model and read-only project access.
  Depends on: `MCP-002`, `AGT-003`.
  Responsible: Agent
  Owner: unassigned
  Scope: Instead of one tool per missing question, generic tools driven by the semantic model: `describe` (entity kinds, their fields, game capabilities and the game's constant vocabularies such as weather, music and movements), `query` (kind plus filter) and `get` (kind plus id) for every kind the model represents, so new model coverage reaches the agent without new tools; plus read-only search and reading of the project's source files, confined to the project directory, for what the model does not cover. This task adds read-only access; native source writes are covered separately by `AGT-008`, with the same snapshot guarantees. Gaps found while using the chat are added here only when they recur. Per master.md "Agent reach over the whole game (Owner direction)". Refs: plan §"Layered architecture" (layer 2), plan §"Layered roadmap toward the beta" (Phase 5).
  Completion: Contract tests show describe, query and get over every model kind against the Gen 3 connector, and read-only access refusing paths outside the project and any write; in the app chat the agent lists the trainers whose id contains DIRECTOR on a real project (manual check recorded).

- [x] **Done** **MCP-006** Compact build and test results for agents.
  Depends on: `MCP-002`.
  Responsible: Agent
  Owner: Claude Code
  Note: Full build logs are not returned to agents; offering them would be a separate task.
  Scope: Requested by the Owner on 2026-11-20 after a Claude turn could not read a successful build result: `build` returned 413 warning diagnostics (66 KB), the Agent SDK spilled it to a file outside the project, and the confined native shell could not open it. Keep the severity of each diagnostic in the model (lost when the Gen 3 connector converts its build report), and make `build`, `run_test` and `build_failed` errors return a bounded summary: outcome, ROM hash and paths, error and warning counts, every error, at most 20 distinct warnings and the number omitted, with long messages shortened. Full logs stay out of agent results. Refs: plan §"Layered architecture" (layer 3), plan §"Rules for the agents writing the code" (Targeted tests).
  Completion: Tests show severity preserved from the connector, a build with hundreds of warnings yielding a result of a few KB with counts and every error kept, and failed builds and test runs bounded the same way; the real project build result is measured before and after on director-scene-mcp004; master.md and README record the format.
  Verification: 2026-11-20 UTC: cause confirmed from the Claude transcript (66,024-byte `build` result with 413 warnings spilled outside the project; confined `head` failed). Implemented `Diagnostic::severity` set from the Gen 3 toolchain level, `domain::compact_diagnostics`/`DiagnosticCounts`, bounded `build`, `run_test` and `build_failed` payloads and updated tool descriptions. `cargo test -p tauros-mcp --test compact_results` passed (4 tests: noisy build under 8 KB with full counts, 50 errors first on failure, small results unchanged, 400 cases under 16 KB with failures first). Real director-scene-mcp004 build through `tauros-mcp-project`: 66,024 bytes with the previous code, 4,174 bytes now (`errors 0, warnings 413, omitted 393`); project unchanged afterwards. `TAUROS_DOCKER_TESTS=1` `mcp_domain_gen3` (3) and `toolchain_build` (2, 1 ignored) passed; `cargo clippy --workspace --tests -- -D warnings`, tests of model, mcp, connector-gen3, agent-bridge and app (40 suites) and 56 Python tests passed. Limits: no new live agent turn on the bounded result; no commit.

- [ ] **Not started** **PROV-001** Show and export provenance labels in the app.
  Depends on: `APP-004`, `MOD-001`.
  Responsible: Agent
  Owner: unassigned
  Scope: Display the AI-Generated / AI-Assisted / Human label of every changed entity in the change history and editors, let the human promote a label when they intervene, and export a per-project provenance report suitable for a release note. Per master.md "Provenance labels (DEC-003)". Refs: plan §"Risks and community notes" (Positioning).
  Completion: Tests show labels follow the definitions (an AI change becomes AI-Assisted after a human edit, never back to AI-Generated); the export lists every labelled entity with its label and commit.

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
- plan §"Layered architecture": `FND-001`, `MOD-001`, `MCP-002`, `MCP-005`, `AGT-008`, `EMU-001`, `APP-003`.
- plan §"Decisions made": `RES-001`, `RES-002`, `RES-003`, `DEC-001`, `DEC-002`, `FND-001`, `MCP-001`, `AGT-001`, `AGT-002`, `AGT-004`, `AGT-008`, `EMU-004`.
- plan §"Layered roadmap toward the beta": Phase 0 `FND-001`, `FND-002`, `RES-001`; Phase 1 `MOD-001`, `G3-001`, `G3-002`, `G3-003`, `G3-004`, `G3-005`, `G3-006`, `G3-007`, `G3-008`, `G3-009`, `G3-010`, `G3-011`; Phase 3 design `VER-001`; Phase 2 `EMU-001`, `EMU-002`, `EMU-003`, `EMU-004`, `EMU-005`; Phase 3 `MCP-001`, `MCP-002`, `MCP-003`, `MCP-004`, `MCP-006`; Phase 4 `AGT-001`, `AGT-002`, `AGT-003`, `AGT-004`, `AGT-005`, `AGT-006`, `AGT-007`, `AGT-008`, `AGT-009`, `AGT-010`, `AGT-011`; Phase 5 `APP-001`, `APP-002`, `APP-003`, `APP-004`, `APP-005`, `MCP-005`; Phase 6 `GFX-001`, `GFX-002`, `GFX-003`, `GFX-004`.
- plan §"Rules for the agents writing the code": `GOV-002`, `AGT-008`, `G3-001`, `G3-002`, `G3-003`, `G3-006`, `G3-009`, `G3-010`, `G3-011`, `EMU-005`, `MCP-004`, `APP-005`, `FND-001`, `DEC-001`.
- plan §"Mature tools to integrate as they are": `FND-002`, `G3-003`, `G3-004`, `G3-005`, `GB-001`.
- plan §"Young projects to take ideas from": `RES-001`, `DOC-001`, `VER-001`, `G3-007`, `DEC-002`, `MCP-003`, `GFX-003`.
- plan §"Graphics assets and visual enhancements": `GFX-001`, `GFX-002`, `GFX-004`.
- plan §"Beyond Gen 3: Gen 1-2 and DS": `MOD-001`, `GB-001`, `GB-002`, `NDS-001`.
- plan §"Risks and community notes": `GOV-003`, `PROV-001`, `DEC-001`, `DEC-002`, `DEC-003`, `G3-001`, `GFX-004`.
- plan §"Next steps": `GOV-001`, `FND-001`, `FND-002`, `RES-001`, `RES-002`.
- plan §"Sources": reference list only; cited by the tasks above, no own work.

## Retired IDs

None yet.
