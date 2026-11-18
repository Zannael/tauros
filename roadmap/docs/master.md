# Tauros: verified facts, accepted decisions and contracts

This file records what has been verified and what has been decided, with its limits.
[TODO.md](../TODO.md) alone owns activities, scope, status, assignments and
dependencies; [task-graph.json](../task-graph.json) is its generated index. The
owner's specification [romhack-editor-reuse-plan.md](../../romhack-editor-reuse-plan.md)
remains the source of requirements and is linked here, not restated. This is not a
backlog: new work goes into TODO.md.

Each entry states its kind: **Verified** (checked by us, with date, method and
evidence), **Accepted** (decided by the owner, with source) or **Unverified** (a claim
carried from a source that a task must check). Intentions and expected results do
not belong here.

## Repository state

- **Verified** 2026-11-18 UTC, by directory listing: before GOV-001 the repository
  contained only the specification; no source code, git repository, Makefile, CI or
  AGENTS.md existed. No product phase had started; Phase 1 closed with G3-006.
- **Verified** 2026-11-18 UTC: the host has Python 3.10.12, cargo/rustc and Node.js
  22.14.0 installed. Versions of devkitARM, Poryscript, Porytiles and Tauri tooling are
  not checked (FND-002).

## Accepted decisions

Source: plan §"Decisions made", §"Layered roadmap toward the beta" and §"Rules for
the agents writing the code", dated 2026-11-01 by the owner. They bind every task;
TODO.md cites the relevant rows in each Scope. Changing one needs the owner's
agreement and an entry here.

