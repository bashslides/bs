# Test overview

A list of the test cases in this repository, grouped by area. The suite has
98 tests: 74 integration tests under `tests/` and 24 inline unit tests in
`src/`.

Integration tests follow one pattern: author a presentation in the JSON source
format, run it through `Engine::compile` + `Renderer::render`, and assert on the
reconstructed character grid (some also assert on cell styles).

## Integration tests (`tests/`)

### Coordinates & deserialization — `tests/units.rs`

| Test | Verifies |
|------|----------|
| `fixed_coordinate_floors_to_a_cell` | A `Fixed` coordinate floors to a whole cell |
| `animated_coordinate_interpolates_linearly` | An `Animated` coordinate interpolates linearly across its window |
| `animated_coordinate_supports_a_decreasing_ramp` | An `Animated` coordinate ramps downward when `from` > `to` |
| `animated_coordinate_clamps_outside_its_window` | An `Animated` coordinate clamps to its endpoints outside the window |
| `frame_range_end_is_exclusive` | `FrameRange` end is exclusive |
| `coordinate_field_accepts_bare_number_or_object` | A coordinate field accepts a bare number or an object form |
| `omitted_optional_width_defaults_to_zero` | An omitted optional width defaults to zero |

### Compile → render → diff pipeline — `tests/pipeline.rs`

| Test | Verifies |
|------|----------|
| `label_renders_text_at_its_position` | A label renders its text at its position |
| `first_frame_is_full_and_later_frames_are_diffs` | The first frame is full; later frames are diffs |
| `animated_position_moves_the_glyph_and_clears_the_old_cell` | An animated position moves the glyph and clears the old cell |
| `higher_z_order_paints_over_lower` | A higher z-order paints over a lower one |
| `frames_range_end_is_exclusive` | A frame range's end is exclusive |
| `off_grid_object_is_clipped_not_panicked` | An off-grid object is clipped, not panicked |

### Engine compile — `tests/engine.rs`

| Test | Verifies |
|------|----------|
| `compile_produces_one_scene_per_frame` | `Engine::compile` yields one scene per frame |
| `empty_presentation_renders_blank_frames` | A presentation with no objects renders blank frames |
| `object_with_frame_range_outside_the_deck_is_never_drawn` | An object whose frame range never intersects the deck is never drawn |

### Renderer & frame replay — `tests/renderer.rs`

| Test | Verifies |
|------|----------|
| `equal_z_order_keeps_source_order` | Ops at equal z-order keep source order (later wins) |
| `grid_at_clamps_a_frame_index_past_the_end` | `grid_at` clamps a frame index past the last frame |
| `grid_at_skips_out_of_bounds_diff_changes` | `grid_at` skips out-of-bounds diff changes instead of panicking |

### Label object — `tests/label.rs`

(Plain placement is also covered in `tests/pipeline.rs`.)

| Test | Verifies |
|------|----------|
| `framed_label_draws_a_border_one_cell_outside_the_text` | `framed` draws a border one cell outside the text |
| `frame_style_colours_the_border_independently_of_the_text` | `frame_style` colours the border, not the text |
| `background_fills_the_box_and_pads_to_height` | A background fills the box and pads to the height |
| `height_clips_extra_lines` | An explicit height clips extra lines |
| `width_wraps_text_across_multiple_rows` | A width wraps text across multiple rows |

### List object — `tests/list.rs`

| Test | Verifies |
|------|----------|
| `unordered_list_uses_default_one_blank_line_between_items` | An unordered list uses the default one blank line between items |
| `spacing_zero_packs_items_on_consecutive_rows` | `spacing` 0 packs items on consecutive rows |
| `ordered_list_numbers_each_item` | An ordered list numbers each item |
| `custom_bullet_is_used_for_unordered_items` | A custom bullet is used for unordered items |
| `wrapped_continuation_rows_align_under_the_item_text` | Wrapped continuation rows align under the item text |
| `trailing_blank_line_does_not_render_a_dangling_bullet` | A trailing blank line does not render a dangling bullet |
| `ordered_multi_digit_markers_align_continuation_rows` | Multi-digit markers (`10.`) align continuation rows under the text |
| `explicit_height_clips_extra_items` | An explicit height clips extra items |
| `background_fills_the_wrap_width` | A background fills the wrap width |

