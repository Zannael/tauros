# All-in-one Pokémon ROM hack editor: what to reuse

2026-11-01 · Simone

## Summary

For Gen 3, the engine is almost entirely already there: decomps, script and tileset compilers, a scriptable emulator and headless tests are mature and can be integrated as "connectors". What is missing, and what makes the project original, is the layer on top: a single app with a common semantic model, agents over MCP, an integrated emulator and adapters for multiple decomps.

- **Integrate right away:** pokeemerald-expansion, Poryscript, Porytiles, mGBA with Lua, the expansion's test runner. Porymap's file format is the standard to stick to.
- **Study before writing code:** poryrom and porygon. They already do "describe the hack and the agent builds it" on pokeemerald, but without a visual editor or an integrated emulator. They are the closest reference, and also the most direct competitors.
- **The genuinely new work:** the Tauri app, the semantic model, the "domain" MCP and the agent → build → test → preview loop.

## Layered architecture

Users and agents only work on layers 1-3. Adding a game means writing a new connector at layer 4 or 5, without touching the rest.

```
The user only sees the app; underneath, connectors are swappable

┌──────────────────────────────────────────────────────────────────────┐
│ 1 · Tauri app (the user)                                             │
│     Chat and voice · map editor · tilesets and pixel art ·           │
│     mGBA preview in the same window                                  │
└──────────────────────────────────────────────────────────────────────┘
                                   ▼
┌──────────────────────────────────────────────────────────────────────┐
│ 2 · Agents and domain MCP                                            │
│     Domain tools, not free-form file edits:                          │
│     create_trainer, place_npc, add_scene, build, run_test            │
└──────────────────────────────────────────────────────────────────────┘
                                   ▼
┌──────────────────────────────────────────────────────────────────────┐
│ 3 · Common semantic model                                            │
│     Trainer, party, map, event, scene, species: same for every game  │
└──────────────────────────────────────────────────────────────────────┘
                                   ▼
┌──────────────────────────────────────────────────────────────────────┐
│ 4 · Decomp connectors                     (dashed = later)           │
│  ┌──────────────────────┐ ┌ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ┐ ┌ ─ ─ ─ ─ ─ ─ ─ ─ ─ ┐│
│  │ Gen 3 · now          │   Gen 1-2 · later        DS · last           │
│  │ pokeemerald-expansion│ │ pokered, pokecrystal │ │ pokeplatinum,    ││
│  │ Poryscript, Porytiles│   .asm macros,           pokeheartgold       │
│  └──────────────────────┘ │ Polished Map         │ │ 3D maps, NARC    ││
│                           └ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ┘ └ ─ ─ ─ ─ ─ ─ ─ ─ ─ ┘│
└──────────────────────────────────────────────────────────────────────┘
                                   ▼  compiled ROM
┌──────────────────────────────────────────────────────────────────────┐
│ 5 · Emulator connectors                                              │
│  ┌────────────────────────────────┐ ┌ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ┐  │
│  │ mGBA (GB, GBC, GBA)            │   melonDS (DS)                    │
│  │ libmgba via mgba-rs, Lua,      │ │ same MCP interface toward the │  │
│  │ savestates, RAM                │   agents                          │
│  └────────────────────────────────┘ └ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ┘  │
└──────────────────────────────────────────────────────────────────────┘
```

Almost all the mature tools live inside the connectors: expansion, Poryscript and Porytiles inside the Gen 3 connector, libmgba inside the emulator connector. The layer-2 MCP tools only talk to the common model: that is what makes them reusable when Gen 1-2 and DS arrive.

## Decisions made

The project starts for personal use, with the goal of possibly releasing it as open source in the future. The core is in Rust and Tauri, chat and microphone live inside the app, and the work proceeds layer by layer up to a complete beta. The target is not an MVP.

