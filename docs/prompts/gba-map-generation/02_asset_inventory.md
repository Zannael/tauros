# GBA TILESET — ASSET INVENTORY

## 1. OBJECTIVE

Analyze the previously generated overworld map in this conversation. Identify the minimum complete set of reusable graphical assets necessary to reconstruct that map in a Game Boy Advance tile-based RPG engine. Treat the map as the exclusive visual source of truth. This is an asset extraction and decomposition task, not a creative design task. Do not generate images at this stage.

## 2. TECHNICAL CONFIGURATION

Target platform: Game Boy Advance.
Hardware tile size: 8 × 8 native pixels.
Logical metatile size: 16 × 16 native pixels.
Rendering scale: 4× nearest-neighbor.
Output metatile size: 64 × 64 image pixels.
Maximum visible colors per palette bank: 15.
Maximum palette banks: 13.
Maximum global visible colors: 150.
Reserved transparency color: #FF00FF.
Hardware tile budget: 1024 in total, split into a primary tileset (at most 512 tiles, 512 metatiles, 6 palette banks) and a secondary tileset (at most 512 tiles, 512 metatiles, 7 palette banks).
Tileset sheet dimensions: 24 × 16 metatiles.
Tileset sheet resolution: 1536 × 1024 pixels.

## 3. SOURCE OF TRUTH

Only identify assets present in the source map or strictly necessary to reconstruct a visible modular asset. Do not invent assets, materials, decorations, buildings, color variants, or environmental features. Do not redesign, improve, rescale, or reinterpret existing assets. Preserve all visual properties. If partially occluded, identify visible portions and portions requiring reconstruction. Do not present inferred graphical details as directly observed.

## 4. ASSET IDENTIFICATION AND DEDUPLICATION

Identify every distinct reusable graphical asset. Determine its unique identifier, descriptive name, category, width and height in metatiles, occurrence count, approximate source coordinates, graphical layer, palette family, modularity, repeatability, foreground priority, collision, behavior, source visibility, and applicable hardware flips.

Collision describes how the player interacts with the asset's footprint: walkable (every cell can be entered), blocked (no cell can be entered), or partial (some cells walkable; list the blocked cells in notes, relative to the asset's top-left cell). Behavior names a special interaction: door, stairs, ladder, bridge, or none.

Identical repeated assets must share one ID. Do not create separate entries for different positions or flipped instances when hardware tile flipping can reproduce them without violating projection, lighting, or visual correctness. Do not treat visually distinct assets as identical unless their differences can be reproduced using existing modules. Count every occurrence, but list each distinct asset once.

## 5. ASSET CLASSIFICATION

Assign each asset to exactly one section:
1. SURFACES: continuous ground, floor, paths, water, bark, snow-covered surfaces, platform surfaces, and necessary terrain transitions.
2. LINEAR ELEMENTS: fences, railings, stairs, bridges, walls, borders, and other repeatable directional structures.
3. VEGETATION: trees, canopies, foliage clusters, bushes, plants, and necessary occlusion variants.
4. BUILDINGS: small buildings and complete reusable architectural units.
5. MODULAR STRUCTURES: large architecture or environmental structures decomposed into reusable modules.
6. SMALL OBJECTS: decorations, furniture, lanterns, crates, barrels, signs, banners, and independently placeable small assets.

Exclude characters, NPCs, creatures, atmospheric particles, and transient visual effects.

## 6. SURFACE DECOMPOSITION

For every continuous material, identify one seamless interior tile, required north, south, west, and east edges, required northwest, northeast, southwest, and southeast outer corners, and inner corners only where necessary. Do not extract arbitrarily large rectangular patches. Use the smallest complete reusable autotile set. Record directly observed versus necessary derived transitions.

## 7. LINEAR ELEMENT DECOMPOSITION

Decompose linear structures into start, seamlessly repeatable middle, end, and directional corner or junction modules where present. Identify horizontal and vertical orientations separately when valid hardware flipping is insufficient. Preserve the original orthographic RPG projection. Do not rotate images to substitute directional assets.

## 8. BUILDING AND LARGE STRUCTURE DECOMPOSITION

Small buildings may remain complete reusable assets with original metatile dimensions. Decompose large structures into the smallest practical reusable modules: towers, walls, gates, roofs, pillars, trunks, branches, junctions, and platform supports. Identify dependencies. Do not include complete structures when constituent modules suffice. Do not count a complete modular structure and its components as separate drawable assets.

## 9. VEGETATION VARIANTS

Identify distinct vegetation designs and required row variants: foreground (complete silhouette), middle row (standardized foliage overlap), and background (transparent regions for placement behind vegetation). Preserve the same footprint and identity across variants. Include only variants present or strictly necessary to reproduce observed occlusion. Do not invent unnecessary variants.

## 10. INVENTORY VALIDATION

Verify complete coverage, no unnecessary assets, deduplication, integer metatile dimensions, reusable autotiles and linear segments, modular large structures, identified foreground priority, and explicit distinction between observed and inferred details. Estimate whether assets fit within the standard two-sheet layout. Report overflow. Never omit required assets solely to fit available space.

## 11. OUTPUT FORMAT

Return valid JSON only. Do not generate an image. Do not wrap the JSON in Markdown code fences. Use this structure:

{
  "source_map": {
    "width_metatiles": null,
    "height_metatiles": null,
    "native_pixel_scale": 4,
    "metatile_size_native_px": 16,
    "coordinate_origin": "top-left",
    "coordinate_indexing": "zero-based"
  },
  "assets": [
    {
      "id": "unique_asset_id",
      "name": "descriptive_asset_name",
      "section": 1,
      "width_metatiles": 1,
      "height_metatiles": 1,
      "occurrences": 1,
      "occurrence_count_status": "estimated",
      "source_positions": [{"column": 0, "row": 0, "flip_x": false, "flip_y": false}],
      "layer": "ground",
      "palette_family": "material_name",
      "repeatable": false,
      "repeat_axis": "none",
      "modular": false,
      "foreground_priority": false,
      "collision": "walkable",
      "behavior": "none",
      "parent_asset": null,
      "source_visibility": "fully_visible",
      "confidence": "high",
      "notes": ""
    }
  ],
  "palette_families": [],
  "excluded_elements": [],
  "estimated_sheet_count": 2,
  "overflow_assets": [],
  "uncertainties": []
}

Use null for unknown values rather than inventing precise measurements.
Allowed layer values: ground, object.
Allowed collision values: walkable, blocked, partial.
Allowed behavior values: door, stairs, ladder, bridge, none.
Allowed repeat_axis values: none, x, y, xy.
Allowed source_visibility values: fully_visible, partially_occluded, inferred.
Allowed confidence values: high, medium, low.
Allowed occurrence_count_status values: counted, estimated, unknown.
Source coordinates refer to the top-left corner of an asset's graphical bounding box, including overhead graphics. All coordinates and dimensions use native metatile units, not output image pixels. Return the complete inventory as a single valid JSON object.