### Table object — `tests/table.rs`

| Test | Verifies |
|------|----------|
| `layout_splits_two_even_columns_and_reserves_border_columns` | Layout splits two even columns and reserves border columns |
| `layout_gives_rounding_remainder_to_the_last_column` | Layout gives the rounding remainder to the last column |
| `layout_without_borders_uses_the_full_width` | Borderless layout uses the full width |
| `normalize_cells_fills_a_full_rows_by_cols_grid` | `normalize_cells` fills a full rows×cols grid |
| `add_column_rescales_fractions_and_widens_every_row` | Adding a column rescales fractions and widens every row |
| `remove_column_rescales_remaining_fractions` | Removing a column rescales the remaining fractions |
| `remove_column_is_a_noop_on_a_single_column_table` | Removing a column is a no-op on a single-column table |
| `bordered_table_draws_a_box_with_centered_content` | A bordered table draws a box with centered content |
| `borderless_table_renders_only_content` | A borderless table renders only content |
| `header_bold_makes_only_the_first_row_bold` | Header-bold makes only the first row bold |
| `explicit_height_pads_a_short_table` | An explicit height pads a short table |
| `explicit_height_never_clips_taller_content` | An explicit height never clips taller content |
| `natural_height_is_content_plus_border_rows` | Natural height is content plus border rows |
| `natural_height_grows_with_wrapped_content` | Natural height grows with wrapped content |
| `col_pixel_range_includes_bounding_borders_when_bordered` | `col_pixel_range` includes bounding borders when bordered |
| `col_pixel_range_is_content_only_without_borders` | `col_pixel_range` is content-only without borders |

### Art object — `tests/art.rs`

| Test | Verifies |
|------|----------|
| `art_renders_each_line_at_its_offset` | Art renders each line at its offset |
| `art_is_placed_at_the_object_position` | Art is placed at the object position |
| `art_spaces_are_transparent` | Spaces in art are transparent |

### Arrow object — `tests/arrow.rs`

| Test | Verifies |
|------|----------|
| `horizontal_arrow_uses_body_then_auto_right_head` | A horizontal arrow uses body chars then an auto right head |
| `vertical_arrow_uses_body_then_auto_down_head` | A vertical arrow uses body chars then an auto down head |
| `leftward_arrow_with_custom_body_points_left` | A leftward arrow with a custom body points left |
| `diagonal_h_first_routes_along_y1_then_bends_down` | A diagonal arrow (|dx| ≥ |dy|) routes horizontally then bends down |
| `head_disabled_draws_body_at_the_endpoint` | `head: false` draws a body char at the endpoint |
| `zero_length_arrow_draws_a_single_point` | A zero-length arrow draws a single point |

### HLine object — `tests/hline.rs`

| Test | Verifies |
|------|----------|
| `default_hline_spans_x_start_to_x_end_exclusive` | An hline spans `x_start`..`x_end` (end exclusive) |
| `custom_draw_character_is_used` | A custom draw character is used |

### Header object — `tests/header.rs`

| Test | Verifies |
|------|----------|
| `glyph_is_filled_with_the_default_block_character` | Glyphs are filled with the default block character |
| `custom_fill_character_is_used` | A custom fill character is used |
| `glyphs_are_spaced_one_column_apart` | Glyphs are spaced one column apart |

### Rect object — `tests/rect.rs`

| Test | Verifies |
|------|----------|
| `border_draws_corners_edges_and_leaves_interior_blank` | The border draws corners/edges and leaves the interior blank |
| `title_is_drawn_on_the_top_edge` | A title is drawn on the top edge |

### Group object — `tests/group.rs`