| Topic | Decision | Why |
| --- | --- | --- |
| User | Initially only Simone, a demanding user. Possible open-source release in the future | No installer or multi-OS support at first, but code and licenses already ready to be published |
| Core | Rust (crate workspace) + Tauri | libmgba integrates natively (mgba-rs), file I/O and builds are fast, a single binary is shipped. The same judgment holds after considering Python too |
| MCP server | Rust with [rmcp](https://docs.rs/crate/rmcp), the official MCP SDK | Domain tools live in the same process as the model and the connectors |
| Agent | Chat and microphone in the app via the Agent SDK, in a small TypeScript sidecar (the SDK only exists for Python and TypeScript). The MCP stays open to external Claude Code and Codex as well | According to [this guide](https://dev.to/hassann/how-to-use-the-claude-agent-sdk-with-your-claude-plan-3olb), since mid-June 2026 the SDK can use the Claude subscription's monthly credit, without an API key. Fine for personal use; for a public release each user will need their own credentials. To be verified against the official documentation |
| Swappable agent | The "agent bridge" interface is a layer of its own: Agent SDK adapter now, possibly Codex or API later | Same logic as the connectors: don't get locked into one vendor |
| Voice | Local transcription (e.g. whisper.cpp) → text → agent | Model-independent, and free |
| poryrom and porygon | Clone and analyze them first, then decide whether to reuse them | Decision deferred to phase 0, with a written report |
| Method | Build layer by layer, bottom up. Each layer is "complete" when the layer above can use it, then get quickly to a beta that covers all features | Using the beta is the best way to find bugs and learn which features are really needed |

## Layered roadmap toward the beta

Each phase ends with something usable, not with a demo. The beta coincides with the end of phase 5; phase 6 can overlap.

1. **Phase 0 — Foundations.** Rust workspace with one crate per layer (`model`, `connector-gen3`, `emu`, `mcp`, `agent-bridge`, `app`). Toolchain setup script (devkitARM, Poryscript, Porytiles). Analysis report on poryrom and porygon.
2. **Phase 1 — Model and Gen 3 connector.** Lossless reading and writing of a pokeemerald-expansion project: trainers, parties, maps and events in Porymap format, scripts via Poryscript, flags. Incremental build. Automatic git commit before every change.
    - Done when: I open a project, change a trainer from Rust code, build, and the ROM works.
3. **Phase 2 — Emulator.** libmgba via mgba-rs: framebuffer, input, audio, savestates, RAM reading, booting straight into a map or a scene (warps and flags).
    - Done when: a minimal Tauri window shows the ROM and starts from the chosen point.
4. **Phase 3 — MCP server.** Domain tools, emulator tools and RAM-based checks.
    - Done when: Claude Code on the PC, connected to the MCP, builds the director's scene end to end (trainer with 3 Pokémon, lost battle without whiteout, verification, preview). From here on the engine is already usable every day.
5. **Phase 4 — Agent bridge and voice.** Sidecar with the Agent SDK connected to the internal MCP, streaming chat, microphone with local transcription.
6. **Phase 5 — Full app = beta.** Map editor and viewer, trainer and scene editor, emulator panel, chat, change history with undo.
7. **Phase 6 — Graphics.** Asset pipeline (Porytiles, Porypal, palette and VRAM validator), pixel panel, Aseprite MCP, catalog of graphical upgrades.

## Rules for the agents writing the code

- **Never destructive:** every operation on a game project goes through a git commit; no write without a snapshot.
- **Targeted tests, not exhaustive ones:** tests are mandatory only for the file ↔ model roundtrip (that is where a bug corrupts data) and for each connector's contract. One integration test per phase. No full suites for every function.
- **A non-blocking bug is acceptable**, a bug that corrupts data is not.
- **Compatible formats:** projects stay openable in Porymap and buildable with `make`.
- **No Nintendo assets in the repo:** the decomp is cloned onto the user's machine.
- **Licenses:** respect libmgba's MPL-2.0; if code from poryrom (MIT) or porygon (Apache 2.0) is reused, credit it.
- **End of phase:** every phase closes by updating the README with what can be done and what is missing.

## Mature tools to integrate as they are

These are projects the community has used for years and that are actively maintained. The Gen 3 connector calls them as libraries or CLIs, without rewriting them.

| Tool | What it does | Status | How we use it |
| --- | --- | --- | --- |
| [pokeemerald-expansion](https://github.com/rh-hideout/pokeemerald-expansion/releases) | Modern Emerald base: updated battle engine, Pokémon and items up to the latest gens, debug menu | Release 1.16.x, frequent updates | Default base for every new project (the "empty ROM" the director starts from) |
| [pret/pokeemerald](https://github.com/pret/pokeemerald) | Vanilla Emerald decomp | Stable, the reference | Alternative for those who want compatibility with the official games |
| [Porymap](https://huderlem.github.io/porymap/) | Map, event, warp and encounter editor for Gen 3 | 6.3.1 (April 2026); has a scripting API | Not embeddable (it's a Qt app), but its file format is the standard: projects stay openable in Porymap too |
| [Poryscript](https://github.com/huderlem/poryscript) | High-level scripting language that compiles to the decomp's scripts | Mature, used by major hacks | Language in which the agents write scenes: more readable and less fragile than `.inc` |
| [Porytiles](https://code.heni.lol/decomp/porytiles) | Compiles layered RGBA PNGs into Porymap-ready tilesets, and decompiles them | Mature; does not yet decompile animations | Bridge between generated or retouched graphics and the game's format |
| [mGBA](https://mgba.io/) with Lua | Reference emulator, Lua scripting, memory reading | Stable (0.10+) | Integrated preview and agent testing |
| Expansion test runner | C tests with a dedicated language, run headless on multiple mGBA instances in parallel | Mature, part of the expansion repository | Basis for automated scene tests ([details](https://deepwiki.com/rh-hideout/pokeemerald-expansion/9-testing-and-debugging)) |
| [Polished Map](https://flathub.org/apps/io.github.Rangi42.polished-map) / Tilemap Studio | Map and tilemap editors for pokered and pokecrystal | Maintained by Rangi42 | Format reference for the future Gen 1-2 connector |

A note on Porymap: it's written in C++/Qt, so it can't go inside a Tauri webview. The sensible move is to reimplement reading and writing of its format (map JSON, layouts, metatiles) in our editor, not the app itself.

## Young projects to take ideas from

Almost all of them were born in 2026, have very few stars and no maintenance guarantees. They should not be used as dependencies: they should be read to understand the design choices and the mistakes already made.

| Project | What it does | Why it matters to us |
| --- | --- | --- |
| [poryrom](https://github.com/scotbearss/poryrom) | Skill plugin for Claude Code, Codex and Gemini CLI: "describe the hack", the agent writes the code on pokeemerald-expansion, builds in Docker and verifies in mGBA by reading memory | It's your idea without a UI. To study: how it verifies, which constraints it knows (map sizes, sprite counts), the wiki generated at every build. MIT license |
| [porygon](https://awesomeclaudeplugins.com/GregTheGreek/porygon) | "AI-first, agent-agnostic decomp toolkit" for pokeemerald, with an MCP server (e.g. `add_sign`, `add_trigger`, `compile_poryscript`) | Concrete example of a "domain" MCP: the agent writes the logic, the tool handles the wiring and validation. Apache 2.0 |
| [mgba-live-mcp](https://pypi.org/project/mgba-live-mcp/) | MCP that keeps an mGBA session alive: input, Lua, memory reading, OAM dump, screenshots | Basis or model for our emulator MCP |
| [mcp-mgba headless](https://glama.ai/mcp/servers/Canadadry/mcp-mgba) | Headless GBA debugger on libmgba: stepping, breakpoints, memory | Useful for agents to understand a crash or a softlock |
| [mgba-rs](https://github.com/ocnc/mgba-rs) | Rust bindings for libmgba: framebuffer, audio, input | The most direct way to get mGBA inside a Rust Tauri backend. Version 0.1, a single commit: probably needs forking |
| MCP servers for Aseprite ([diivi/aseprite-mcp](https://mcpservers.org/servers/diivi/aseprite-mcp), [pixel-mcp](https://pkg.go.dev/github.com/willibrandon/pixel-mcp)) | Let the AI draw in Aseprite: pixels, layers, frames, palettes, export | Workbench for human-in-the-loop graphics: the agent sketches, you retouch in the same file |
| [PokeMCP](https://github.com/adamescj/PokeMCP) and the [Claude Plays Pokémon writeup](https://latent.space/reading/claude-plays-pokemon/) | Agents that play FireRed or Red through an emulator | They confirm that screenshot-only navigation is the weak spot. For testing, use RAM, savestates and warps rather than having the agent "play" |

The main lesson from poryrom and porygon: chat-only versions already exist. The added value lies in the integrated app (map, tileset, emulator and chat in the same window) and in multi-decomp support.

## Graphics assets and visual enhancements

"Fourth-generation-looking" graphics on GBA have already been demonstrated by the community. The editor should offer them as a catalog of upgrades to enable and customize, not as work to redo every time.

**What already exists**

- **DS-style sprites:** the expansion includes updated DS-style sprites and a sprite and animation viewer ([feature list](https://github.com/Jaizu/good-emerald)).
- **80x80 sprites:** in a dedicated thread the community confirms that 80x80 sprites in battle and in the Pokédex [are feasible](https://www.pokecommunity.com/threads/pokeemerald-expansion-question-increasing-sprite-dimensions-and-palette-bit-depth.541933/). That's the size of fourth-gen sprites, so they can be reused without resizing.
- **Animated battle sprites:** there's a tutorial for continuous Black-and-White-style animations on pokeemerald ([thread](https://www.pokecommunity.com/threads/animated-battle-sprites-in-the-style-of-black-and-white-in-pokeemerald.491184/)). The cost is space: every Pokémon has twice the frames, and this is where the 32 MB comes in.
- **Shared tilesets:** [Team Aqua's Asset Repo](https://github-wiki-see.page/m/TeamAquasHideout/Team-Aquas-Asset-Repo/wiki) and its [Great Tileset Exchange](https://github-wiki-see.page/m/TeamAquasHideout/Team-Aquas-Asset-Repo/wiki/The-Great-Tileset-Exchange) collect community-made, decomp-ready tilesets, with the requirement to credit the authors.
- **Sprite packs:** the [LibertyTwins](https://www.pokecommunity.com/threads/rom-hacking-sprites-pack-battle-backgrounds-overworlds-trainer-sprites-and-more-updated-2026.527581/) pack (battle backgrounds, overworlds, trainers) is aimed at binary FireRed, but many sprites work with the decomp too.
- **Weather and particles:** Emerald's weather system (`field_weather_effect.c`) already handles rain, snow and ash. The autumn route with falling leaves is a new weather effect built on the same pattern. Trees swaying in the wind are animated tiles, a format that Porytiles cannot yet decompile.

**Proposed pipeline for graphical upgrades**

1. **Source:** an existing asset (decomp, asset repo, fourth-gen sprites) or an agent's sketch in Aseprite.
2. **Conversion:** reduction to 16 colors per palette and to 8x8 tiles (Porypal for sprites, Porytiles for tilesets).
3. **Validator:** checks dimensions, colors, duplicate tiles and VRAM budget, so errors show up before the build.
4. **Human retouching:** in the editor's pixel panel or in Aseprite.
5. **Preview:** build and mGBA in the same window.

"Smoothing" fourth-gen assets is therefore a palette-reduction problem, not a resolution problem. It's the step where an agent plus a validator pays off the most.

## Beyond Gen 3: Gen 1-2 and DS

The most sensible second connector is Gen 1-2: it has mature tools and it's 2D. DS should be designed into the interfaces from now on, but built last.

| Family | Decomp | Existing editors | Emulator | Connector difficulty |
| --- | --- | --- | --- | --- |
| Gen 1-2 (GB/GBC) | pokered, pokecrystal, pokegold: assembly disassemblies | [Polished Map](https://flathub.org/apps/io.github.Rangi42.polished-map), Tilemap Studio | mGBA (also supports GB/GBC) | Medium: data in `.asm` macros, same 2D-map idea |
| Gen 3 (GBA) | pokeemerald, pokefirered, pokeruby + expansion: C | Porymap, Poryscript, Porytiles | mGBA | Low: it's the first connector |
| Gen 4 (DS) | [pokeplatinum](https://www.github.com/pret/pokeplatinum) (still a work in progress), pokeheartgold | [DSPRE](https://github.com/DS-Pokemon-Rom-Editor/DSPRE) (Windows only, works on binary ROMs), Pokémon DS Map Studio | melonDS or DeSmuME | High: 3D maps, NARC archives, a different emulator |

A positive sign for DS: pokeplatinum already converts a lot of data from JSON to binary during the build. For example, there's one JSON file per trainer with its party ([overview](https://docsearch.algolia.com/mcp/docs/repo/pret/pokeplatinum)). This kind of data maps well onto the common semantic model. 3D maps don't: they will require a separate editor.

## Risks and community notes

The most concrete risk isn't technical: part of the ROM hacking community does not accept AI-generated content.

- **PokéCommunity** bans any AI-generated content in fan games published on the forum, even as placeholders ([notice](https://www.pokecommunity.com/threads/pokeemerald-expansion-question-increasing-sprite-dimensions-and-palette-bit-depth.541933/)). Hacks made with the editor may not be publishable there if the AI generates art or text.
- **pret** does not accept AI-generated code in its repositories. The editor must work on users' local forks and never contribute upstream automatically.
- **Positioning:** present the AI as director and technical assistant that writes scripts, wiring and tests, and keep the human retouching of graphics clearly visible. A label recording what was generated and what was done by hand may help.
- **Legal:** the editor must not contain the decomp or any Nintendo assets. It must clone the repos onto the user's computer, as Porymap and poryrom do. The same goes for sprites extracted from fourth-gen games.
- **Licenses:** libmgba is MPL-2.0. If it's statically linked into the Tauri binary, its source must be made available, as the [mgba-rs](https://github.com/ocnc/mgba-rs) README also points out. Poryrom is MIT and porygon is Apache 2.0: ideas and code can be taken from them with credit.
- **Young dependencies:** mgba-rs and the mGBA MCPs are at version 0.x and maintained by individuals. Better to put them behind an interface of our own, so they can be replaced.

## Next steps

- [ ] Open the repo and give Claude Code, as its first prompt, the "Decisions made", "Roadmap" and "Rules for the agents" sections.
- [ ] Phase 0: workspace, toolchain setup, report on poryrom and porygon.
- [ ] Verify against the official documentation the usage rules for the Agent SDK with a subscription.

## Sources

- [pokeemerald-expansion, releases](https://github.com/rh-hideout/pokeemerald-expansion/releases)
- [pret/pokeemerald, Useful Modding Tools](https://github.com/pret/pokeemerald/wiki/Useful-Modding-Tools)
- [Porymap, documentation and changelog](https://huderlem.github.io/porymap/)
- [Poryscript](https://github.com/huderlem/poryscript)
- [Porytiles](https://code.heni.lol/decomp/porytiles)
- [Expansion testing and debugging](https://deepwiki.com/rh-hideout/pokeemerald-expansion/9-testing-and-debugging)
- [poryrom](https://github.com/scotbearss/poryrom)
- [porygon](https://awesomeclaudeplugins.com/GregTheGreek/porygon)
- [mgba-live-mcp](https://pypi.org/project/mgba-live-mcp/)
- [mcp-mgba headless](https://glama.ai/mcp/servers/Canadadry/mcp-mgba)
- [mgba-rs](https://github.com/ocnc/mgba-rs)
- [diivi/aseprite-mcp](https://mcpservers.org/servers/diivi/aseprite-mcp)
- [Claude Plays Pokémon, writeup](https://latent.space/reading/claude-plays-pokemon/)
- [Team Aqua's Asset Repo](https://github-wiki-see.page/m/TeamAquasHideout/Team-Aquas-Asset-Repo/wiki)
- [BW-style animated sprites on pokeemerald](https://www.pokecommunity.com/threads/animated-battle-sprites-in-the-style-of-black-and-white-in-pokeemerald.491184/)
- [80x80 sprites in the expansion and PokéCommunity's AI rule](https://www.pokecommunity.com/threads/pokeemerald-expansion-question-increasing-sprite-dimensions-and-palette-bit-depth.541933/)
- [pret/pokeplatinum](https://www.github.com/pret/pokeplatinum)
- [DSPRE](https://github.com/DS-Pokemon-Rom-Editor/DSPRE)
- [Polished Map](https://flathub.org/apps/io.github.Rangi42.polished-map)
- [pokeheartgold fork (pret's stance on AI code)](https://github.com/ctrl-cheeb-del/pokeheartgold/wiki)
