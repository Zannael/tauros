# GBA TILESET — SOURCE MAP ASSET EXTRACTION

## 1. INPUT REFERENCES

SOURCE_MAP: The previously generated overworld map in this conversation.
ASSET_INVENTORY: The previously established JSON asset inventory in this conversation.
TARGET_SHEET: [INSERT SHEET NUMBER: 1, 2, OR AN OVERFLOW SHEET]

## 2. OBJECTIVE

Generate a production-oriented graphical tileset sheet containing the reusable assets identified in the source map and approved inventory. This is graphical extraction and reconstruction, NOT creative asset generation. Treat the source map as the exclusive graphical reference and the inventory as the exclusive content specification. The result must resemble a raw graphical tileset exported from a GBA RPG development tool. Do not generate another assembled map.

## 3. IMMUTABLE TECHNICAL CONFIGURATION

Target platform: Game Boy Advance.
Hardware tile size: 8 × 8 native pixels.
Logical metatile size: 16 × 16 native pixels.
Rendering scale: 4× nearest-neighbor.
Output hardware tile size: 32 × 32 image pixels.
Output metatile size: 64 × 64 image pixels.
Sheet width: 24 metatiles.
Sheet height: 16 metatiles.
Native sheet resolution: 384 × 256 pixels.
Final sheet resolution: exactly 1536 × 1024 pixels.
Reserved transparency color: pure #FF00FF.
Maximum visible colors per palette bank: 15.
Maximum palette banks: 13.
Maximum global visible colors: 150.
Hardware tile budget: 1024 in total (primary tileset: 512 tiles, 6 palette banks; secondary tileset: 512 tiles, 7 palette banks).
Output format: lossless PNG.
Coordinate origin: top-left.
Coordinate indexing: zero-based.

## 4. SOURCE MAP FIDELITY

STRICT REQUIREMENTS:
- Copy the graphical appearance of each asset from the source map. Preserve original pixel scale, graphical dimensions, proportions, silhouettes, color values, outline thickness, shading, lighting direction, orientation, orthographic projection, and material identity.
- The tileset and source map must appear to originate from the same graphical asset library. An asset placed back into the source map must visually match its original occurrence.
- Do not redraw assets in a different style, enhance them, or add graphical detail. Do not simplify distinctive visible features unless required to resolve a technical conflict.
- For partially occluded assets, reconstruct only the minimum missing information needed to create a reusable asset. Match the existing graphical design. Never invent unrelated details.

## 5. CONTENT COMPLETENESS AND DEDUPLICATION

Include every inventory asset assigned to the requested sheet. Include each distinct drawable asset exactly once across the entire collection. Do not duplicate repeated map instances or include separate flipped copies when hardware flipping suffices. Do not add color variants absent from the source map, alternative designs, unused decorations, bonus assets, characters, NPC sprites, or complete assembled structures when modules suffice.

The inventory determines what must be included. The source map determines how it must look. Neither artistic preference nor available space justifies extra assets.

## 6. GRID AND PIXEL ALIGNMENT

STRICT REQUIREMENTS:
- Every native pixel occupies exactly one solid 4 × 4 output-pixel block, aligned with the global image origin.
- Every asset occupies an integer number of 16 × 16 native-pixel metatiles. Its bounding box begins and ends at metatile boundaries. Every metatile occupies exactly 64 × 64 output pixels.
- Preserve inventory footprints. Do not resize assets, use fractional pixels or metatiles, interpolate, smooth, or draw the grid.
- Internal artwork may use native pixel coordinates within its footprint. Fill unused bounding-box areas with reserved magenta.

## 7. CAMERA AND PROJECTION

Use exactly the same fixed, orthographic, elevated top-down three-quarter RPG projection as the source map, matching Generation III GBA overworld conventions.

Forbidden: front elevation views, straight-on architectural facades, side-scrolling projection, isometric projection, perspective convergence, vanishing points, camera rotation, alternative viewpoints, and asset turnarounds.

Buildings must show the same substantial visible roof surfaces and downward viewing angle as their source counterparts. Never convert a top-down building into a front-facing architectural sprite. Do not change the camera angle merely because the asset is isolated.

## 8. PIXEL ART RENDERING

