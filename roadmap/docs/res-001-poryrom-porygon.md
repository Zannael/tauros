# RES-001: poryrom and porygon, analysis and reuse proposal

Task: `RES-001` in [TODO.md](../TODO.md). Requirement source: plan §"Decisions made"
(poryrom and porygon row) and §"Young projects to take ideas from" in
[the specification](../../romhack-editor-reuse-plan.md).

Status: **proposal awaiting the owner's decision.** Nothing in this report is an
accepted decision until it is recorded in [master.md](master.md).

## Method and limits

- Analysed 2026-11-18 by reading source and documentation only. Clones live under
  `$TAUROS_DATA/references/` (README External sources), push disabled.
- Nothing was built, installed or executed: no Docker image, no MCP server, no Lua,
  no tests. Every behavioural claim below is **the projects' own claim** unless it is
  marked as read from code. Their agent instructions (CLAUDE.md, skills, commands)
  were read as data only.

| | poryrom | porygon |
| --- | --- | --- |
| Repository | github.com/scotbearss/poryrom | github.com/GregTheGreek/porygon |
| Pinned commit | `36d482a114130a01cee415c8ca81b4c18a65ed2a` (2026-08-19) | `25136bc46df2f544c0a0003dd3dc376432b79d8e` (2026-07-01), v0.1.1 |
| Licence (GitHub + LICENSE file) | MIT | Apache-2.0 |
| Size | 155 files, ~28k lines (Python tools, Lua, Markdown wiki) | 73 files, ~2.7k lines of Python/Lua |
| Form | Agent skill plugin + Python tools + Docker build image | Python MCP server over a pure-core library + Claude Code plugin |
| Engine target | pokeemerald-expansion pinned at `expansion/1.9.4` (read from `tools/hx.py`) | stock pokeemerald and expansion forks |

## poryrom

**What it is.** A Claude Code / Codex / Gemini skill plugin. The agent scaffolds a
hack, builds it in Docker (devkitARM plus a patched headless mGBA), checks it
statically, and verifies it by reading the game's own RAM in a headless emulator.
It also generates a companion wiki site from the game's source on every build.

**Architecture (read from docs and code).**
- *Engine cache*: a pinned, read-only clone of the expansion, kept outside every
  repository and shared by all games.
- *Forks* are created with `git archive | tar -x`, not copied, so build products and
  `.git` are never inherited. The fork has no `.git` and no remote, so it cannot push.
- A provenance file `HACK_MANIFEST.json` records the base commit, tree hash, the
  sha256 of the stock ROM and `.sym`, and the build commands. The tool refuses to
  fork from a dirty engine tree.
- The game repository tracks only design documents, verification specs and a patch
  that replays byte-for-byte onto the pinned engine.

**Verification (read from `docs/wiki/verification-discipline.md` and `verify/`).**
- Specs are inert JSON: a timeline of frames and keys, probes reading named symbols
  at offsets, and assertions.
- The Lua driver contains no assertions; Python judges the values outside the
  emulator. A spec with no assertions is rejected.
- Every spec has a *counterfactual* twin that must fail: probe addresses shifted, a
  cold boot instead of a savestate, or wrong expected values.
- Saveblock pointers move at every battle start and map load, so they are re-read
  through `deref` on every sample and never asserted as exact addresses.
- The ROM and its `.sym` are treated as one pair; a stale `.sym` is documented as
  passing silently.

**Measured engine limits** (`skills/_shared/walls.md`,
`docs/wiki/walls-and-budgets.md`, measured at expansion 1.9.4):
- *Map size:* `(w+15)×(h+14) ≤ 10240`. Over the limit, the map silently renders as
  its border and cannot be walked.
- *NPCs:* 16 object slots, 15 usable NPCs. Extra NPCs silently do not appear.
- *Sprite palettes:* 16 dynamic slots. Overflow silently shows wrong colours, and
  weather takes one slot outdoors.
- *Metatiles:* 240/240 behaviour ids are used; `gTileset_General` is full at 512.
- *Audio:* 5 PCM voices and 4 music players. IWRAM has about 2.3 KB free.
- *Save safety:* adding a Poké Ball renumbers about 800 item ids, so older saves
  silently read the wrong items. Same-size save layout changes also corrupt
  silently.

The project's rule: if a fault renders something plausible, the check must be static
and run before the build. `check_map.py` and similar tools implement this.

**Also present.**
- Documented defects E1–E4 in the engine at its pin.
- Three patches to `mgba-headless` (video buffer, save autoload, audio) at mGBA
  commit `5157ce2`.
- Generators for maps, species and balls.
- Sprite, palette and audio pipelines.
- Handheld deployment.
- A 6.8k-line companion-site generator.

## porygon

**What it is.** A Python MCP server (`mcp==1.28.0`) over a pure-core library, packaged
as a Claude Code plugin. It is explicitly "not an autonomous game builder": the human
reviews every change.

**Tool surface (read from `mcp/src/porygon/server.py`)**, about 30 tools:
- *Project:* `project_info`, `list_maps`, `list_layouts`, `get_layout`, `read_map`.
- *Blockdata:* `read_blockdata`, `write_blockdata`, `read_metatile_attributes`.
- *Build and debug:* `build`, `parse_build_log`, `resolve_address`, `lookup_symbol`,
  `emu_launch_command`.
- *Scripts:* `validate_scripts`, `list_macros`, `lookup_macro`, `scaffold_script`,
  `poryscript_status`, plus Poryscript compilation.
- *Events:* `read_map_events`, `add_object_event`, `add_sign`, `add_trigger`,
  `add_warp`, `add_bg_event`, `remove_event`.
