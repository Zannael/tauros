# GBA RPG TILE-BASED MAP GENERATOR

## 1. SCENE DESCRIPTION — FREE-FORM INPUT

[INSERT SCENE DESCRIPTION HERE]

The scene description may be an informal narrative, a collection of notes, a transcribed voice message, a list of ideas, or a detailed environmental description.
Interpret this input as a level design brief, not as an illustration brief. You are responsible for translating the description into a coherent, visually consistent, navigable RPG map. You may independently determine the spatial arrangement, architecture, environmental composition, path network, and placement of reusable assets.
All technical constraints defined below take absolute priority over artistic freedom and narrative details. If the scene is too complex to satisfy the technical constraints, simplify its visual representation rather than violating those constraints.

## 2. GENERATION CONFIGURATION

Target platform: Game Boy Advance.
Target visual style: Generation III handheld monster-catching RPG overworld maps, with compatible Nintendo DS-era 2D RPG conventions.
Map width: 24 metatiles.
Map height: 16 metatiles.
Hardware tile size: 8 × 8 native pixels.
Logical metatile size: 16 × 16 native pixels.
Rendering scale: 4× nearest-neighbor.
Native map resolution: 384 × 256 pixels.
Final image resolution: 1536 × 1024 pixels (landscape, aspect ratio exactly 3:2).
Minimum empty perimeter: 2 metatiles on every side, included within the map dimensions (usable environment area: 20 × 12 metatiles).
Empty perimeter color: solid #FF00FF.
Scene background color: one solid color, distinct from #FF00FF, used for every empty space inside the environment (sky, void, chasm below platforms). Choose it as part of the palette.
Maximum global visible colors: 150.
Maximum palette banks: 13.
Maximum visible colors per palette bank: 15, plus one reserved transparency entry.
Maximum unique 8 × 8 hardware tiles: 1024 in total, split into a primary tileset (at most 512 tiles, 512 metatiles, 6 palette banks) and a secondary tileset (at most 512 tiles, 512 metatiles, 7 palette banks).
Maximum unique building designs: 3.
Maximum unique vegetation designs: 4.
Maximum unique decorative object designs: 6.
Maximum unique ground materials: 4.
Maximum unique large structural systems: 2.
Character reference height: 32 native pixels.
Standard door width: 16 native pixels.
Standard small house footprint: 5 × 5 metatiles.
Characters included in final map: NO.
Output format: lossless PNG.

## 3. GENERATION OBJECTIVE

Generate a complete, standalone, production-oriented, tile-based overworld map suitable for implementation in a Game Boy Advance RPG engine. The result must resemble a map assembled in a traditional 2D tile editor using a finite library of reusable tiles and metatiles. It must NOT resemble a hand-painted environment, a pixel art illustration, a cinematic game screenshot, a concept art piece, or a procedurally textured scene. Prioritize tile reusability, grid alignment, palette efficiency, collision readability, consistent scale, and technical extractability over decorative complexity. The map must be designed as an actual playable environment.

## 4. COMPLETE MAP BOUNDARIES AND FRAMING

STRICT REQUIREMENTS:
- Show the entire map within a single image. Never crop any part of the environment.
- Every building, tree, canopy, wall, platform, bridge, staircase, fence, and decorative object must be fully visible.
- No environmental asset may touch or cross the image boundaries.
- Maintain a continuous empty perimeter of at least two complete metatiles around the entire environment.
- The empty perimeter must contain only solid #FF00FF. Do not place decorative elements, particles, shadows, vegetation, or environmental details inside it.
- The environment must terminate naturally inside the canvas. All outer terrain boundaries must be explicitly resolved using complete edge and corner tiles.
- The map must not imply that visible structures continue beyond the image. Include complete roofs, complete tree canopies, complete structural foundations, and complete silhouettes.
- If the environment does not fit, reduce the number of structures or simplify the layout. Never solve spatial limitations through cropping, zooming, shrinking individual objects, or changing the established pixel scale.

This is a complete map export, not a camera viewport.

## 5. GRID, RESOLUTION AND PIXEL SCALE