Use authentic native-resolution pixel art, flat discrete colors, clean pixel clusters, consistent one-native-pixel dark outlines where present in the source, and two or three shading tones per basic material color.

Forbidden: anti-aliasing, gradients, bloom, glow, volumetric lighting, soft shadows, ambient occlusion, depth of field, motion blur, film grain, painterly textures, randomized surface noise, dense dithering, and high-resolution illustrations with pixelation filters. Artwork must appear created directly on the native grid.

## 9. PALETTE MANAGEMENT

Preserve the source map palette. Group colors into stable material families actually present in the source, such as wood, stone, vegetation, snow, fabric, and metal. Each family should use at most 15 visible colors when practical. Every 8 × 8 native hardware tile must fit within one palette bank containing at most 15 visible colors plus reserved transparency. Reuse exact RGB values for identical materials. Do not introduce subtle variations, independent recoloring, or intermediate edge colors. Reserved magenta must never be visible artwork. Preserve one global lighting direction.

## 10. BACKGROUND AND ASSET SEPARATION

The entire sheet background must be solid pure magenta RGB 255, 0, 255 (#FF00FF). All empty areas use exactly this color. No white, black, gray, checkerboard, semi-transparent pixels, halos, gradients, or ground textures around isolated assets.

Every independently placeable asset or grouped modular block must be separated from neighboring groups by at least one complete empty metatile. Adjacent modules within the same predefined autotile or linear group may occupy neighboring cells without separators. Designated separator rows must remain entirely magenta. No overlapping bounding boxes, cropped assets, or decorative borders.

## 11. SHEET 1 — SURFACES, LINEAR ELEMENTS AND VEGETATION

Use only for TARGET_SHEET 1. The sheet contains 24 columns and 16 rows; all coordinates are zero-based metatile units.

### SECTION 1 — SURFACES, FIRST BAND

Rows 0–2. Up to three material groups, each 6 columns × 3 rows. Starting columns: 0, 7, 14. Columns between groups remain magenta.

Exact relative layout within each group:

| Row | Column 0 | Column 1 | Column 2 | Column 3 | Column 4 | Column 5 |
|---|---|---|---|---|---|---|
| 0 | NW outer corner | North edge | NE outer corner | Empty | NW inner corner if required | NE inner corner if required |
| 1 | West edge | Seamless center | East edge | Empty | SW inner corner if required | SE inner corner if required |
| 2 | SW outer corner | South edge | SE outer corner | Empty | Empty | Empty |

Unused transitions remain magenta.

### EMPTY SEPARATOR
Row 3 entirely magenta.

### SECTION 1 — SURFACES, SECOND BAND
Rows 4–6, same layout as first band. Additional material groups only as required. Unused groups remain magenta.

### EMPTY SEPARATOR
Row 7 entirely magenta.

### SECTION 2 — LINEAR ELEMENTS
Rows 8–10. Place required linear modules in compact groups. Horizontal structures: start, repeatable middle, end in horizontal sequence. Vertical structures: top, repeatable middle, bottom in vertical sequence. Staircases preserve source width; arrange top, middle and bottom vertically. Include corners and junctions only as required. Maintain at least one empty magenta metatile between independent groups. Do not rotate horizontal assets into vertical variants.

### EMPTY SEPARATOR
Row 11 entirely magenta.

### SECTION 3 — VEGETATION
Rows 12–15. Preserve original footprints. For each family, order required variants: foreground, middle-row, background. Separate independent variants by one empty magenta metatile. Variants of the same family have identical dimensions. Include only required variants. Assign assets that cannot fit to overflow. Never resize or crop.

## 12. SHEET 2 — BUILDINGS, MODULAR STRUCTURES AND SMALL OBJECTS

Use only for TARGET_SHEET 2. Same 24 × 16 grid.

### SECTION 4 — BUILDINGS
Rows 0–5. Place every assigned distinct building once, left to right, preserving graphical dimensions and footprint. Maintain at least one empty magenta metatile between buildings. Entrances on the lowest appropriate visible row consistent with the source. No ground beneath buildings, adjacent fences, plants, decorations, paths, furniture, or shadows cast onto background. Fully visible roof overhangs.

### EMPTY SEPARATOR
Row 6 entirely magenta.

### SECTION 5 — MODULAR STRUCTURES
Rows 7–11. Place reusable towers, walls, gates, pillars, trunk segments, branches, and junctions only if present in the approved inventory. No assembled castles, large trees, or other decomposable structures. Preserve exact footprints. Repeatable segments must connect seamlessly. Separate independent modules or groups by one empty magenta metatile.

### EMPTY SEPARATOR
Row 12 entirely magenta.

### SECTION 6 — SMALL OBJECTS
Rows 13–15. Place objects left to right in descending source occurrence count. Use inventory dimensions, preferably 1 × 1 or 1 × 2 when consistent with source. Center within bounding boxes. No surrounding terrain. One empty magenta metatile between independent objects. Do not invent animation frames. If explicitly approved, arrange up to four frames horizontally within one group, with identical dimensions and alignment.

## 13. OVERFLOW SHEETS

If a section cannot accommodate all required assets, use additional sheets with the same 24 × 16 dimensions, 1536 × 1024 resolution, scale, magenta background, category-specific layout, footprints, and separators. An overflow sheet inherits the corresponding base layout. Do not mix unrelated categories in unused sections or duplicate assets from earlier sheets. Never enlarge the canvas, change scale, resize or crop assets, or remove separators. If an individual asset exceeds section height, use a dedicated overflow sheet with an explicitly declared layout exception. If it exceeds the entire sheet, decompose it into reusable modules instead of resizing.

## 14. SURFACE-SPECIFIC REQUIREMENTS

Interior tiles must be completely opaque and repeat seamlessly in all four directions. Transition tiles preserve original appearance; outside the boundary use pure magenta, not neighboring materials. The compositor places neighboring terrain on the appropriate graphical plane. Do not include large repeated patches; only necessary interior, edge, outer-corner, and inner-corner modules.

## 15. LINEAR ELEMENT REQUIREMENTS

Decompose every linear structure into reusable modules. Repeatable middle occupies exactly one metatile along the repetition axis; perpendicular dimension matches original width. Middle connects seamlessly with itself; start and end connect to middle. Preserve post, railing, step, and detail spacing. No complete long fences, walls, bridges, or stairs baked into unique assets.

## 16. VEGETATION REQUIREMENTS

Preserve original silhouettes, colors, outlines, dimensions. Foreground variants include complete silhouettes. Middle-row variants reproduce standardized foliage overlap. Background variants use magenta in transparent regions. No unrelated terrain, fences, buildings, or decorations baked in. Maintain consistent alignment across variants. No randomized leaves or branches.

## 17. BUILDING AND STRUCTURAL REQUIREMENTS

Isolate architecture from surroundings. No ground beneath, adjacent objects, or cast shadows on terrain. Preserve roof depth and elevated top-down projection. Foreground-priority elements remain visually distinguishable. Large modules connect correctly on the metatile grid, without seams or inconsistent outlines.

## 18. SMALL OBJECT REQUIREMENTS

Each object completely isolated; unused bounding-box area magenta. Do not combine unrelated objects or add environmental shadows outside objects. Preserve original scale and appearance. No additional orientations or variants unless required by inventory.

## 19. INTERNAL GENERATION PROCEDURE

Before rendering: inspect source map and approved inventory; select only assets assigned to TARGET_SHEET; determine bounding boxes; plan metatile positions; check section boundaries, empty rows, separators, and overlaps; verify no resizing; assign overflow as necessary; reproduce original appearance. Only then render. Do not display packing plan inside image.

## 20. FINAL VALIDATION

Verify 24 × 16 metatiles, 1536 × 1024 output pixels, consistent 4 × 4 blocks, correct elevated orthographic projection, pure magenta background, no visible grid, cropping or overlaps, correct separators and category placement, original dimensions and appearance, no duplicates or invented assets or unnecessary color variations, seamless modules, correct decomposition, and no forbidden rendering effects. Use overflow rather than violating technical specifications.

## 21. OUTPUT REQUIREMENTS

Return exactly one image representing the requested sheet. Return only that sheet, as lossless PNG at specified resolution. No text, titles, labels, legends, signatures, watermarks, borders, grid lines, rulers, editor interfaces, asset identifiers, presentation mockups, assembled source map, or multiple sheets combined in one image. The result must resemble a raw production-oriented Game Boy Advance graphical tileset.
