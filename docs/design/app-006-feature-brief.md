# APP-006 feature brief for UI exploration

Status: reviewed with the Owner on 2026-11-26; the accepted direction is under
"Accepted direction" below and recorded in `roadmap/TODO.md` (APP-006) and
`roadmap/docs/master.md`.

Purpose: a flat list of what the Tauros desktop app must let the user do, with no
screens, panels or layout implied. It is the input for generating candidate screen
mockups (image model), which we then judge together for UI/UX sense and
feasibility in a Tauri webview.

## Context the designs must respect

- Desktop app (Linux first), single window, Tauri webview; usable from about
  1280×720 up to large monitors. Keyboard-friendly, accessible contrast.
- One user, personal use, working on one local game project at a time
  (pokeemerald-expansion today; Gen 1-2 and DS later, so nothing Gen 3-specific
  should be baked into the global structure).
- Two equal ways to act: by hand in editors, or by asking an agent in chat. Every
  agent change must be visible, verifiable and undoable.
- The game itself is a 240×160 (3:2) pixel-art frame; it must be shown at integer
  scales without blur.

## Features

### Project and session
- Open/switch a local game project; see which project, branch and game base are
  active.
- See project health at a glance: toolchain present, last build result, pending
  validation errors.
- Language choice (English, Italian).

### Agent chat
- Converse with an agent in streaming; see its reasoning steps, tool calls and
  their results (compact, expandable), including images such as emulator
  screenshots.
- Choose the provider (Claude, Codex, Antigravity) and the model for the next
  messages; sign in/out per provider with visible pending/success/failure/cancel.
- Stop a running request; start a new chat; see what context/scope the agent is
  working on (e.g. the selected map or trainer).
- Dictate a message: choose engine (offline / cloud), clear "get ready → speak now"
  state, cancel, and always review the draft before sending.

### Change history and safety
- Chronological list of changes (by hand or by agent), each with what changed,
  who/what made it and when.
- Undo/restore to a previous point; see snapshots and recovery states.
- Provenance labels (AI-generated vs hand-made) visible and exportable.

### Build, test and play
- Build the project and see compact, actionable results (errors pointing to the
  file/object at fault).
- Run tests/verification specs and see pass/fail with evidence.
- "Build & play": boot the new build directly into a chosen map, position or scene.

### Emulator
- Play the game in the app: pause, reset, frame advance, fast forward, rewind,
  speed, scale, aspect lock, mute/volume, audio channels, video layers.
- Save states (quick slots and files), import/export .sav, screenshots.
- Key configuration, ROM info, emulator log.
- Inspection tools: memory and symbols, tiles/VRAM, palettes, sprites/OAM, with
  auto-refresh.

### Map editing (upcoming, APP-001)
- Browse maps; view a map with its layout, metatiles, events, warps, NPCs and
  collisions as toggleable layers.
- Paint metatiles, place/move/edit events; resize maps; create new maps.
- Jump from a map position to "play here" in the emulator.
- Edits stay Porymap-compatible.

### Trainers and scenes (upcoming, APP-002)
- Browse and edit trainers and their parties via forms.
- Write/edit scenes (Poryscript) and flags, with compile feedback.

### Graphics (Phase 6)
- Asset pipeline with tileset/palette/VRAM validation and clear violations.
- Pixel panel for retouching tiles and sprites (palette-aware).
- Catalog of graphical upgrades to enable, configure and revert.
- Map generator: describe a scene (text or voice), pick a style reference,
  generate a layout, inventory and tileset sheets in batches; accept, correct or
  regenerate each step; see run activity, cost/usage and errors.

### Cross-cutting
- Persistent, actionable notifications (errors stay until handled; busy states
  show progress and can be cancelled where possible).
- Empty states that tell the user what to do next.
- Predictable focus and keyboard shortcuts; a way to discover all functions
  (command palette or equivalent).

## Process for the image exploration

1. Generate several candidate screens from this brief (different global structures,
   e.g. IDE-like docked panels, workspace tabs, chat-centric, canvas-centric).
2. Review together: hierarchy, discoverability, density, what each layout
   sacrifices, and whether it is buildable in HTML/CSS within the webview.
3. Pick a direction; translate it into a concrete structure and record the accepted
   decisions in TODO.md (APP-006) and master.md before implementation.