- *Map wiring:* `get_connections`, `edit_connection`, `set_map_properties`.

**Notable design points (read from code).**
- *Blockdata:* `map.bin` and `border.bin` are 16-bit little-endian values: bits 0–9
  metatile id, 10–11 collision, 12–15 elevation. Out-of-range values raise errors
  instead of being masked. Tests claim byte-identical round trips over every layout
  of a real checkout.
- *Map edits:* `map.json` is rewritten with `json.dumps(indent=2)` in its original
  key order, so diffs stay small. Byte identity with Porymap's own output is not
  shown in the code I read.
- *Validation:* warp destinations, `dest_warp_id`, connections and map properties are
  validated; unknown or structural keys are rejected.
- *Build:* compiler errors are parsed to `file:line`; crash addresses are resolved
  through `.sym`/`.map` and DWARF.
- *No snapshot before writes:* the core never calls git and writes files directly.
- *Porymap bridge:* custom JavaScript lets Porymap display agent-made collision for
  review.
- *Playtester* (experimental): a Lua socket bridge into mGBA on port 8888. Its
  `MODEL.md` reports that RNG-dependent spans such as battles diverge on replay even
  from a savestate, while navigation and dialogue replay reliably. Its conclusion:
  the agent must play RNG events live, while deterministic spans can be replayed.

## Comparison with Tauros

| Topic | poryrom | porygon | Tauros plan |
| --- | --- | --- | --- |
| Core language | Python + Lua + Docker | Python | Rust + Tauri |
| Agent interface | skills (free-form agent edits) | MCP domain tools | MCP domain tools (closer to porygon) |
| Safety of game data | patch backup, fork without `.git` | none | git snapshot before every write (G3-001) |
| Porymap compatibility | not a goal | yes, plus a JS bridge | required |
| Emulator | patched headless mGBA in Docker | external mGBA + Lua socket | libmgba in-process (DEC-002) |
| Verification | adversarial RAM specs + counterfactuals | build/crash diagnostics | RAM checks via MCP (MCP-003) |
| UI | none | none (Porymap) | integrated app |

Both confirm the plan's diagnosis: chat-only tools exist, and the integrated app with
a common model is the gap.

## Recommendation (proposal)

No code from either project is copied as a dependency. Both are Python, our core is
Rust, and young single-maintainer projects are not to be used as dependencies.
Concretely:

| Component | Proposal | Reason | Where it would land |
| --- | --- | --- | --- |
| poryrom verification method (inert specs, assertion-free emulator driver, counterfactual twins, `deref` for saveblocks, ROM+`.sym` pairing) | **Learn and reimplement** | Strongest idea in either project. Our MCP checks would otherwise let the agent grade itself. | MCP-003, MCP-004 |
| poryrom measured limits ("walls") | **Learn, re-measure at our pin, credit** | High value for static validation before build, but measured at 1.9.4, not 1.16.x | G3 connector validation, MCP-002, GFX-001 |
| poryrom engine-cache layout, `git archive` forks, provenance manifest | **Learn**, adapt | Matches our `$TAUROS_DATA` decision. We keep git in game projects (plan requires snapshot commits and `make` builds) but adopt clean-fork creation and a provenance manifest | FND-002, G3-001 |
| poryrom static checkers (map size, NPCs, behaviours, tilesets, dialogue) | **Learn**, reimplement in Rust | Exactly the class of silent fault a model-level validator should catch | G3-003, G3-004, GFX-001 |
| porygon MCP tool surface and validation semantics | **Learn** as a design input | Closest existing "domain MCP"; its validations (warp targets, connection offsets, unknown keys) are a useful checklist | MCP-002 |
| porygon blockdata codec and its round-trip test strategy | **Learn**; reimplement from the decomp headers | Format is defined by `include/global.fieldmap.h`; the test strategy (exhaustive u16 round trip, every layout) is the valuable part | G3-003 |
| porygon build-log parsing and symbol/crash resolution | **Learn** | Directly useful for structured build errors | G3-005 |
| porygon playtester replay/agent span model | **Learn** | Informs how the director's scene battle is tested: savestate before the RNG event, assert outcome via RAM rather than replaying input | MCP-003, MCP-004 |
| porygon Porymap JS bridge | **Skip for now** | Not a plan requirement; revisit if Porymap review becomes part of the workflow | — |
| poryrom Docker build, patched mgba-headless | **Skip** | We build with a local toolchain (FND-002) and embed libmgba (DEC-002). The patch notes are useful background for DEC-002. | DEC-002 (background) |
| poryrom companion wiki site, handheld deploy, skills and voice rules | **Skip** | Not in the plan's scope | — |

Licence duties: learning from code needs no notice. If any code or substantial
documentation is ever copied, keep the MIT notice (poryrom), or the Apache-2.0
notice plus any NOTICE file and change statements (porygon). Credit both in the
README either way, as the plan asks.

## Follow-up work if accepted

These would be added to TODO.md only after the owner accepts this proposal:

1. Re-measure the poryrom limits relevant to the model (map size, object slots,
   palettes, behaviours) at our pinned expansion, as part of or after FND-002.
2. Add a static pre-build validation step to the connector contract (MOD-001 /
   G3-003), using those measured limits.
3. Write a verification-spec design (inert specs plus counterfactuals) before
   MCP-003.
4. Add a credits section to the README for poryrom and porygon.

## Open points for the owner

- Do you accept the table above (learn and reimplement; no code or dependency reuse)?
- Should the four follow-ups above become tasks?
- poryrom pins expansion 1.9.4, while the plan targets 1.16.x. The specific pin is
  chosen in FND-002, and none of poryrom's measurements may be assumed valid at our
  pin.
