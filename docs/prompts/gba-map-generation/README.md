# GBA map and tileset generation — operating guide

Four English prompts, run in order in the same new chat with an image-generating model. The texts are a consolidated editorial version of prompts defined in conversation: constraints, layouts and JSON schemas are preserved, but they are not a word-for-word transcript.

## Why the map and the tiles are drawn separately

A Gen 3 map is a grid of metatile IDs; the screen shows about 15 × 10 metatiles and the engine scrolls. Vanilla Rustboro and Slateport are 40 × 60, Lilycove 80 × 40, Route 119 40 × 140. pokeemerald-expansion accepts any map with (width + 15) × (height + 14) ≤ 10240 metatiles. The graphics budget (1024 tiles in total: primary 512 tiles and 6 palettes, secondary 512 tiles and 7 palettes) belongs to the tileset, not to the map, so a large map costs no more tiles than a small one: it repeats the same assets.

In the ChatGPT chat app the output has a fixed area of about 1.57 megapixels and only the aspect ratio can be requested. The Image API accepts any size with both sides divisible by 16 and an aspect between 1:3 and 3:1 (up to 3840 × 2160; above 2560 × 1440 is experimental), a transparent background and a `complexity` setting; the automated pipeline (`scripts/mapgen.py`, the app's Map generator view) uses them. Model choice there: `gpt-image-2.6-flare` (fast) for layouts, `gpt-image-2.6-sunburst` (editing precision) for sheets and corrections. One image cannot hold a full-size location at a detailed scale, so the work is split:

- **Map layout (01):** the whole location at plan scale. Through the API the image is sized exactly to the map: one metatile is one cell of 16 to 48 pixels (the largest that fits 2560 × 1440 with sides divisible by 16) plus a 2-cell magenta border, so the layout can be read cell by cell; maps up to 156 × 86 metatiles within the engine limit. In a chat app the size is fixed (cell 16, 1536 × 1024). It decides what exists, what it looks like and where it goes.
- **Tileset sheets (03):** sheets at 4× game scale that redraw every distinct asset of the layout in detail, with the footprint fixed by the inventory (02): 2560 × 1408 (40 × 22 metatiles) on a transparent background through the API, 1536 × 1024 (24 × 16) on magenta in a chat app. The inventory is drawn in batches of at most 16 assets of one category (12 in chat), one image per batch: asked to pack a whole inventory section on a fixed grid, the image model drew less than half of it.

The full map is then rebuilt by the pipeline at layout size from the metatiles extracted from the sheets. An intermediate detailed "style sample" scene was tried and dropped: every redraw by the image model drifts from the design, and the sheets already provide the detailed graphics.

Downloads may arrive as WebP rather than PNG; the pipeline must accept them and check for compression artefacts.

## Style reference

Attach the style reference image to prompt 01, and optionally to prompt 03. It shows only the camera angle (three-quarter top-down) and, loosely, the pixel-art look; it does not define size, scale, content or layout, and the prompts say so explicitly.

The reference is not stored in the repository, because it is derived from third-party tilesets that must stay local. Build it from a modern GBA-compatible tileset in the local asset catalog: a 320 × 192 native-pixel crop enlarged 4× with nearest-neighbor scaling and centred on a 1536 × 1024 solid #FF00FF canvas.

In chat, the image model only receives the short prompt that the assistant writes. That is why prompt 01 contains a verbatim image prompt and keeps the full specification for the assistant's own checks.

## 1. Map layout
Open a new chat with image generation available. Open `01_map_layout.md` and replace `[INSERT SCENE DESCRIPTION HERE]` with the free-form scene description (notes or a voice transcript are fine; English keeps the prompt uniform). Attach the style reference together with the completed file and ask the model to execute it in full. Save the image as `layout.png` and keep the map size and asset list the model writes after it.

## 2. Inventory
In the same chat, paste `02_asset_inventory.md` in full (or attach it and ask the model to run it). Ask for the analysis of the layout, without generating images. Save the returned JSON as `inventory.json`. Check uncertainties, duplicates, dimensions, collisions, behaviors and overflow before going on. The inventory is a visual estimate, not a measured extraction.

## 3. Sheets
Send `03_tileset_sheet_generation.md` once in the same chat, with the map layout attached (and optionally the style reference). The model writes the batch plan and draws batch 1. Then write `next` for each following batch, or `redo N` to regenerate one. Save the plan as `batches.md` and each image as `sheet_NN.png`. If the model loses track of the layout, attach it again with the `next` message.

## 4. Manifests
After each sheet, send `04_tileset_manifest.md` with TARGET_SHEET_ID set to `batch_NN` and save the JSON as `manifest_NN.json`.

## 5. Verification
If the model mixes up references, attach the layout and the inventory again. Do not regenerate assets already accepted.

Then hand images, inventory and manifests to the deterministic pipeline. It checks the actual resolution, 4×4 blocks, magenta RGB, grid, palettes, hashes, repeatability, asset correspondence and the tile budget. PNG format and the requested size are not guaranteed by the prompt alone. On error, fix or regenerate only the non-conforming product and update its manifest.

## Expected outputs
```
layout.png
inventory.json
batches.md
tilesets/
  sheet_01.png
  manifest_01.json
  sheet_02.png
  manifest_02.json
  ...
```

Each batch adds one image + manifest pair. There is no need to ask the model to produce every output in a single message.
