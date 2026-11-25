# GBA TILESET — ASSET SHEET GENERATION (ONE BATCH PER IMAGE)

Attach the map layout image (prompt 01) to the same message as this prompt, every time. Optionally attach the style reference too; it shows only the camera angle and general look.

## 1. INPUT REFERENCES

SOURCE_MAP: The attached map layout (prompt 01). It is drawn at plan scale (about 16 image pixels per metatile); the sheets redraw its assets at 4× game scale. It is the only graphical source: ignore every other generated image in the conversation.
ASSET_INVENTORY: The approved JSON inventory in this conversation (prompt 02).

## 2. HOW TO EXECUTE THIS PROMPT

You are the assistant that drives the image generator. The image generator only sees the text you send it and the attached images, and it cannot place dozens of assets on a precise grid in one image. So the inventory is drawn in small batches, one image per batch.

This prompt is sent once. Then:

1. Split the inventory into batches (section 3) and write the batch plan as a numbered list: for each batch, the asset IDs, names and footprints.
2. Generate batch 1 (steps 4–7 below).
3. Afterwards, each time the user writes "next", generate the next batch of the plan with the same procedure, until the plan is complete. If the user writes "redo N", regenerate batch N.

For each batch:

4. Copy the image prompt in section 4 word for word, replacing only `{ASSET_LIST}` with one line per asset of the batch (section 3 gives the format), `{SHEET_SIZE}` and `{BACKGROUND}`. The automated pipeline uses `2560×1408` (40 × 22 metatiles) and `fully transparent` with the API's transparent-background option; in a chat app use `1536×1024` (24 × 16 metatiles) and `solid pure magenta (#FF00FF)`. Do not shorten, paraphrase or add mood, magic, light or atmosphere words.
5. Generate the image with the attached map layout as the design reference.
6. Check the result against section 5. If an item fails, regenerate once with the same prompt followed by one line naming the failed items. Return the best attempt.
7. After the image, write the batch number, which asset IDs are present (left to right, top to bottom) and which are missing. Missing assets are appended to a later batch of the plan.

## 3. BATCH RULES

- One batch draws at most 16 assets (12 in a chat app), and their footprints plus a one-metatile gap around each must fit in the sheet: 40 × 22 metatiles (24 × 16 in a chat app). Large assets (more than 6 × 6) get a batch of their own or share it with at most two small ones.
- Group by category so that one image has one kind of content: surfaces and their transitions; railings, fences and other linear modules; stairs and bridges; vegetation; buildings; structural modules (castle, giant tree); small objects.
- An autotile material (interior, edges, outer corners, inner corners) counts as one asset and is drawn as a joined block: a 3 × 3 block (outer corners, edges, interior) plus, if required, a separate 2 × 2 block of inner corners.
- A linear structure (railing, wall, bridge, staircase) counts as one asset and is drawn as start, one repeatable middle and end, joined in a row (horizontal) or a column (vertical).
- Asset list line format: `<name>: <width>×<height> tiles, <short description of design, colors and material taken from the map layout>`. Do not put IDs or any other text in the image prompt; the generator might draw them.

## 4. IMAGE PROMPT (SEND VERBATIM)

```
Create a game tileset sheet: separate assets on a {BACKGROUND} background, for a Game Boy Advance era monster-collecting RPG. Redraw each listed asset from the attached map layout with exactly the same design, colors and materials, in the same three-quarter top-down view (roofs and tops seen from above, fronts visible, no perspective), at game scale.

Scale: 16×16 pixel game tiles enlarged 4× with nearest-neighbor, so one tile is a 64×64 pixel square and every game pixel is a crisp 4×4 block. Each asset is exactly the listed number of tiles wide and tall.

Assets:
{ASSET_LIST}

Layout: landscape {SHEET_SIZE}. Place the assets left to right in rows, each aligned to the 64-pixel tile grid, with at least one empty 64-pixel tile of background between assets. Every asset is complete, isolated and drawn once. Around and inside each asset, everything that is not the asset is background: no ground, grass, snow, glow or shadow under or around objects. Ground tiles are opaque and seamless; their edge and corner tiles keep the material edge and are background outside it.

Flat colors with 2–3 shades per material, clean dark outlines, hard pixel edges. No painting, no gradients, no glow, no soft shadows, no perspective, no characters, no text, no labels, no grid lines, no frames.
```

