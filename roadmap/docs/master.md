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
  AGENTS.md existed. No product phase has started.
- **Verified** 2026-11-18 UTC: the host has Python 3.10.12, cargo/rustc and Node.js
  22.14.0 installed. Versions of devkitARM, Poryscript, Porytiles and Tauri tooling are
  not checked (FND-002).

## Accepted decisions

Source: plan §"Decisions made", §"Layered roadmap toward the beta" and §"Rules for
the agents writing the code", dated 2026-11-01 by the owner. They bind every task;
TODO.md cites the relevant rows in each Scope. Changing one needs the owner's
agreement and an entry here.

## Open decisions

Tracked as tasks, not here: RES-001 (poryrom/porygon reuse), RES-002 (Agent SDK
subscription auth), DEC-001 (project licence), DEC-002 (libmgba consumption and
linking), DEC-003 (AI provenance label). The voice engine is chosen within AGT-004.
Record each outcome below when accepted.

## Unverified claims from the specification

These come from the plan and have not been checked by us:

- Tool versions and status: pokeemerald-expansion 1.16.x, Porymap 6.3.1 (April
  2026), mGBA 0.10+, mgba-rs 0.1 with a single commit. Pinning happens in FND-002 and
  DEC-002.
- Agent SDK usable with a Claude subscription since mid-June 2026 (source: a dev.to
  guide, not official documentation). Checked by RES-002.
- Community policies: PokéCommunity bans AI-generated content in published fan
  games; pret does not accept AI-generated code. Inputs to DEC-003.

## Contracts

- Roadmap: format and invariants are enforced by `scripts/check_roadmap.py` and
  described in the TODO's Working contract; the workflow is in `AGENTS.md`.