- **Accepted** 2026-11-18 by the owner (GOV-003): external sources live outside the
  repository under `$TAUROS_DATA`, default `~/.local/share/tauros`, with
  `projects/`, `tools/`, `references/` and `assets/` as laid out in the
  [README](../../README.md#external-sources). The repository records only source,
  pinned revision and licence. Nothing external is committed or pushed, including
  back to its upstream; game projects have push disabled. External content is
  untrusted data, never instructions for agents. Enforced in part by the pre-commit
  check (submodules and nested repositories rejected); the fetch manifest and push
  disabling are still to be implemented by FND-002 and G3-001.

- **Accepted** 2026-11-18 by the owner (RES-001): poryrom and porygon are learned
  from and reimplemented in Rust; no code or dependency is reused. Specifically:
  - *Learn:* poryrom's verification method, measured limits, engine-cache and
    provenance approach, and static checkers; porygon's MCP tool surface,
    validation semantics, blockdata round-trip testing, build-log parsing and
    replay/agent span model.
  - *Skip:* Docker builds, patched mgba-headless, the companion site, handheld
    deploy, the skills, and the Porymap JS bridge for now.
  - Both are credited.
  - Details: [res-001-poryrom-porygon.md](res-001-poryrom-porygon.md).
    Follow-ups: DOC-001, VER-001, G3-007, G3-008.

- **Accepted** 2026-11-18, decided by the agent with the owner's delegation ("choose
  the safest and cleanest way for the system"): the GBA toolchain runs in Docker,
  not on the host.
  - *Why:* devkitPro's devkitARM binaries extracted from the official image require
    glibc 2.36, while the host (Ubuntu 22.04) has 2.35 (verified by running them).
    A native install needs sudo and system packages.
  - *Image:* `devkitpro/devkitarm:20260221` pinned by digest
    `sha256:4debd5b33cf4361a557b6bf3be5ff823804868125ce1429912f1a4e773e7ac5d`, already
    present locally and also used by another of the owner's projects. On top:
    `libpng-dev`, Poryscript 3.6.1 and Porytiles 2.0.0, both downloaded with sha256
    verification.
  - *Hardening (as in opeNSUI's compose):* builds run with `--network none`, the
    host uid/gid, `--cap-drop ALL`, `no-new-privileges`, a read-only root and a tmpfs
    `/tmp`, mounting only the game project directory.
  - *Engine:* pokeemerald-expansion pinned at `expansion/1.16.4` (commit `3796c72`),
    the latest 1.16 patch, as the plan targets 1.16.x; 1.17.2 exists and is not
    adopted. Pristine clones live in `$TAUROS_DATA/engines/` and are never built in
    place.
  - *Supersedes* the RES-001 note that Docker builds were skipped: that referred to
    poryrom's image, not to Docker as such.
  - *Consequence:* Docker is a runtime requirement of Tauros.

## Project licence (DEC-001)

**Accepted** 2026-11-18 by the owner: Tauros is MIT-licensed. It is compatible with
the DEC-002 obligations: MIT code may link MPL-2.0, LGPL-2.1 and BSD components, and
binary releases ship their notices and point to their sources. Hacks made with
Tauros do not inherit its licence.

## Semantic model mapping (MOD-001)

**Verified** 2026-11-18 UTC.
- *Code:* the types live in `crates/model` (`tauros-model`).
- *Gen 3 paths:* read in the pinned expansion (commit `3796c72`).
- *Gen 1-2 and DS paths:* listed through the GitHub API at pret/pokered `ba9c056`,
  pret/pokecrystal `9ce4c4e` and pret/pokeplatinum `62bdaac`. Cells marked *(to
  verify)* were not opened and belong to GB-001 / NDS-001.

| Model type | Gen 3 (pokeemerald-expansion) | Gen 1-2 (pokered / pokecrystal) | DS (pokeplatinum) |
| --- | --- | --- | --- |
| `Trainer`, `Party`, `PartyMember` | `src/data/trainers.party`: `=== TRAINER_X ===` blocks in Showdown syntax with display names (`Geodude @ Oran Berry`). The connector maps names to constants. | `data/trainers/parties.asm` in both; level, species and, in Crystal, items and moves *(field layout to verify)* | `res/trainers/data/<trainer>.json`: `name`, `class`, `items`, `ai_flags`, `double_battle`, `party[]` with `species`, `level`, `item`, `moves`; `form`, `iv_scale` and `ball_seal` go into `extensions` |
| `Map`, `MapGeometry::Grid`, `Connection` | `data/maps/<Map>/map.json` (id, name, layout, music, weather, connections…), `data/layouts/layouts.json`, binary `map.bin`/`border.bin` | pokered `data/maps/headers`, `data/maps/objects`, `maps/*.blk`; pokecrystal `maps/<Map>.asm` + `.blk` | `res/field/maps/<map>/`. 3D terrain is `MapGeometry::External`, edited outside the shared model |
| `Event` (`Npc`, `Warp`, `Trigger`, `Background`) | `map.json` `object_events`, `warp_events`, `coord_events` (`type: trigger`), `bg_events` (`type: sign` etc.) | object, warp, coord and bg event lists in the map object files *(to verify)* | *(to verify)* |
| `Scene` | `data/maps/<Map>/scripts.inc` (`Assembly`) or `.pory` (`Poryscript`) | `scripts/*.asm` (pokered), `maps/<Map>.asm` script sections (pokecrystal) | `res/field/scripts/` (`Other`) |
| `Species` | `include/constants/species.h` and `src/data/pokemon/species_info/` | *(to verify)* | *(to verify)* |
| `Flag` | `include/constants/flags.h` (2301 `FLAG_` defines) | pokered `constants/event_constants.asm`; pokecrystal `constants/event_flags.asm`, `engine_flags.asm` | *(to verify)* |

Design decisions:
- *The model is a view.* Connectors keep the files as the source of truth and apply
  `ChangeSet`s losslessly. Game-specific fields ride in `Extensions`.
- *Identifiers* are the game's constants.
- *Events* are addressed by kind and index, because order is meaningful.
- *Every `ChangeSet`* carries a summary and a `Provenance` label.
- *Validation:* generic rules in `validate` check change sets in order against a
  working copy (party size, levels, moves, held items, abilities, species, warp
  destinations, event, trainer and scene existence, unique flags). Game limits are
  added per connector (G3-008).
- *Contract:* the suite (`contract::assert_contract`) has 8 checks: determinism,
  sane capabilities, empty no-op, lossless unchanged rewrite, local and visible
  edits, rejection without writes, missing targets, atomicity.

## Provenance labels (DEC-003)

**Accepted** 2026-11-18 by the owner: a provenance label is attached wherever AI can
intervene. That covers scripts, wiring, text, data, maps and graphics, in the app,
in commits and in exported reports. The owner's definitions:

| Label | Definition |
| --- | --- |
| **AI-Generated** | The thing was conceived and built entirely by AI, with no human taking part anywhere in the process. |
| **AI-Assisted** | A human took part in at least one step between conception and completion; AI was also used. |
| **Human** | AI was never used for that thing. |

Interpretations to be confirmed by the owner. They are applied as defaults in the
implementation:
- *Approval only:* a human who only approves an AI result without changing it counts
  as taking part, so the result is AI-Assisted. The alternative would keep it
  AI-Generated.
- *Monotonic labels:* a human edit turns AI-Generated into AI-Assisted, and an AI
  edit turns Human into AI-Assisted. Nothing ever returns to AI-Generated or Human.
- *Aggregates* such as a map or a release take the most AI-involved label among
  their parts (Generated > Assisted > Human).

Implementation: MOD-001 (type and change-set field), G3-001 (commit trailer
`Tauros-Provenance:`), PROV-001 (app display and export).

## libmgba consumption (DEC-002)

**Accepted** 2026-11-18 (agent-owned decision task; the owner asked to proceed):
- *No mgba-rs dependency.* mgba-rs (ocnc, MIT, crates `mgba`/`mgba-sys` 0.1.0, single
  commit `f8c4c81`) vendors an mGBA 0.11-dev snapshot without recording its
  upstream commit. Its safe API covers only ROM, frames, keys, video and audio, and
  its maintenance is unknown.
- *Instead:* our own `tauros-mgba-sys` crate builds **unmodified mGBA 0.10.5**
  (latest release, tag commit `26b7884bc25a5933960f3cdcd98bac1ae14d42e2`, MPL-2.0)
  from `$TAUROS_DATA/sources/mgba-0.10.5`, fetched and pinned by
  `scripts/setup_toolchain.py`.
  - It uses a cmake static build plus bindgen; the approach is learned from
    mgba-rs, with credit.
  - Configuration: GBA and GB cores on, scripting/Lua, debuggers, Qt, SDL and
    ffmpeg off. Rust reads memory directly, so Lua is not needed.
  - Only `tauros-emu` may depend on it; the safe wrapper and the emulator trait
    come in EMU-001.
- *Linking: static*, matching the plan's single binary. The static `libmgba.a`
  contains mGBA's own MPL-2.0 code, `blip_buf` (LGPL-2.1) and `inih` (BSD-3);
  zlib comes from the system.

  For personal use nothing applies. On any binary distribution:
  1. state where the unmodified libmgba 0.10.5 source is available (MPL-2.0);
  2. provide the complete corresponding source of Tauros, or object files, so users
     can relink against a modified `blip_buf` (LGPL-2.1 §6);
  3. ship the MPL, LGPL and BSD notices.

  These are satisfied by an open-source release with a third-party notices file.
  DEC-001 must pick a licence compatible with this.

**Verified** 2026-11-18 UTC:
- `cargo test -p tauros-mgba-sys` builds libmgba 0.10.5 and passes the ROM-less
  init and teardown test; `projectVersion` reports `0.10.5`.
- The ignored prototype test, run with `TAUROS_BASE_ROM` set to the FND-002 base
  ROM, passed:
  - it loaded the ROM, ran 120 frames with a non-blank framebuffer;
  - it read the cartridge game code `BPEE` at 0x080000AC through `busRead8`;
  - it saved and reloaded a savestate.
- `ar t libmgba.a` lists `blip_buf.c.o`, `ini.c.o` and `png-io.c.o`.

**Verified** 2026-11-18 UTC (EMU-001): `tauros-emu` exposes the `Emulator` trait
and `Mgba`, its libmgba 0.10.5 GBA implementation (all FFI in `crates/emu/src/mgba.rs`;
`tauros-mgba-sys` gained bindings for `GBAIsROM`, `blip_*` and `GBAKey`).
- Facts measured with `cargo test -p tauros-emu`: mGBA boots a ROM that has no
  Nintendo logo through its HLE BIOS. Its 32-bit colour is `0x00BBGGRR`, and BGR555
  full red/blue come out as 255. The GBA core produced 37936 stereo frames at
  32768 Hz over 70 frames, 1.2% under the nominal 59.7275 Hz rate. Two cores fed
  the same ROM and input produce identical frames after 400 frames.
- `GBAIsROM` rejects a ROM without the `0xEA` entry-branch byte at offset 3, but
  accepts one whose 0xB2 byte is not `0x96`.
- With the user-built Emerald base ROM, the audio peak over the first 400 frames
  is 9249. Pressing Start changes 38382 of the 38400 pixels after 200 frames,
  compared with an idle twin.

Decisions (agent-owned, EMU-001): the trait is synchronous and frame-stepped. The
caller owns pacing, threads and audio output, so the app and agent tools can drive
it alike. Audio is drained per call as interleaved `i16` at 32768 Hz, with a
2048-frame buffer inside the core. mGBA's stderr logging is replaced by a silent
logger. `Mgba` is `Send`, not `Sync`. The test cartridge is original assembly
(`crates/emu/tests/probe/probe.s`) embedded as bytes, so the always-on tests need
no ROM.

## Toolchain and base build (FND-002)

**Verified** 2026-11-18 UTC by running `scripts/setup_toolchain.py` (pins in
[toolchain/sources.json](../../toolchain/sources.json)):
- *Image:* `tauros-gba-toolchain:349743719ad7`.
- *Tools:* devkitARM `arm-none-eabi-gcc` 15.2.0, Poryscript 3.6.1, Porytiles 2.0.0,
  libpng-dev 1.6.39-2+deb12u6.
- *Engine:* pokeemerald-expansion `expansion/1.16.4` at
  `3796c72237d2ebf0361c6f4a1b4be551d8858904`, built unmodified as `pokeemerald.gba`.
  - ROM sha256 `267819561bc027dc12373757d325626d7918ce9ea47773415712e2ea5528ddba`
  - `.sym` sha256 `8fb8f2b4fed8877d720b0a0a02e898d97b3abdc6c1935575626ea0d9f85c16e5`
  - Both come from one ELF; the pair is recorded beside the build in
    `$TAUROS_DATA/engines/…-base/tauros-base-build.json`. This is the reference
    "base ROM" for VER-001 counterfactuals.
- *Build facts at this pin:*
  - The expansion refuses to build a tree without git history unless `.histignore`
    exists.
  - `make syms` run separately relinks the ELF after the ROM was produced, so
    always build `rom syms` in one invocation.
- *Not yet verified:* that the ROM boots. That belongs to G3-006 and the emulator
  work.

**Verified** 2026-11-18 UTC (G3-005): the expansion's bundled test runner
(`tools/mgba/mgba-rom-test`) needs `libelf.so.1`, missing from the image above.
`libelf1` 0.188-2.1 was added to the Dockerfile; the image is now
`tauros-gba-toolchain:d75267a55161`, and `setup_toolchain.py install` rebuilt the
base ROM with the same ROM and `.sym` sha256 as above (the build is reproducible
across the two images).

## Engine limits at expansion 1.16.4 (G3-007)

**Verified** 2026-11-18 UTC by reading the pinned source (commit `3796c72`) and
scanning its data files. These are static checks; runtime confirmation waits for the
emulator. The baseline is poryrom's measurements at 1.9.4 (credited, RES-001).

| Limit | Value at our pin | Source | Status vs 1.9.4 |
| --- | --- | --- | --- |
| Map size | `(w+15)×(h+14) ≤ 10240` blocks (`MAP_OFFSET_W` 15, `MAP_OFFSET_H` 14, `MAX_MAP_DATA_SIZE` 10240) | include/fieldmap.h:15-25; src/fieldmap.c:170-174 | **Confirmed.** Width and height are set before the size test, which returns silently and leaves the buffer `MAPGRID_UNDEFINED`. |
| Object events on screen | 16 slots (`OBJECT_EVENTS_COUNT`), one used by the player: 15 NPCs. Follower Pokémon would take another, but `OW_FOLLOWERS_ENABLED` is FALSE by default | include/constants/global.h:95; include/config/overworld.h:61 | **Confirmed.** |
| Object event templates per map | 64 (`OBJECT_EVENT_TEMPLATES_COUNT`) | include/constants/global.h:100 | New entry (not in the 1.9.4 notes read) |
| Sprite palettes | 16 OBJ palette slots (`sSpritePaletteTags[16]`); when none is free in the overworld, the engine first despawns generated overworld encounters; if still full, `IndexOfSpritePaletteTag` returns 0xFF, stored in the 4-bit `oam.paletteNum` as slot 15, so the sprite silently uses another palette | src/sprite.c:255, 1640-1668, 535, 1717 | **Confirmed, with new mitigation.** The encounter eviction did not appear in the 1.9.4 notes. Weather allocates its own palette tags (src/field_weather.c:201, 926). |
| Metatile behaviours | 240 defined (`NUM_METATILE_BEHAVIORS`), 8-bit field (`METATILE_ATTR_BEHAVIOR_MASK` 0xFF), 255 = `MB_INVALID`: ids 240–254 are appendable. 27 `MB_UNUSED_*` names; scanning all 134 `metatile_attributes.bin` files finds 2 of them live (`MB_UNUSED_SOOTOPOLIS_DEEP_WATER` id 24, 6 metatiles; `_2` id 26, 1 metatile); no id ≥ 240 in data | include/constants/metatile_behaviors.h; include/global.fieldmap.h:39 | **Changed/qualified.** Not "full": 15 ids can be appended and 25 unused names are dead in data. |
| Shared primary tileset | `gTileset_General` uses 512/512 tiles and 512/512 metatiles (`NUM_TILES_IN_PRIMARY`, `NUM_METATILES_IN_PRIMARY` 512); primary palettes 6 of 13 | include/fieldmap.h:8-14; data/tilesets/primary/general | **Confirmed full.** |
| Item ids and saves | All 872 items have explicit ids (1–873, `ITEMS_COUNT` after them). Bag slots store the raw `enum Item` (packed) | include/constants/items.h; include/global.h:675-679, 1087-1090 | **Changed.** Adding an item appends an id and renumbers nothing. Changing an existing id still silently shifts saved bag contents. |
| Poké Balls | 27 balls; ball id comes from the item's `secondaryId` (`ItemIdToBallId`); the Pokémon stores it in 6 bits (max 63) | include/constants/pokeball.h; src/pokeball.c:1441; include/pokemon.h:141 | **Changed.** Adding a ball no longer forces item renumbering. The new limit is 63 ball ids. |
| Save sectors | 3968 data bytes per sector (`SECTOR_DATA_SIZE`); `SaveBlock3` size statically asserted | include/save.h:7; src/save.c:80 | Size limits are checked at compile time; same-size layout changes remain undetected (structural, not re-measured). |

Rules for G3-008:
- Reject oversize maps, more than 15 simultaneous NPCs in view, renumbering of
  existing item ids, and new behaviours other than appended or confirmed-dead ids.
- Warn on maps that need more than the free sprite palette slots.

## Gen 3 connector (G3-002..G3-005)

**Verified** 2026-11-18 UTC on `git archive` copies of the pinned expansion
(`3796c72`), by the tests in `crates/connector-gen3/tests`; evidence and limits per
task are in the TODO's Verification fields. Code: `crates/connector-gen3`
(`Gen3Connector`).

Facts about the pinned data:
- *Trainers:* src/data/trainers.party holds 855 trainers, trainers_frlg.party 624,
  battle_partners.party and debug_trainers.party 2 each. trainerproc maps display
  names to constants by fixed rules (`fprint_constant`, `fprint_species`) plus two
  tables (gendered species such as `Nidoran (M)`, item-selected forms such as
  `Arceus-Fire`); with those rules every name in the four files resolves to a
  constant the headers define. A Pokémon without `Level:` gets level 100.
- *Maps:* 939 `map.json` files. 95 of them (FRLG) write empty arrays as `[\n\n  ]`,
  so re-serialising JSON is not lossless; `connections` is a list, `null` or `0`.
  `dest_warp_id` is a string, numeric or `WARP_ID_DYNAMIC`/`WARP_ID_SECRET_BASE`
  (include/constants/maps.h). `MAP_DYNAMIC` is a pseudo-map used as a warp
  destination. Coord events are `trigger` or `weather`; bg events `sign`,
  `hidden_item` or `secret_base`; object events `object` or `clone`.
- *Layouts:* 785 layouts; 20 `map.bin` files hold one block more than
  `layouts.json` declares (unused or alternate layouts); the engine reads only
  width × height blocks. 134 tilesets: Emerald attributes are 16-bit, FRLG 32-bit.
- *Scripts:* 16 570 distinct global labels in map and shared `scripts.inc` files;
  one, `MtChimney_EventScript_BagIsFull`, is defined twice, in the two branches of
  an `.if`. `data/event_scripts.s` includes the FRLG maps' scripts inside `.if IS_FRLG`.
- *Flags:* flags.h has 2301 `FLAG_` defines for the Emerald build (FRLG's are in
  flags_frlg.h); 1787 are referenced outside their definition; 373 are
  `FLAG_UNUSED_*` placeholders, 2 of which are referenced. Event flags lie between
  `TEMP_FLAGS_END` + 1 (0x20) and `TRAINER_FLAGS_START` (0x500).
- *Build:* clean `make rom syms` of a full copy takes 90–95 s with 16 jobs, a
  no-change rebuild about 2 s. `make check TESTS='<prefix>'` runs a filtered
  subset of the expansion's tests in the container.

Decisions (agent-owned, within the tasks' scope):
- *Lossless by construction.* `.party` files are split into trainer blocks and
  edited line by line; JSON is parsed into a tree that keeps each scalar's text and
  the whitespace around members; script files are split at global labels. An
  unchanged entity is never re-rendered, and changed events are aligned with a
  longest common subsequence so insertions and removals leave other events alone.
- *Names:* the model holds constants; writes reuse the file's spelling, then an
  in-game name that resolves back to the constant, then the constant itself.
  trainerproc's naming rules are reimplemented; its two species tables are read at
  run time from the project's `tools/trainerproc/main.c`, not copied into Tauros.
- *Events* are written grouped by kind (one JSON array each), so the model order
  is meaningful within a kind only.
- *Scenes:* existing scripts are `Assembly` scenes (one per global label). A
  Poryscript scene is stored as `data/maps/<Map>/scenes/<Label>.pory` (or
  `data/scripts/scenes/` without a map), compiled by Tauros into `<Label>.inc` and
  included right after its map's `scripts.inc`, so it is assembled under the same
  conditions as the map.
- *Flag allocation* renames one unreferenced `FLAG_UNUSED_*` placeholder in the
  event range whose value no other flag shares (lowest first), rewriting only that
  line; it never invents a value.
- *Git snapshots are byte-exact:* the G3-001 guard runs git with
  `core.autocrlf=false`, since the owner's global `core.autocrlf=input` would
  normalise CRLF palettes on snapshot and restore.
- *Validation (MOD-001 amendment):* an upsert identical to the current entity is
  not re-checked, so a project's own data (an empty `TRAINER_NONE`, warps to
  `MAP_DYNAMIC`) never fails validation when rewritten unchanged.
- *Not yet supported, rejected before writing:* new trainer constants (G3-009),
  new maps and layout resizing (G3-010), editing trainer blocks that contain C
  comments, labels defined twice, moving scenes between maps.

**Verified** 2026-11-18 UTC (G3-003): the owner opened the prepared
`porymap-check-g3-003` project (edit commit `5c7d71e9`) in Porymap 6.3.1
installed from Flathub on Linux, with game version `pokeemerald`. The screenshot
supplied in chat shows Route101 rendered and the added NPC selected at
x=12, y=10, elevation=3, with `OBJ_EVENT_GFX_BOY_1`,
`MOVEMENT_TYPE_FACE_DOWN` and `Route101_EventScript_Boy`. This closes the
previously missing edited-map opening check. Limits: one edited map was inspected;
saving/reopening in Porymap and GUI inspection of every map were not checked.
The map/layout and block/metatile roundtrip tests were rerun and passed that day
(939 maps, 785 layouts, 9058 events, 1570 binary files, 134 tilesets); the earlier
container build evidence remains recorded in TODO.md under G3-004/G3-005.

**Verified** 2026-11-18 UTC (G3-006): Phase 1 is complete. On a copy of the base
build, raising TRAINER_ROXANNE_1's lead from level 12 to 13 through
`Gen3Connector::apply` and building with `make rom syms` in the container yields a
ROM of the same size that differs from the base ROM in exactly one byte (ROM offset
`0x479db2`, 12 to 13). That byte lies 26 bytes into the party that
`gTrainers[DIFFICULTY_NORMAL][TRAINER_ROXANNE_1]` points to (`0x8479d98`); the
table is at `0x844d8d8` with 3 x 855 rows of 52 bytes, derived from the `.sym` size
and the project's headers. The base build is therefore byte-reproducible for an
unchanged source. Method: byte-level test runner, not an emulator; the trainer was
not observed in a running battle.

## Agent SDK authentication (RES-002)

**Verified** 2026-11-18 UTC by reading official Anthropic pages (accessed that day):
[Agent SDK overview](https://code.claude.com/docs/en/agent-sdk/overview),
[Legal and compliance](https://code.claude.com/docs/en/legal-and-compliance),
[Authentication](https://code.claude.com/docs/en/authentication),
[Use the Claude Agent SDK with your Claude plan](https://support.claude.com/en/articles/15036540-use-the-claude-agent-sdk-with-your-claude-plan),
[Monthly API credits for Max, Ultra, and Team plans](https://support.claude.com/en/articles/17154008-monthly-api-credits-for-max-ultra-and-team-plans).

- Signed in with a Claude plan, the Agent SDK draws from the plan's usage limits.
  Anthropic paused previously announced changes to this on 15 June 2026. Advertised
  Pro/Max/Ultra limits assume ordinary, individual use of Claude Code and the SDK.
  The SDK runs the unmodified Claude Code binary, which reads the user's own login
  (Linux: `~/.claude/.credentials.json`, created by Anthropic's own sign-in flow).
- Since 7 October 2026, Max ($100/month), Ultra ($200/month) and Team plans include
  monthly API credits. They cover the Agent SDK only when it runs with an API key
  from the Console organization linked to the plan, not when signed in with the
  plan. Unused credits expire each billing cycle.
- Developers of products, including Agent SDK agents, must use API-key (or cloud
  provider) authentication. Unless previously approved by Anthropic, they may not
  offer claude.ai login in their applications, route requests through users'
  Free/Pro/Max/Ultra credentials, or collect, store or intermediate claude.ai
  credentials or tokens. Each end user's usage must be billed to that user.
- The Agent SDK is governed by the Commercial Terms. A product must not be named or
  styled "Claude Code"; "Powered by Claude" is allowed.

Consequences, as verified facts rather than decisions:
- The plan's premise (the SDK can use the subscription's monthly credit without an
  API key) conflates two mechanisms: plan sign-in draws plan limits, while the
  monthly API credits require an API key.
- For the owner's personal use, signing in with the owner's own plan through
  Anthropic's flow is documented as supported.
- For a public release, the app may not offer claude.ai login. Each user brings an
  API key or cloud credential, unless Anthropic approves otherwise; follow-up
  `AGT-005`.
- Not determined: which plan the owner holds. Monthly credits exist only on
  Max, Ultra and Team.

## OpenAI and Google agent runtimes (RES-003)

**Verified** 2026-11-18 UTC by reading official pages (accessed that day). Not run or
tested. Several OpenAI pages moved to learn.chatgpt.com during the check, and some
returned 404 or 403; only content actually read is recorded.

OpenAI (Codex / ChatGPT). Sources:
[Codex authentication](https://learn.chatgpt.com/docs/auth.md),
[Codex SDK](https://learn.chatgpt.com/docs/codex-sdk),
[Codex MCP](https://developers.openai.com/codex/mcp),
[Sign in with ChatGPT](https://developers.openai.com/siwc),
[SIWC in open-source apps (cookbook)](https://developers.openai.com/cookbook/articles/sign-in-with-chatgpt),
[SIWC Codex app-server](https://developers.openai.com/siwc/token-sharing-open-source/codex-app-server),
[SIWC preview limitations](https://developers.openai.com/siwc/token-sharing-open-source/preview-limitations).
- Codex CLI, IDE extension and desktop app accept either ChatGPT sign-in
  (subscription) or an API key (usage-based). Credentials are cached in
  `~/.codex/auth.json` or the OS keyring. API keys are recommended for automation.
- The Codex SDK has a TypeScript library (starts and resumes local Codex threads,
  Node 18+) and a Python library (controls the local Codex app-server over JSON-RPC).
  The `codex mcp-server` mode was removed; the app-server is the integration
  point.
- As an MCP client, Codex reads `[mcp_servers]` from `~/.codex/config.toml` or a
  project's `.codex/config.toml`, supporting stdio and Streamable HTTP servers.
- **Sign in with ChatGPT** (preview) lets an app use a user's ChatGPT plan for
  eligible requests:
  - Eligible apps: open-source projects, personal projects that run locally, and
    selected private apps.
  - Eligible users: ChatGPT Plus and Pro.
  - Registration is dynamic (`client_id=dynamic_agent_client`). Paid or remotely
    hosted apps need the waitlist.
  - Only the Responses API is covered, with `store: false` and `stream: true`;
    hosted MCP/connectors and several hosted tools are excluded.
  - Codex app-server is a documented route: the app passes the user's access token
    to the app-server child process.
  - No rate limits and no publication date are stated on the pages read.

Google (Gemini / Antigravity). Sources:
[Gemini CLI authentication](https://geminicli.com/docs/get-started/authentication/),
[Gemini CLI terms and privacy](https://geminicli.com/docs/resources/tos-privacy) (updated 2026-04-10),
[Antigravity authentication](https://antigravity.google/docs/install/),
[Antigravity SDK overview](https://antigravity.google/docs/sdk/overview/),
[Antigravity MCP](https://antigravity.google/docs/mcp/).
- Gemini CLI accepts Google sign-in (free tier and Google AI Pro/Ultra), a Gemini
  API key, or Vertex AI. The applicable terms differ per method; Pro/Ultra fall
  under the Google and Google One terms.
- Gemini CLI terms: accessing "the services powering Gemini CLI" through
  third-party software, tools or services violates the terms and may lead to account
  suspension.
- Antigravity shares one sign-in across its app, CLI, IDE extensions and SDK
  (personal Free, Pro, Ultra; Workspace plans exclude Antigravity). For headless use,
  `GEMINI_API_KEY` or Application Default Credentials are documented.
- The Antigravity SDK is Python and runs a local harness. Its overview lists only a
  Gemini API key or Vertex AI as authentication, and it supports MCP servers.
- As an MCP client, Antigravity (app, CLI and SDK) reads
  `~/.gemini/config/mcp_config.json` or a workspace's `.agents/mcp_config.json`,
  supporting stdio and Streamable HTTP servers.
- **Unverified (forum or third-party sources only):** that a Pro/Ultra subscription
  may not drive Gemini CLI or Antigravity from inside a third-party app. This is
  treated as not permitted until an official source says otherwise.

Consequences, as verified facts rather than decisions:
- **External clients:** Claude Code, Codex (CLI/IDE/app) and Gemini CLI or
  Antigravity are all MCP clients able to load a local stdio server. Each provider's
  own official client, signed in with that provider's plan, can drive the Tauros MCP
  without Tauros handling any credentials. This is the broadest compliant way to
  split agent work across providers.
- **Embedded in the app (AGT-002 and later adapters):**

  | Provider | Personal use with the owner's plan | Public release |
  | --- | --- | --- |
  | Anthropic | official sign-in (RES-002) | API key per user |
  | OpenAI | via Sign in with ChatGPT (preview; Plus/Pro; local or open-source apps) or an API key | open-source eligible for SIWC; otherwise API key |
  | Google | API key or Vertex only; consumer plans treated as not usable from Tauros | API key per user |

## Open decisions

Tracked as tasks, not here. The voice engine is chosen within AGT-004.
Record each outcome below when accepted.

## Unverified claims from the specification

These come from the plan and have not been checked by us:

- Tool versions and status: pokeemerald-expansion 1.16.x, Porymap 6.3.1 (April
  2026), mGBA 0.10+, mgba-rs 0.1 with a single commit. Pinning happens in FND-002 and
  DEC-002.
- Resolved by RES-002 (see Agent SDK authentication above): the dev.to claim about
  the Agent SDK and subscriptions is only partly correct.
- Community policies: PokéCommunity bans AI-generated content in published fan
  games; pret does not accept AI-generated code. Inputs to DEC-003.

## Contracts

- Roadmap: format and invariants are enforced by `scripts/check_roadmap.py` and
  described in the TODO's Working contract; the workflow is in `AGENTS.md`.