| Test | Verifies |
|------|----------|
| `group_members_render_independently_and_the_group_adds_nothing` | Members render independently; the group emits no cells |
| `auto_group_does_not_gate_its_members` | An auto group (no `frames`) lets members render on their own ranges |
| `explicit_group_range_narrows_member_frames` | An explicit group range overrides (narrows) a member's range |
| `explicit_group_range_widens_member_frames` | An explicit group range overrides (widens) a member's range |

### Command object — `tests/command.rs`

| Test | Verifies |
|------|----------|
| `command_compiles_to_region_spec` | A command compiles to a `CommandRegion` spec |
| `command_draws_a_clean_placeholder_box_into_the_static_frame` | A command draws a clean placeholder box into the static frame |
| `box_height_follows_the_height_field` | Box height follows the `height` field |
| `border_can_be_disabled_for_a_frameless_region` | The border can be disabled for a frameless region |
| `command_output_renders_clipped_into_region` | Command output renders clipped into the region |

## Inline unit tests (`src/`)

### Property editing — `src/editor/properties.rs`

| Test | Verifies |
|------|----------|
| `label_properties_roundtrip` | `Label` properties round-trip through get/set |
| `hline_properties_roundtrip` | `HLine` properties round-trip through get/set |
| `rect_properties_roundtrip` | `Rect` properties round-trip through get/set |
| `header_properties_roundtrip` | `Header` properties round-trip through get/set |
| `arrow_properties_roundtrip` | `Arrow` properties round-trip through get/set |
| `art_properties_roundtrip` | `Art` properties round-trip through get/set |
| `table_properties_roundtrip` | `Table` properties round-trip through get/set |
| `group_properties_roundtrip_and_bounds` | `Group` properties round-trip and bounds compute; explicit range shows values + override note |
| `auto_group_shows_blank_frames_and_no_note` | An auto group shows blank first/last frame and no override note |
| `command_properties_roundtrip` | `Command` properties round-trip through get/set |
| `list_properties_roundtrip` | `List` properties round-trip through get/set |
| `unknown_property_is_rejected` | An unknown property name is rejected |
| `coordinate_get_set_roundtrips` | Coordinate get/set round-trips |
| `resize_group_scales_members_with_fractional_precision` | `resize_group` scales members with fractional precision |

### Word-wrap — `src/engine/objects/wrap.rs`

| Test | Verifies |
|------|----------|
| `a_word_longer_than_the_width_is_hard_broken` | A word longer than the width is hard-broken |
| `continuation_indent_is_clamped_below_the_width` | The continuation indent is clamped below the width |

### Word-wrap (indexed) — `src/engine/objects/table.rs`

| Test | Verifies |
|------|----------|
| `indexed_wrap_matches_plain_wrap_and_maps_indices` | Indexed wrap matches plain wrap and maps source indices |
| `indexed_wrap_counts_newlines_in_source_offsets` | Indexed wrap counts newlines in source offsets |
| `caret_blank_pos_opens_a_new_line_after_trailing_newline` | Caret-blank position opens a new line after a trailing newline |

### Text buffer — `src/editor/textedit.rs`

| Test | Verifies |
|------|----------|
| `insert_and_motion` | Insert and cursor motion |
| `multiline_line_col_and_vertical_motion` | Multiline line/col tracking and vertical motion |
| `newline_inserts_rather_than_commits` | Newline inserts rather than commits |

### Object defaults — `src/editor/object_defaults.rs`

| Test | Verifies |
|------|----------|
| `create_default_covers_every_object_type` | `create_default` builds the expected variant for every `OBJECT_TYPES` index |

### Art library — `src/art_library.rs`

| Test | Verifies |
|------|----------|
| `builtins_are_present_and_named` | Built-in art pieces are present and named |

## Not covered (intentional)

These run only in the interactive TUI / at play time and are verified manually.

| Area | Reason |
|------|--------|
| `Command` run-loop | Spawn, piped I/O, timeout, ✓/✗ status — runs at play time in the TUI |
| Editor | Mode FSM transitions, immediate-edit-on-add, panel rendering — interactive TUI |