STRICT REQUIREMENTS:
- Every native game pixel must correspond to exactly one solid 4 × 4 block of output image pixels. All native pixels must have identical dimensions. The pixel grid origin must be consistent across the entire image.
- Never use fractional pixels, subpixel positioning, variable pixel sizes, or interpolated edges.
- The final image dimensions must exactly match the configured map dimensions multiplied by the metatile size and rendering scale.
- Every metatile occupies exactly 16 × 16 native pixels, equivalent to 64 × 64 output pixels.
- Every hardware tile occupies exactly 8 × 8 native pixels, equivalent to 32 × 32 output pixels.
- All structural anchors, building footprints, terrain boundaries, stairs, paths, and fence segments must align with the 16 × 16 metatile grid. Smaller modular details may align with the underlying 8 × 8 hardware tile grid. Individual pixel artwork may use native pixel coordinates inside each tile.
- Do not draw the underlying grid. Do not introduce cell outlines, tile separators, rulers, alignment guides, or visible editor overlays.

The entire image must appear as if it were rendered at the configured native resolution and subsequently enlarged using exact nearest-neighbor scaling.

## 6. CAMERA AND PROJECTION

STRICT REQUIREMENTS:
Use a fixed, orthographic, elevated top-down three-quarter RPG projection. The camera looks downward onto the environment, revealing horizontal ground surfaces, roofs, platform tops, wall caps, and the front-facing portions of vertical structures. The visual projection must match traditional Generation III Game Boy Advance RPG overworld conventions. The camera is never positioned at human eye level.

Forbidden: front elevation views, straight-on architectural views, side-scrolling platformer views, side-view environments, isometric projection, dimetric projection, oblique architectural perspective, perspective convergence, vanishing points, cinematic camera angles, wide-angle distortion, camera rotation, variable viewing angles between assets, foreshortening based on distance, and perspective-based scaling.

World-space horizontal axes remain aligned with the horizontal and vertical image axes. Vertical architectural edges remain vertical on the image. Ground tiles remain square. Buildings must visibly occupy depth on the map grid. They must show substantial roof surfaces from above, rather than appearing as flat frontal facades. Walkable surfaces must occupy clearly readable two-dimensional areas, not narrow strips beneath upright scenery. Objects located farther north must not become smaller than identical objects located farther south. Every asset must use the same fixed projection, orientation, pixel scale, and visual construction rules.

## 7. PIXEL ART RENDERING

STRICT REQUIREMENTS:
- Use authentic low-resolution pixel art, flat discrete color regions, consistent one-native-pixel dark outlines on appropriate object silhouettes, and two or three shading tones per basic material color.
- Build details using intentional native-pixel clusters. Keep silhouettes clean and readable. Preserve hard pixel boundaries.
- Forbidden: anti-aliasing, smooth gradients, subpixel rendering, bloom, glow effects, volumetric lighting, ambient occlusion, soft shadows, depth of field, motion blur, film grain, photographic textures, painterly brushwork, dense dithering, procedural surface noise, randomized texture details, and simulated high-resolution pixel art.
- Do not create a high-resolution illustration and subsequently apply a pixelation filter. Construct the environment directly using the native game-pixel grid.

## 8. COLOR PALETTE AND LIGHTING

Use a finite, globally consistent indexed-color palette.
- Respect the configured maximum global color count. Use approximately 8–12 colors per material family. Every individual 8 × 8 hardware tile must be representable using a single palette bank containing no more than 15 visible colors.
- Reuse identical RGB color values across repeated materials. Do not introduce subtle color variations between instances of the same asset or intermediate colors along edges.
- Reserve #FF00FF exclusively for the empty perimeter outside the environment. Inside the environment, empty space (sky, void, chasms) uses the scene background color, never #FF00FF.
- Use one consistent global light direction, originating from the upper-left side of the image. Lighting must be encoded directly into reusable asset graphics.
- Do not modify an asset's palette based on its position, neighboring objects, elevation, or proximity to a light source.
- Lanterns, windows, crystals, and other emissive objects may use bright flat colors internally, but must not cast colored light onto surrounding tiles.
- Any shadow must be hard-edged, grid-compatible, reusable, and independent of unique environmental interactions.