## 5. CHECKLIST (AFTER GENERATION)

- Every listed asset is present once, complete and not cropped; nothing else is drawn.
- Same design, colors and materials as in the map layout; same three-quarter top-down view.
- Crisp 4× pixels; tile-sized footprints as listed; assets aligned to a 64-pixel grid.
- Empty background (transparent, or pure magenta in a chat app) and at least one empty tile between assets; no ground, glow or shadow baked around objects.
- No text, labels, grid lines, frames or characters.

## 6. TECHNICAL SPECIFICATION (FOR YOUR CHECKS AND THE MANIFEST)

### 6.1 Sheet format
Sheet: 40 × 22 metatiles, 2560 × 1408 pixels in the automated pipeline (24 × 16, 1536 × 1024 in a chat app), 4× nearest-neighbor. Hardware tile: 8 × 8 native pixels (32 × 32 output pixels). Metatile: 16 × 16 native pixels (64 × 64 output pixels). Coordinates are zero-based metatile units from the top-left. Background and every empty area: fully transparent (automated pipeline) or pure #FF00FF (chat app); no white, black, gray, checkerboard, halos, glows or gradients. The API returns soft alpha (faint halos, solid pixels at alpha 252-253): the pipeline keeps the raw file and makes alpha binary at 128. Assets are segmented by background separation, so the one-metatile gap matters more than an exact position.

### 6.2 Budget and palettes
Hardware tile budget: 1024 in total (primary 512 tiles, 512 metatiles, 6 palette banks; secondary 512 tiles, 512 metatiles, 7 palette banks). At most 15 visible colors plus transparency per bank, 150 in total. Group colors into stable material families present in the layout (wood, stone, vegetation, snow, fabric, metal); reuse exact RGB values for identical materials; no per-instance recoloring or intermediate edge colors. One light direction from the upper left.

### 6.3 Fidelity
Redraw each asset at 4× with the inventory footprint, preserving design, proportions, silhouette, colors, materials, outline style, shading and orientation. Add only the pixel detail the larger scale requires. Do not change style, redesign, recolor or embellish. Do not add variants, bonus assets, characters, or complete assembled structures when modules suffice. Do not include flipped copies when hardware flipping suffices.

### 6.4 Surfaces
Interior tiles are fully opaque and repeat seamlessly in all four directions. Edge and corner tiles keep the material edge; outside the boundary use the background, not a neighboring material. Only the interior, edge, outer-corner and inner-corner modules, no large patches.

### 6.5 Linear elements
The repeatable middle is exactly one metatile along the repetition axis and connects seamlessly with itself; start and end connect to it. Preserve post, railing, step and detail spacing. No long fences, walls, bridges or stairs baked into a single asset.

### 6.6 Vegetation
Original silhouettes and colors. Foreground variants have complete silhouettes; middle-row variants reproduce the standardized foliage overlap; background variants leave their transparent regions as background. No terrain or other objects baked in.

### 6.7 Buildings and structures
Isolated from surroundings: no ground beneath, adjacent objects or cast shadows on terrain. Same roof depth and top-down view as in the layout; never a front-facing facade. Large structures (castles, giant trees) are drawn as modules (towers, wall segments, gates, trunk and branch segments) that connect on the metatile grid, never as one assembled picture.

### 6.8 Small objects
Each object isolated, with its unused bounding-box area left as background. No animation frames unless the inventory asks for them.

### 6.9 Output
Return exactly one image per batch, lossless PNG if possible (the chat app may deliver WebP). No text, titles, labels, legends, watermarks, borders, grid lines, editor interfaces or asset identifiers in the image.
