# GBA RPG MAP LAYOUT (FULL-SIZE PLAN)

Attach the style reference image to the same message as this prompt.

The style reference shows only the camera angle (three-quarter top-down RPG view) and, loosely, the kind of clean pixel-art look we want. It does NOT define the size of the map, the scale of this image, the number of objects, the content or the layout. Do not copy its buildings, its composition or its pixel size.

## 1. HOW TO EXECUTE THIS PROMPT

You are the assistant that drives the image generator. The image generator only sees the text you send it, so:

1. Read the scene description in section 2 and design the full map (section 3).
2. Copy the image prompt in section 4 word for word, replacing only the placeholders: `{SIZE}` (map size in metatiles), `{LAYOUT}`, and the image geometry `{IMAGE_SIZE}`, `{CELL}` and `{BORDER}`. The automated pipeline (`scripts/mapgen.py`) requests an image of exactly (width + 4) × cell by (height + 4) × cell pixels, with the largest cell from 48 down to 16 pixels that fits 2560 × 1440 (or 1440 × 2560) with both sides divisible by 16, and a border of 2 cells. In a chat app, where the size cannot be set, use cell 16, the app's image size and a border of at least 32 pixels. Do not shorten, paraphrase or add mood, magic, light or atmosphere words.
3. Generate the image, using the attached reference only for camera angle and general look.
4. Check the result against section 5. If an item fails, regenerate once with the same prompt followed by one line naming the failed items. Return the best attempt.
5. After the image, write the map size you chose (width × height in metatiles) and a list of every distinct asset design you used (each building design, tree design, ground material, bridge, staircase, fence, decoration), one line each. This list is the starting point of the asset inventory (prompt 02).

## 2. SCENE DESCRIPTION — FREE-FORM INPUT

[INSERT SCENE DESCRIPTION HERE]

The description may be informal notes, a voice transcript or a detailed brief. Treat it as a level design brief for a full game location, not as an illustration brief.

## 3. MAP DESIGN (WRITTEN BY YOU)

This image is the whole map at plan scale: one metatile (one character step) is one cell of 16 to 48 image pixels, aligned to the image grid, so the layout can later be read cell by cell. It is the full-size layout of a real location, like a city or route in a Generation III game; the detailed art is drawn later, asset by asset, on the tileset sheets at 4× game scale.

- Choose the map size freely to suit the scene, like real game locations: a town is typically 30–50 metatiles per side, a large city 40 × 60 or 80 × 40, a route can be long and narrow. The map can be up to 156 metatiles on its long side and 86 on its short side, at most three times longer than wide, within the engine limit in section 6. Do not make it small to be safe; use the space the scene needs.
- Organize the location freely: districts, main roads, plazas, landmarks, elevation, water, forest borders, bridges, stairs. Place every building and feature where it makes sense for gameplay and for the story of the place.
- Keep a finite asset library: about 3–6 building designs (plus up to 2 landmark structures built from modules), up to 4 tree or plant designs, up to 4 ground materials, up to 8 decoration types. A large map is made by repeating these designs, not by inventing new ones.
- Write the layout as at most 150 words of concrete English: map size, districts by cardinal direction (north = top), roads, landmarks, borders, connections and exits. Name objects, not moods.
- Every bridge and staircase connects two walkable areas. Every door has a walkable tile in front of it. Main roads are at least 2 metatiles wide. The map edge is closed by natural borders (trees, cliffs, water) except at the exits.

## 4. IMAGE PROMPT (SEND VERBATIM)

```
Create the complete overworld map of a location for a Game Boy Advance era monster-collecting RPG, seen like a full map export from a tile-based map editor. Use the attached image only as a reference for the camera angle (three-quarter top-down: roofs seen from above, building fronts visible, no perspective) and loosely for the clean pixel-art look. Do not copy its size, scale, content or layout.

Grid: the image is {IMAGE_SIZE} pixels. One walking step is exactly one {CELL}×{CELL} pixel square, aligned to the image's pixel grid. The map is exactly {SIZE} squares and fills the image, except for a solid pure magenta (#FF00FF) border exactly {BORDER} pixels wide on every side. A small house is about 5×5 squares, a tree about 2×2, a road at least 2 squares wide. The whole map is visible; nothing is cut by the border.

Layout: {LAYOUT}

Style: flat colors, crisp pixels, dark outlines, repeated objects drawn identically, seamless repeating ground. No painting, no gradients, no glow, no fog, no particles, no perspective, no characters, no text, no labels, no UI, no visible grid.
```

## 5. CHECKLIST (AFTER GENERATION)

- It reads as a full tile-based game map, not an illustration, poster or concept art.
- The map has the chosen size and fills the image; it is not a small island in a large empty area.
- Three-quarter top-down camera, no perspective convergence.
- Magenta outside the map on all sides; nothing is cropped.
- The layout from section 3 is present; roads, bridges and stairs connect walkable areas; doors are reachable.
- Repeated designs look identical; the number of distinct designs stays within the library in section 3.
- No characters, text or UI.

## 6. ENGINE FACTS (FOR YOUR PLANNING)

- A Gen 3 map is a grid of metatile IDs. The screen shows about 15 × 10 metatiles and the engine scrolls; map size does not cost graphics memory.
- Engine limit (pokeemerald-expansion): (width + 15) × (height + 14) ≤ 10240 metatiles; for example 100 × 70 fits, 156 × 86 does not.
- The graphics budget belongs to the tileset, not to the map: at most 1024 unique 8 × 8 tiles in total (primary 512 tiles and 6 palette banks, secondary 512 tiles and 7 palette banks), 15 colors plus transparency per bank. A bigger map simply repeats the same assets, so keep the asset library finite.