## 9. FINITE TILE LIBRARY AND EXACT REPETITION

Treat the map as an assembly of reusable graphical modules. Conceptually establish the complete asset library before constructing the map. Every repeated tile or asset must be an exact graphical duplicate of its original instance: identical native-pixel positions, pixel clusters, color values, shading, and dimensions. Visually similar redrawings are NOT acceptable substitutes for duplication.

- Use exactly one repeating interior tile for each continuous ground or surface material. Repeat the same interior tile without local variation.
- Do not generate unique patches of texture or randomized cracks, scratches, stones, grass blades, snow patches, bark patterns, or water ripples.
- Reuse the same terrain transition, building, decorative object, staircase, and fence modules everywhere.
- Horizontal and vertical tile flipping may be used only when compatible with projection, lighting direction, and asset semantics. Do not rotate existing assets to simulate different orientations.

Visual richness must come from spatial arrangement and recombination of a limited asset library, not from graphical uniqueness.

## 10. AUTOTILING AND TERRAIN TRANSITIONS

Each terrain or surface material must be constructed using a finite autotile system: one seamless interior tile, four directional edge tiles, four outer corner tiles, and four inner corner tiles only when necessary. Every occurrence of a particular edge or corner must reuse the same graphical module. Transitions must align exactly with the metatile grid. Irregular or organic edges (for example ragged bark or snow drifts) are allowed only when they are drawn into the fixed edge and corner modules and repeat identically at every occurrence. Avoid bespoke curved edges, diagonal transitions, one-off boundary noise, and hand-drawn silhouettes requiring additional unique tiles. Approximate complex terrain shapes using reusable edge and corner modules. All interior tiles must connect seamlessly with adjacent copies of themselves.

## 11. MODULAR ARCHITECTURE AND LARGE STRUCTURES

All buildings and large environmental structures must be assembled from reusable components. Small buildings should share wall sections, roof sections, roof edges and corners, doors, windows, and foundations. Differences between designs should result primarily from different combinations of existing modules.

Large structures must never be rendered as a single unique illustration. Castles, fortifications, towers, massive trees, elevated platforms, cliffs, and similar structures must be constructed from reusable segments: horizontal and vertical wall segments, towers, gate modules, pillars, trunk segments, branch segments, structural junctions, platform interiors and edges, and standardized staircase segments.

Do not exceed the configured maximum number of unique structural systems or building designs. Achieve monumental scale by repeating existing modules rather than creating a large unique asset.

## 12. VEGETATION AND DECORATIVE OBJECTS

Vegetation must be constructed from a small library of reusable silhouettes. Every instance of a particular tree, bush, foliage cluster, or plant design must be pixel-identical to every other instance of that design. Do not introduce randomized leaf placement, branches, texture, shading, or color variations. When arranging vegetation in rows, use standardized foreground, middle-row, and background configurations only when necessary. Foreground vegetation must retain its complete visible silhouette. Avoid uncontrolled horizontal overlap between adjacent vegetation assets. Decorative objects must remain visually distinct and reusable. Do not exceed the configured decorative asset budget. Decorative richness is subordinate to tile efficiency and navigational readability.

## 13. LAYER SEPARATION AND OCCLUSION

GROUND PLANE: continuous terrain, floors, paths, and other walkable surface materials.
OBJECT PLANE: buildings, walls, vegetation, fences, furniture, and decorative structures.

- The ground plane must remain conceptually continuous underneath placed objects.
- Do not bake surrounding terrain textures into reusable object graphics or fuse objects with their surrounding environment.
- Avoid decorative object-to-object overlap. Do not place foliage over fences, lanterns in front of building facades, crates against unrelated objects, or decorations across structural boundaries.
- Intentional overlap is permitted only between standardized modules belonging to the same predefined structural asset.
- Use a single reserved transparency color for empty regions around object silhouettes.
- Overhead elements, such as roof overhangs and elevated tree canopies, must have clearly defined silhouettes suitable for foreground rendering priority.
- Do not create ambiguous overlaps that prevent the underlying tile structure from being reconstructed.

