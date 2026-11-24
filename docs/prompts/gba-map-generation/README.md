# GBA map and tileset generation — operating guide

Four English prompts, run in order in the same new chat with an image-generating model. The texts are a consolidated editorial version of prompts defined in conversation: constraints, layouts and JSON schemas are preserved, but they are not a word-for-word transcript.

Image size: in the ChatGPT chat app the output has a fixed area of about 1.57 megapixels and only the aspect ratio can be requested; exact pixel sizes are an API-only parameter. A 3:2 landscape image is 1536 × 1024, which is why the map is 24 × 16 metatiles at 4× (20 × 12 usable inside the 2-metatile perimeter) and every tileset sheet is 24 × 16 metatiles. Downloads may arrive as WebP rather than PNG; the pipeline must accept them and check for compression artefacts.

## 1. Map
Open a new chat with image generation available. Open `01_map_generation.md` and replace `[INSERT SCENE DESCRIPTION HERE]` with the free-form scene description (notes or a voice transcript are fine; English keeps the prompt uniform). Paste the whole content, or attach the completed file and ask the model to run all of it. Generate one map only. Save it as `map.png`. If you regenerate, state explicitly which version is final.

## 2. Inventory
In the same chat, paste `02_asset_inventory.md` in full (or attach it and ask the model to run it). Ask for the analysis of the final map, without generating images. Save the returned JSON as `inventory.json`. Check uncertainties, duplicates, dimensions, collisions, behaviors and overflow before going on. The inventory is a visual estimate, not a measured extraction.

## 3. Sheet 1
Open `03_tileset_sheet_generation.md`. Replace the TARGET_SHEET placeholder with `1`. Send the full prompt in the same chat, stating that it must use the final map and the approved inventory. Save the sheet as `sheet_01.png`.

## 4. Manifest 1
Open `04_tileset_manifest.md`, replace TARGET_SHEET_ID with `sheet_1`, then send it in the same chat to analyse the sheet just generated. Save the JSON as `manifest_01.json`.

## 5. Sheet 2
Reuse `03_tileset_sheet_generation.md` with TARGET_SHEET `2`. Repeat that the graphical source is the original final map, not sheet 1. Save `sheet_02.png`.

## 6. Manifest 2
Reuse `04_tileset_manifest.md` with TARGET_SHEET_ID `sheet_2`. Save `manifest_02.json`.

## 7. Overflow and verification
If more sheets are needed, reuse prompt 03 naming the base sheet, the overflow identifier and the IDs of the assets still missing (for example `sheet_1_overflow_01`, layout 1). Produce a manifest for every additional image. Do not duplicate assets already generated. If the model mixes up references, attach the final map, the inventory and the specific sheet again as needed.

Then hand images, inventory and manifests to the deterministic pipeline. It checks the actual resolution, 4×4 blocks, magenta RGB, grid, palettes, hashes, repeatability, asset correspondence and the tile budget (1024 tiles in total: primary 512 tiles and 6 palettes, secondary 512 tiles and 7 palettes). PNG format and the requested size are not guaranteed by the prompt alone. On error, fix or regenerate only the non-conforming product and update its manifest.

## Expected outputs
```
map.png
inventory.json
tilesets/
  sheet_01.png
  manifest_01.json
  sheet_02.png
  manifest_02.json
```

Each overflow adds one image + manifest pair. There is no need to ask the model to produce every output in a single message.
