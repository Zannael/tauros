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

Tracked as tasks, not here: DEC-001 (project licence), DEC-002 (libmgba consumption and
linking), DEC-003 (AI provenance label). The voice engine is chosen within AGT-004.
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