## 14. LEVEL DESIGN, COLLISIONS AND NAVIGATION

- Clearly distinguish walkable and non-walkable areas. All walkable terrain must have readable boundaries.
- All required gameplay areas must belong to a connected navigable layout.
- Every functional entrance must have at least one unobstructed walkable metatile directly in front of it.
- Every bridge must connect two valid walkable surfaces. Every staircase must connect two valid elevations or walkable surfaces. No bridge or staircase may terminate in empty space.
- No essential doorway or route may be obstructed by decorations, vegetation, or structural objects. No accessible platform may be unintentionally isolated.
- Collision boundaries must align with the metatile grid. Main routes should preferably be at least two metatiles wide. Secondary routes must be at least one full metatile wide.
- Represent elevation changes using standardized visual modules. Decorative structures must not create ambiguous collision boundaries.
- The map must be logically traversable without jumping, diagonal movement, or unintended traversal through blocked tiles. Design for traditional four-directional RPG movement.

## 15. ASSET SCALE AND CHARACTER REFERENCES

Maintain consistent physical proportions throughout the environment. Use the configured character height, door width, and standard house footprint as fixed scale references. Do not change asset scale to improve composition or fit more structures into the image. Identical buildings must occupy identical grid dimensions. Characters, NPCs, creatures, and movable entities must not appear unless explicitly enabled. Use an imaginary character reference during planning. Do not bake gameplay sprites into environmental tiles.

## 16. TILE BUDGET AND COMPLEXITY CONTROL

Respect the configured maximum number of unique hardware tiles. Estimate graphical complexity before rendering. Prioritize reusable ground tiles, standardized terrain boundaries, modular structural assets, essential navigation elements, reusable vegetation, then decorative objects.

If the estimated tile budget is exceeded, remove unnecessary decorations, reduce material families and building designs, simplify vegetation silhouettes, replace bespoke structures with reusable modules, reduce graphical detail, and increase asset repetition. Never reduce native pixel size, introduce uncontrolled color variation, or violate the grid to accommodate additional detail. A simpler technically valid map is always preferable to a more detailed technically invalid map.

## 17. INTERNAL PLANNING PROCEDURE

Before rendering, internally:
1. Interpret the free-form scene description.
2. Determine required environmental features.
3. Establish the finite palette and reusable asset library.
4. Assign grid-aligned footprints to all structures.
5. Define walkable terrain and collision boundaries.
6. Design the complete connected navigation network.
7. Position large modular structures.
8. Place reusable vegetation and decorative objects.
9. Verify layer separability and occlusion behavior.
10. Verify that the complete environment fits within the required empty perimeter.
11. Estimate the unique tile and palette budgets.
12. Simplify the design wherever necessary.

Only after planning, render the final map using the established reusable graphical modules. Do not display the planning process in the image.

## 18. FINAL VALIDATION

Verify complete environment visibility, uninterrupted empty perimeter, no cropping, correct elevated orthographic top-down projection, consistent native pixel dimensions, configured image resolution, grid alignment, pixel-identical repeated assets, seamless terrain interiors, reusable edges and corners, modular large structures, color and tile budgets, connected navigation, unobstructed entrances, valid bridge and staircase endpoints, logical layer separation, no baked-in characters, and no forbidden rendering effects. If any requirement is violated, revise and simplify the map before producing the final output.

## 19. OUTPUT REQUIREMENTS

Return one complete assembled overworld map. Return only the map image. The output must be a lossless PNG at the exact configured resolution of 1536 × 1024 pixels (aspect ratio 3:2). Do not include text, titles, labels, legends, signatures, watermarks, decorative frames, visible grids, editor interfaces, asset previews, separate tileset sheets, character sprites, atmospheric particles, additional viewpoints, or presentation mockups. The final result must look like a complete export from a traditional Game Boy Advance RPG tile-based map editor.
