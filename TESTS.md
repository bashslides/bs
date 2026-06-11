# Test overview

A list of the test cases in this repository, grouped by area. The suite has
59 tests: 42 integration tests under `tests/` and 17 inline unit tests in
`src/`.

Integration tests follow one pattern: author a presentation in the JSON source
format, run it through `Engine::compile` + `Renderer::render`, and assert on the
reconstructed character grid.

## Integration tests (`tests/`)

### Coordinates & deserialization — `tests/units.rs`

| Test | Verifies |
|------|----------|
| `fixed_coordinate_floors_to_a_cell` | A `Fixed` coordinate floors to a whole cell |
| `animated_coordinate_interpolates_linearly` | An `Animated` coordinate interpolates linearly across its window |
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

### List object — `tests/list.rs`

| Test | Verifies |
|------|----------|
| `unordered_list_uses_default_one_blank_line_between_items` | An unordered list uses the default one blank line between items |
| `spacing_zero_packs_items_on_consecutive_rows` | `spacing` 0 packs items on consecutive rows |
| `ordered_list_numbers_each_item` | An ordered list numbers each item |
| `custom_bullet_is_used_for_unordered_items` | A custom bullet is used for unordered items |
| `wrapped_continuation_rows_align_under_the_item_text` | Wrapped continuation rows align under the item text |
| `trailing_blank_line_does_not_render_a_dangling_bullet` | A trailing blank line does not render a dangling bullet |

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
| `group_properties_roundtrip_and_bounds` | `Group` properties round-trip and bounds compute |
| `unknown_property_is_rejected` | An unknown property name is rejected |
| `coordinate_get_set_roundtrips` | Coordinate get/set round-trips |

### Word-wrap — `src/engine/objects/table.rs`

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

### Art library — `src/art_library.rs`

| Test | Verifies |
|------|----------|
| `builtins_are_present_and_named` | Built-in art pieces are present and named |