Known limits of image models for this: unreliable text and icons, invented
controls, inconsistent details between images. Use the images to judge structure
and mood, not as pixel specifications.

## Accepted direction (Owner review, 2026-11-26)

Thirteen generated mockups were reviewed (not stored: they contain invented and
third-party art). Their overall structure and palette were liked; they are a
reference, not a specification.

- **One function, one place.** No duplicated controls across views; the command
  palette (Ctrl+K) searches the actions views declare, it does not copy them.
- **Shell:** left navigation, top project bar, bottom status bar (last build, tests,
  Ctrl+K, notifications). Our own logo from `assets/brand/`.
- **Modular header actions:** each view declares its actions (one primary, few
  secondary, the rest in an overflow menu); the header shows the active view's set.
  No per-page "quick actions" column.
- **Agent:** a collapsible panel docked on the right, available in every view and
  expandable to full width; its context follows what is open (map, trainer, script).
  Provider and model are chosen only there; Settings only signs accounts in and out.
- **Emulator:** "Play here" does not leave the current editor (docked or separate
  window); the full emulator view keeps a slimmed inspector, without breakpoints.
- **Emulator controls:** no speed/scale drop-downs. Scale defaults to the largest
  integer multiple that fits, with a magnifier for fixed 1×–4×. Speed is a
  fast-forward button with a badge (1×, 2×, 4×, unlimited) cycling on click, list
  on right-click; hold-to-fast-forward stays.
- **Density:** no page descriptions, no nested cards; three-column views collapse
  their side columns below about 1440 px and stay usable at 1280×720.
- **Project view:** Overview and Build & Test merged (health, build, tests,
  artifacts, recent activity); no hero image or "next steps".
- **Maps:** edited inside Tauros (APP-001), Porymap-compatible; Porymap is not
  duplicated, only launchable for things Tauros does not cover.
- **Trainers:** no display-name localisation or notes fields; no painted battle
  preview, replaced by "Try the battle" booting into it; full party fields (moves,
  item, ability, nature, IVs/EVs).
- **Scripts:** Poryscript `.pory` sources with a real code editor (CodeMirror),
  linked resources and find references.
- **Settings:** applied immediately (no global Save); no temperature, no GitHub
  integration; toolchain status kept.
- **Map generator:** lists every tileset batch; cost shown in dollars.
- **Theme:** light and dark, following the system by default, using the mockups'
  lavender/violet palette; WCAG AA contrast is a requirement.
- **Frontend:** move the UI to Svelte (built with Vite) as part of APP-006.

## Audit of the current app (2026-11-26)

Walkthrough of the actual app (`target/debug/tauros-app --project` on the
director-scene-mcp004 project, Petalburg City) on an isolated Xvfb display at
1600×900 and 1280×720, with screenshots of the Play view, the four emulator menus,
Game State Viewers, Key Configuration, Emulator Log, ROM Information, History,
Map generator and the agent panel. Screenshots stay outside the repository (game
graphics). Not exercised: agent turns, sign-in, dictation, builds, full
keyboard/focus traversal.

### Existing, keep (behaviour preserved, restyled into the new shell)
- Emulator functions: pause, frame advance, reset, rewind, fast forward, quick
  slots 1-9, state files, .sav import/export, PNG screenshot, key remapping,
  video layers, audio channels, volume/mute, aspect lock, pause when unfocused,
  rewind buffer, and their keyboard shortcuts.
- Game state viewers: palettes, tiles/VRAM, sprites/OAM, memory and symbols, with
  auto refresh.
- Map and point selector with Preview and Build & play (boot to point), post-build
  preview banner, "Agent held" state.
- Agent: provider and model choice, New chat, streaming with tool rows, Stop,
  ChatGPT and Google sign-in, automatic project/map context.
- Dictation: offline and cloud engines, preparation and ready cue, cancel, review
  before Send.
- History: entries with provenance labels (human / ai-generated), commit id and
  restore.
- Map generator: layout → inventory → sheets steps with accept, correct,
  regenerate, automatic checks and cost line.

### Existing, evolve
- Top bar: Play / Map generator tabs become left navigation; the truncated status
  text ("MAP_PETALBURG_CITY (15, 10) · 8x (4…") moves to the status bar.
