# GBA TILESET — POST-GENERATION MANIFEST AND VALIDATION

## 1. INPUT REFERENCES

SOURCE_MAP: The map layout (prompt 01).
ASSET_INVENTORY: The approved JSON asset inventory and the batch plan (prompt 03).
TARGET_SHEET_IMAGE: The most recently generated tileset sheet image.
TARGET_SHEET_ID: [INSERT SHEET IDENTIFIER, e.g. batch_01]

## 2. OBJECTIVE

Analyze the actual generated tileset image. Produce a machine-readable manifest describing the assets actually present. Compare the sheet against the approved inventory and technical specification. This is image inspection and validation. Do not generate another image. Do not report intended positions as observed positions. Do not assume the image follows the generation instructions correctly.

## 3. COORDINATE SYSTEM

Expected image size: 2560 × 1408 pixels from the automated pipeline, 1536 × 1024 from a chat app.
Expected metatile width and height: 64 image pixels.
Expected grid: 40 columns × 22 rows (24 × 16 from a chat app).
Coordinate origin: top-left.
Coordinate indexing: zero-based.

An asset's starting position refers to the top-left corner of its assigned graphical bounding box, including transparent pixels within its intended footprint. Do not confuse the visible silhouette bounding box with the assigned metatile bounding box.

## 4. ASSET IDENTIFICATION

Inspect the generated sheet and identify every distinct asset. Report corresponding inventory ID, name, sheet ID, section, starting column and row, width and height in metatiles, source occurrence count, layer, palette family, foreground priority, collision and behavior (copied from the inventory), repeatability and axis, source fidelity, grid alignment, and confidence in identification and coordinates.

Preserve inventory IDs. Do not create new IDs for recognized inventory assets. Record unexpected assets separately. Record missing assets of this batch and duplicate occurrences. If a position cannot be reliably determined, use null and explain uncertainty.

## 5. VALIDATION REQUIREMENTS

Inspect for missing, unexpected, or duplicate assets; incorrect dimensions, section placement, graphical scale, or projection; cropped or overlapping assets; missing separator cells or occupied separator rows; incorrect background; visible grid or labels; incorrect modular decomposition; inconsistent styles or palettes; apparent transparency edge artifacts; and apparent source fidelity problems.

Distinguish visually assessable properties from properties requiring deterministic pixel-level validation. Do not claim exact pixel equivalence, color counts, palette compatibility, dimensions, or grid alignment unless actually measured. Visual inspection alone is not sufficient proof of pixel-level correctness.

## 6. OUTPUT FORMAT

Return valid JSON only. Do not wrap in Markdown code fences. Use the following structure:

{
  "sheet_id": "sheet_1",
  "expected_resolution_px": {"width": 2560, "height": 1408},
  "expected_grid_metatiles": {"columns": 40, "rows": 22},
  "assets": [
    {
      "id": "inventory_asset_id",
      "name": "asset_name",
      "section": 1,
      "column": 0,
      "row": 0,
      "width_metatiles": 1,
      "height_metatiles": 1,
      "source_map_occurrences": 1,
      "layer": "ground",
      "palette_family": "material_name",
      "foreground_priority": false,
      "collision": "walkable",
      "behavior": "none",
      "repeatable": false,
      "repeat_axis": "none",
      "source_fidelity": "approximate_match",
      "grid_alignment": "visually_aligned",
      "confidence": "high",
      "notes": ""
    }
  ],
  "missing_assets": [],
  "unexpected_assets": [],
  "duplicate_assets": [],
  "overflow_assets": [],
  "validation": {
    "visual_inspection": {
      "all_expected_assets_present": null,
      "no_unexpected_assets": null,
      "no_duplicate_assets": null,
      "correct_section_layout": null,
      "separator_rows_empty": null,
      "assets_not_cropped": null,
      "assets_not_overlapping": null,
      "consistent_projection": null,
      "consistent_style": null,
      "background_appears_magenta": null,
      "no_visible_grid": null,
      "no_text": null
    },
    "deterministic_checks_required": [
      "exact_image_dimensions",
      "exact_background_rgb",
      "native_pixel_block_consistency",
      "metatile_grid_alignment",
      "unique_rgb_color_count",
      "hardware_tile_palette_compatibility",
      "pixel_exact_asset_comparison",
      "seamless_module_validation",
      "unique_hardware_tile_count"
    ],
    "issues": [],
    "uncertainties": [],
    "overall_status": "requires_deterministic_validation"
  }
}

Allowed collision values: walkable, blocked, partial.
Allowed behavior values: door, stairs, ladder, bridge, none.
Allowed source_fidelity values: approximate_match, differs, unknown.
Allowed grid_alignment values: visually_aligned, visually_misaligned, unknown.
Allowed confidence values: high, medium, low.
Use true, false, or null for visual inspection results. Return the complete manifest without additional prose.
