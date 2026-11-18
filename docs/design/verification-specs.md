# Verification specs

Design for task `VER-001` in the [roadmap](../../roadmap/TODO.md). It defines how Tauros
proves that a change to a game project does what an agent claims. It is the contract
that MCP-003 (emulator tools and RAM checks) and MCP-004 (director's scene) implement.

The method is learned from poryrom's verification discipline and porygon's playtester
model, studied at the commits recorded in the
[RES-001 report](../../roadmap/docs/res-001-poryrom-porygon.md). No code or text is
copied. Every engine symbol named below is **unverified**: it is an illustration
until checked against our pinned expansion (G3-007).

## Why this shape

An agent that builds a change and then grades it will tend to grade it as passing.
The design therefore removes the agent's ability to influence the verdict after the
spec is written:

1. **Specs are inert data.** A spec is JSON, never code. It cannot compute its own
   verdict.
2. **The driver asserts nothing.** The emulator-side driver runs the timeline,
   samples memory and emits raw samples. It never sees the expected values.
3. **Judgement happens outside the emulator.** A separate evaluator compares samples
   to assertions. Driver and evaluator share only the sample record format.
4. **Every spec must be able to fail.** Counterfactual runs must fail for the stated
   reason, or the spec is not accepted as proof.
5. **Static before dynamic.** If a fault would render something plausible (an
   oversize map, a palette overflow, an NPC beyond the spawn window), only a static
   check before the build can catch it. Those checks belong to G3-008, not to specs.
   A spec complements them and does not replace them.

## Components

| Component | Crate | Role |
| --- | --- | --- |
| Spec loader | `mcp` (or a future `verify` crate) | Parses and validates spec JSON; applies the rejection rules below |
| Build pair | `connector-gen3` | Produces the ROM and its symbol table and records both hashes |
| Driver | `emu` | Boots or resumes, plays the timeline, samples probes, writes samples. No assertions, no access to `assert` |
| Evaluator | `mcp` | Resolves the verdict from samples and assertions, including counterfactual inversion |
| MCP tool | `mcp` | `run_verification(spec, mode)` returns the evaluator's report, never the driver's opinion |

Specs live in the user's game project under `.tauros/verify/<name>.json`, so they are
versioned with the game by the G3-001 snapshot commits. They never live in this
repository.

## Spec format

```json
{
  "schema": 1,
  "name": "rival-loss-no-whiteout",
  "description": "What is claimed and why these probes prove it.",
  "start": { "kind": "savestate", "file": "before-rival.ss", "rom_sha256": "<hash>" },
  "timeline": [
    { "name": "talk", "frames": 120, "keys": ["A"] },
    { "name": "battle", "frames": { "random_range": [3000, 3600] }, "policy": "lose-fast" },
    { "name": "settle", "frames": 600, "keys": [] }
  ],
  "probes": [
    { "id": "outcome", "why": "...", "symbol": "gBattleOutcome", "type": "u8",
      "sample": { "at_step_end": ["battle", "settle"] } },
    { "id": "map", "why": "...", "symbol": "gSaveBlock1Ptr", "deref": true,
      "offset": 4, "type": "bytes", "length": 4, "sample": { "every": 60, "at": ["last"] } },
    { "id": "alive", "why": "Liveness: main loop advancing.", "symbol": "gMain",
      "offset": 32, "type": "u32", "sample": { "every": 30 }, "supporting": true }
  ],
  "assert": [
    { "check": "compare", "probe": "outcome", "at": "settle", "op": "==", "value": "B_OUTCOME_LOST" },
    { "check": "compare", "probe": "map", "at": "last", "op": "==", "value": "MAP_ROUTE103" },
    { "check": "strictly_increases", "probe": "alive", "supporting": true }
  ],
  "counterfactuals": ["shift", "wrong_value", "base_rom"]
}
```

### Fields

- **`start`** says where the run begins:
  - `cold_boot`: no state.
  - `savestate`: a state file plus the `rom_sha256` it was taken from.
  - `boot_to`: map, position and flags through the EMU-003 boot-to-point mechanism.

  A savestate is bound to its ROM. A spec that compares two builds must use
  `cold_boot` or `boot_to` with ROM-resident data.
- **`timeline`** is an ordered list of steps.
  - `frames` is an exact count or `{"random_range": [a, b]}`. Randomised durations
    stop a check from passing only for one exact replay.
  - `keys` holds the inputs.
  - `policy` hands an RNG-driven step to an agent-or-driver strategy instead of
    recorded input (see RNG below).
  - The step names are anchors for sampling.
- **`probes`** say what memory to read:
  - `symbol` is resolved from the paired symbol table; raw addresses are forbidden.
  - `offset` and `type` describe the value: `u8|u16|u32|s8|s16|s32|ptr|bytes`, with
    `length` for `bytes`.
  - `deref` re-reads the pointer at `symbol` on **every** sample before applying
    `offset`. It is mandatory for save-block pointers, which the engine moves at
    battle start and map load.
  - `sample` combines `every`, explicit frames in `at`, and `at_step_start` /
    `at_step_end`.
  - `why` is mandatory free text. `supporting: true` marks a probe that cannot carry
    a verdict on its own.
- **`assert`** is a list of checks against probes:
  - `compare` uses the ops `== != < <= > >= approx`.
  - Value-shape checks: `strictly_increases`, `non_decreasing`, `changes_at_least`,
    `constant`.
  - Pointer-name checks: `symbol_at`, `symbol_sequence`, `symbol_never`.
  - Count and range checks: `in_range`, `sample_count`.
  - `at` selects `first|last|all|any|<step name>|<frame>`. A named constant such as
    `B_OUTCOME_LOST` is resolved from the project's headers at build time and
    recorded in the report.
- **`counterfactuals`** lists the counterfactual runs that must fail (next section).

### Rejection rules (spec loader)

A spec is rejected, and nothing runs, when:

1. `assert` is empty, or every assertion is `supporting`.
2. A probe has no `why`, uses a raw address, or reads a save-block pointer without
   `deref`.
3. An assertion compares a save-block pointer to an exact address. Assert what it
   points to, not where it is.
4. The ROM and symbol table hashes recorded by the build do not form a pair. The
   symbol table is checked against the ROM it came from by content, never by filename.
5. A `savestate` start names a ROM hash different from the ROM under test.
6. No counterfactual is listed, or a listed one is not applicable to the spec.
7. The last sample of any asserted probe falls before the event it asserts. The
   loader computes sample frames from the timeline; there is no implicit end-of-run
   sample.

## Counterfactuals

A spec counts as proof only if each listed counterfactual makes it fail. The
evaluator runs them automatically and reports them beside the real run.

| Kind | What changes | Proves |
| --- | --- | --- |
| `shift` | every probe base moved by a fixed offset inside the same RAM region | the assertions depend on reading the right addresses |
| `wrong_value` | each exact expected value nudged, by one where it can be | the assertions are tight enough to notice a plausible near-miss |
| `base_rom` | the same spec run against the unmodified pinned base ROM | the change, not the starting state, produces the result |
| `cold_boot` | for savestate specs, start from power-on instead | the assertions depend on the resumed state |

The verdict is `PROVEN` only when the real run passes and every counterfactual fails.
If a counterfactual unexpectedly passes, the verdict is `INCONCLUSIVE`, never `PASS`,
and the report names the surviving assertions.

## Weak assertions

Some assertion forms survive a shifted address because garbage satisfies them. The
evaluator ranks assertions and refuses to let weak ones carry a verdict alone.

- **Weak (supporting only):**
  - thresholds (`> >= !=`);
  - `constant`, `in_range`, `sample_count`;
  - an exact compare against `0` or against a common fill value;
  - a negative `symbol_at`.
- **Discriminating:** an exact non-zero compare, `symbol_at`, `symbol_sequence`, and
  agreement between two independent routes to the same value. For example, the
  trainer id read through the save block equals the id read from the party struct.

At least one discriminating assertion must survive all counterfactuals by failing in
each of them.

## RNG-dependent steps

Recorded input does not replay reliably through RNG-driven events such as battles,
even from a savestate; navigation and dialogue do. Therefore:

- Deterministic steps (walking, dialogue, menus) may replay recorded keys. They must
  end against something fixed (a wall, an edge, a closed text box) so small timing
  drift re-converges.
- RNG steps use a `policy` instead of keys: a small strategy the driver executes
  live, such as `lose-fast` (always choose the first move against a much stronger
  party). The policy acts on game state; it is not an assertion.
- Assertions on RNG steps target the **outcome** (who won, which map follows, which
  flags are set), never intermediate turns.
- A savestate taken immediately before the RNG event anchors repeated attempts.

## Report

The evaluator returns:
- the verdict (`PROVEN`, `FAILED`, `INCONCLUSIVE` or `REJECTED`);
- per-assertion results for the real run and for each counterfactual;
- the ROM and symbol table hashes;
- the resolved constants;
- screenshots at step ends;
- the driver's raw sample file path.

The agent quotes the report and does not paraphrase a verdict.

## Worked example: the director's scene

The plan's Phase 3 acceptance scene: a trainer with three Pokémon and a lost battle
without whiteout, verified and previewed. All symbols and constants are unverified
until G3-007 checks them at our pin, and the no-whiteout mechanism itself must be
identified there.

1. **Static (G3-008, before build):** the trainer exists with exactly 3 party
   members; the scene map is within limits; the script compiles with Poryscript.
2. **Spec `director-trainer-party`** (`boot_to` the scene map, cold, ROM-resident
   data). It asserts the party count and each species of the new trainer's party
   through its data symbol.
   - Counterfactuals: `shift`, `wrong_value` (species off by one), `base_rom` (the
     trainer is absent from the base ROM).
3. **Spec `director-loss-no-whiteout`**. It starts from a savestate just before the
   trainer talks, with policy `lose-fast` for the battle. It asserts:
   - the outcome equals the "lost" constant;
   - after settling, the player is still on the scene map, not at a recovery point,
     via the save-block location with `deref`;
   - the scene's completion flag is set;
   - money is unchanged, if the whiteout mechanism normally deducts it.

   Counterfactuals: `shift`, `wrong_value`, and `base_rom` with a control. On the
   base ROM, an ordinary lost battle must produce a whiteout, so the "still on the
   scene map" assertion fails there.
4. **Preview:** the same savestate is opened in the app's emulator panel for the
   owner to watch (MCP-004 acceptance).

## Out of scope here

- Implementation: MCP-003, MCP-004.
- Static limits: G3-007, G3-008.
- The expansion's own C test runner (`make check`) is a second, independent
  instrument for battle logic, wrapped by G3-005. It is headless and battle-only:
  no overworld, input, save or rendering.
