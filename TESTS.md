# Test overview

A list of the test cases in this repository, grouped by area. The suite has
59 tests: 42 integration tests under `tests/` and 17 inline unit tests in
`src/`.

Integration tests follow one pattern: author a presentation in the JSON source
format, run it through `Engine::compile` + `Renderer::render`, and assert on the
reconstructed character grid.

## Integration tests (`tests/`)

### Coordinates & deserialization — `tests/units.rs`

- `fixed_coordinate_floors_to_a_cell`
- `animated_coordinate_interpolates_linearly`
- `animated_coordinate_clamps_outside_its_window`
- `frame_range_end_is_exclusive`
- `coordinate_field_accepts_bare_number_or_object`
- `omitted_optional_width_defaults_to_zero`

### Compile → render → diff pipeline — `tests/pipeline.rs`

- `label_renders_text_at_its_position`
- `first_frame_is_full_and_later_frames_are_diffs`
- `animated_position_moves_the_glyph_and_clears_the_old_cell`
- `higher_z_order_paints_over_lower`
- `frames_range_end_is_exclusive`
- `off_grid_object_is_clipped_not_panicked`

### Table object — `tests/table.rs`

- `layout_splits_two_even_columns_and_reserves_border_columns`
- `layout_gives_rounding_remainder_to_the_last_column`
- `layout_without_borders_uses_the_full_width`
- `normalize_cells_fills_a_full_rows_by_cols_grid`
- `add_column_rescales_fractions_and_widens_every_row`
- `remove_column_rescales_remaining_fractions`
- `remove_column_is_a_noop_on_a_single_column_table`
- `bordered_table_draws_a_box_with_centered_content`
- `borderless_table_renders_only_content`
- `header_bold_makes_only_the_first_row_bold`
- `explicit_height_pads_a_short_table`
- `explicit_height_never_clips_taller_content`
- `natural_height_is_content_plus_border_rows`
- `natural_height_grows_with_wrapped_content`
- `col_pixel_range_includes_bounding_borders_when_bordered`
- `col_pixel_range_is_content_only_without_borders`

### Art object — `tests/art.rs`

- `art_renders_each_line_at_its_offset`
- `art_is_placed_at_the_object_position`
- `art_spaces_are_transparent`

### List object — `tests/list.rs`

- `unordered_list_uses_default_one_blank_line_between_items`
- `spacing_zero_packs_items_on_consecutive_rows`
- `ordered_list_numbers_each_item`
- `custom_bullet_is_used_for_unordered_items`
- `wrapped_continuation_rows_align_under_the_item_text`
- `trailing_blank_line_does_not_render_a_dangling_bullet`

### Command object — `tests/command.rs`

- `command_compiles_to_region_spec`
- `command_draws_a_clean_placeholder_box_into_the_static_frame`
- `box_height_follows_the_height_field`
- `border_can_be_disabled_for_a_frameless_region`
- `command_output_renders_clipped_into_region`

## Inline unit tests (`src/`)

### Property editing — `src/editor/properties.rs`

- `label_properties_roundtrip`
- `hline_properties_roundtrip`
- `rect_properties_roundtrip`
- `header_properties_roundtrip`
- `arrow_properties_roundtrip`
- `art_properties_roundtrip`
- `table_properties_roundtrip`
- `group_properties_roundtrip_and_bounds`
- `unknown_property_is_rejected`
- `coordinate_get_set_roundtrips`

### Word-wrap — `src/engine/objects/table.rs`

- `indexed_wrap_matches_plain_wrap_and_maps_indices`
- `indexed_wrap_counts_newlines_in_source_offsets`
- `caret_blank_pos_opens_a_new_line_after_trailing_newline`

### Text buffer — `src/editor/textedit.rs`

- `insert_and_motion`
- `multiline_line_col_and_vertical_motion`
- `newline_inserts_rather_than_commits`

### Art library — `src/art_library.rs`

- `builtins_are_present_and_named`