- Emulator menubar plus transport strip: each function lives once, as view header
  actions with an overflow menu (and in Ctrl+K); the menubar goes.
- Speed: fast-forward toggle, Speed submenu, a "Fast 8x" pill and a "Fast 8x"
  overlay become one fast-forward button with a badge. The saved speed silently
  restarts the emulator at 8× on launch: start at 1×, keep the chosen multiplier.
- Scale submenu: automatic largest integer fit plus a magnifier for fixed 1×-4×.
- Checked menu items have misaligned, right-shifted labels (Fast Forward, Pause when
  unfocused, Lock aspect ratio).
- Game State Viewers is a modal that hides and blocks the game: becomes a slimmed
  inspector beside the game in the Emulator view.
- Emulator Log: flooded with DMA INFO lines, no level filter; default to warnings
  and errors with a level filter.
- ROM Information mixes ROM data with the frame counter and the boot target; frame
  counter, sound and rewind count move to the status bar.
- History opens inline below the game, reachable only by scrolling, and the
  emulator menubar scrolls away with the page: becomes its own view with
  filters (human / AI / snapshot), detail and diff; "Restore state" relabelled
  with a clear consequence.
- Map generator: the page shifts horizontally against Play (content and agent panel
  move about 20 px); the header still shows emulator status; the scene prompt is a
  one-line input; its own Settings move into the Settings view.
- Agent panel: fixed width, not collapsible; empty chat has no guidance; dictation
  controls permanently take space and use unstyled native selects with
  inconsistent outlines.
- Theme: dark only with orange accent; becomes light and dark tokens with the
  violet palette, WCAG AA.
- At 1600 px the centred layout leaves empty side margins; at 1280×720 the key
  legend is cut and History is out of view.

### Existing, remove
- "Open ROM…" button in the top bar (duplicate of File › Open ROM; the action
  remains once, in the emulator overflow and Ctrl+K).
- Duplicate menu entries for transport actions already on buttons (Pause, Frame
  Advance, Reset, Rewind, Screenshot).
- The second "Fast 8x" indicator (overlay on the game).
- The permanent key legend under the game (it belongs to Key Configuration and
  tooltips).
- History "Refresh" button (the list updates itself).

### Not existing, to do
- Shell: left navigation, project bar (project, branch, health), status bar
  (last build, tests, notifications), own logo, Ctrl+K command palette,
  view-declared header actions.
- Project view: health, build with readable log and actionable errors, tests and
  verification results, artifacts, recent activity.
- Collapsible right-docked agent panel in every view, context following the open
  item, changed files with diff / undo / play here under agent replies.
- Play here from any view without leaving it.
- Settings view: account sign-in/out, dictation defaults, emulator preferences and
  keys, toolchain status, theme, interface language.
- Light theme and interface language switch (only speech language exists today).
- Persistent notifications and guiding empty states.
- Consistent focus order and keyboard behaviour, verified contrast.
- Svelte + Vite frontend.

Planned but unimplemented functions (Owner decision 2026-11-26) are shown in the
UI, disabled, with a "Work in progress" tooltip naming the task: Maps (APP-001),
Trainers and Scripts (APP-002), Graphics pixel panel (GFX-002), provenance export
(PROV-001) and similar. Building those functions is not part of APP-006.

## Implementation notes (2026-11-26)

Where the implementation departs from the lists above, and why:

- **Sign-in stays in the agent panel.** The runtimes sign in for the selected
  provider only (the backend refuses sign-in for another provider), so sign-in
  sits next to provider choice; Settings has no account section.
- **Map generator settings stay in its view** as "Run settings": they belong to
  one map run, not to the app.
- **Preview and Build & play stay in the Emulator view** with the boot point; the
  Project view offers Build (without booting) and Run tests, backed by the new
  `project_build` and `project_test` commands.
- **Changes under agent replies** list the history entries the reply recorded and
  link to History, where restore lives; no second undo button.
- **Dictation engine and speech language** moved to Settings; the agent panel keeps
  the microphone button, cancel and status.
- The tools panel of the Emulator view starts hidden when the game would shrink
  below 2×; the header toggle overrides it.
- `attach` and the commands that lock the emulator during a boot run off the main
  thread, so the window paints while the first boot runs.
