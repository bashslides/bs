# Test overview

A list of the test cases in this repository, grouped by area. The suite has
469 tests: 156 integration tests under `tests/` and
313 inline unit tests in `src/`.

This file is not maintained on trust: `tests/docs.rs` fails `cargo test` when a
test exists that is not listed here, when a listed test does not exist, or when
the totals above are stale.

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
| `animated_coordinate_clamps_outside_its_window` | An `Animated` coordinate clamps to its endpoints outside the window (span resolved from an `AnimSpans` table) |
| `animated_coordinate_with_missing_animation_holds_at_from` | A dangling `anim` reference (no such animation) renders as if static at `from` |
| `start_value_ignores_animation_timing` | `Coordinate::start_value()` returns `from` for `Animated`, the floored value for `Fixed` (no table needed) |
| `frame_range_end_is_exclusive` | `FrameRange` end is exclusive |
| `coordinate_field_accepts_bare_number_or_object` | A coordinate field accepts a bare number or an object form |
| `omitted_optional_width_defaults_to_zero` | An omitted optional width defaults to zero |
| `color_accepts_named_strings_and_rgb_maps` | `Color` parses a named string and an RGB **map** (`{"r","g","b"}`), and round-trips to the same shape |
| `color_rejects_the_rgb_array_form` | The `{"rgb": [r,g,b]}` array form is rejected — pins the syntax the docs promise |

### Compile → render → diff pipeline — `tests/pipeline.rs`

| Test | Verifies |
|------|----------|
| `label_renders_text_at_its_position` | A label renders its text at its position |
| `first_frame_is_full_and_later_frames_are_diffs` | The first frame is full; later frames are diffs |
| `animated_position_moves_the_glyph_and_clears_the_old_cell` | An animated position moves the glyph and clears the old cell |
| `higher_z_order_paints_over_lower` | A higher z-order paints over a lower one |
| `frames_range_end_is_exclusive` | A frame range's end is exclusive |
| `off_grid_object_is_clipped_not_panicked` | An off-grid object is clipped, not panicked |
| `objects_near_the_u16_edge_are_clipped_not_wrapped` | Every positioned renderer at x = 65535 (and a huge framed label at y = 65535) compiles without overflow and draws nothing back onto the canvas |

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
| `control_characters_in_a_deck_print_as_spaces` | A control character in a deck cell (`\r`, `\t`) is sent to the terminal as a space (`Cell::printable`), so it cannot move the cursor and garble a repainted row |
| `wide_and_zero_width_chars_print_as_one_column` | `Cell::printable` sends an emoji or CJK char as `?` and a combining accent as a space, so every cell is one terminal column; ASCII, box drawing, `▶` and `★` pass through |
| `grid_at_normalises_a_ragged_full_frame` | `grid_at` pads/truncates a ragged `Full` frame to the contract shape, so a diff into a missing cell does not panic |

### Label object — `tests/label.rs`

(Plain placement is also covered in `tests/pipeline.rs`.)

| Test | Verifies |
|------|----------|
| `framed_label_draws_a_border_one_cell_outside_the_text` | `framed` draws a border one cell outside the text |
| `framed_label_at_the_origin_keeps_its_text_visible_inside_the_border` | A framed label at (0,0) shifts its text inside the border instead of hiding under it |
| `align_center_centres_text_within_the_width` | `align: center` centres each row within `width` |
| `align_right_pushes_text_to_the_right_edge` | `align: right` right-aligns text within `width` |
| `valign_center_offsets_text_down_within_the_height` | `valign: center` vertically centres the rows within `height` |
| `valign_bottom_places_text_on_the_last_row` | `valign: bottom` pushes the rows to the bottom of `height` |
| `frame_style_colours_the_border_independently_of_the_text` | `frame_style` colours the border, not the text |
| `background_fills_the_box_and_pads_to_height` | A background fills the box and pads to the height |
| `height_clips_extra_lines` | An explicit height clips extra lines |
| `width_wraps_text_across_multiple_rows` | A width wraps text across multiple rows |
| `label_spaces_erase_lower_objects_even_without_bg` | Without a `bg`, a label's interior and width-padding spaces still blank the object beneath |
| `huge_bg_label_fills_only_the_canvas` | A 20000×20000 bg-filled label renders quickly and fills exactly the canvas |
| `multi_digit_numbered_line_wraps_under_its_text` | A `100. ` line's continuation indent is the full marker width (5), so a long first word stays on the number's row and continues under the text |
| `nested_bullet_keeps_its_first_word_and_wraps_under_its_text` | A nested `  - ` bullet keeps its long first word on the marker row and its continuation hangs under the item text |

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
| `a_long_first_word_stays_on_the_marker_row` | A first word too long for the row after the marker (bulleted and ordered) hard-breaks on the marker row instead of stranding the bullet alone |
| `an_indented_item_keeps_its_first_word_on_the_marker_row` | A list item starting with spaces does not soft-break after the marker, stranding the bullet |
| `huge_spacing_only_builds_the_visible_rows` | A `spacing` of 1e13 with `height` 3 completes quickly and shows only the first item |

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
| `huge_row_count_renders_only_the_visible_rows` | `rows: 1e9` renders the visible rows quickly (top border, first row, a separator) |
| `cell_cursor_at_the_maximum_z_order_does_not_overflow` | The editor's cell-cursor overlay on a table at `z_order` `i32::MAX` saturates instead of overflowing |

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
| `horizontal_double_head_points_outward_at_both_ends` | `head_start: true` adds an outward head at (x1,y1): `◀` start, `▶` end |
| `vertical_double_head_points_outward_at_both_ends` | A vertical double-headed arrow points `▲` at the start, `▼` at the end |
| `diagonal_double_head_heads_the_horizontal_start` | On an H-first diagonal, the start head sits on the horizontal leg and points outward |
| `start_head_only_leaves_the_end_as_body` | `head: false` + `head_start: true` draws a head only at the start |
| `custom_head_char_applies_to_the_start_head_too` | A custom `head_ch` rotates to the outward sibling for the start head |
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
| `text_word_wraps_when_too_wide_for_the_canvas` | Header word-wraps onto the next glyph line when too wide for the canvas, breaking on word boundaries |

### Rect object — `tests/rect.rs`

| Test | Verifies |
|------|----------|
| `border_draws_corners_edges_and_leaves_interior_blank` | The border draws corners/edges and leaves the interior blank |
| `title_is_drawn_on_the_top_edge` | A title is drawn on the top edge |
| `title_on_a_rect_narrower_than_its_padding_is_skipped` | A title on a rect of width 0 or 1 at x=0 draws no cells and does not underflow |
| `title_at_the_maximum_z_order_still_draws_over_the_border` | A titled rect at `z_order` `i32::MAX` compiles without overflow and the title still shows over the border |

### Group object — `tests/group.rs`

| Test | Verifies |
|------|----------|
| `group_members_render_independently_and_the_group_adds_nothing` | Members render independently; the group emits no cells |
| `auto_group_does_not_gate_its_members` | An auto group (no `frames`) lets members render on their own ranges |
| `explicit_group_range_narrows_member_frames` | An explicit group range overrides (narrows) a member's range |
| `explicit_group_range_widens_member_frames` | An explicit group range overrides (widens) a member's range |

### Loop object — `tests/looping.rs`

| Test | Verifies |
|------|----------|
| `loop_regions_collects_specs_with_defaults` | A `loop` compiles to a `LoopRegion`; omitted fields default (500 / 0 / true) |
| `loop_regions_carries_explicit_fields` | Explicit `delay_ms` / `count` / `bounce` pass through to the region |
| `disjoint_loops_validate` | Side-by-side, non-touching loops pass `validate_loops` |
| `overlapping_loops_are_rejected` | Partially crossing ranges (10..20 vs 15..25) are rejected |
| `nested_loops_are_rejected` | A loop fully containing another is rejected (no nesting) |
| `loop_past_end_of_deck_is_rejected` | A range extending past `frame_count` is rejected |
| `empty_loop_range_is_rejected` | A zero-width range (`start == end`) is rejected |
| `a_deck_with_no_loops_validates_and_emits_nothing` | No loops → validates and emits no regions |
| `loop_region_follows_an_explicit_group_range` | A loop in an explicit-range group emits its `LoopRegion` on the group's range |
| `validate_loops_checks_the_explicit_group_range` | Overlap is checked on the group-overridden range, not the loop's own |

### Animation object — `tests/animation.rs`

| Test | Verifies |
|------|----------|
| `animation_regions_collects_specs_with_defaults` | An `animation` compiles to an `AnimationRegion`; omitted fields default (auto_play true / 500 ms) |
| `animation_regions_carries_explicit_fields` | Explicit `auto_play` / `delay_ms` pass through to the region |
| `animations_may_overlap_each_other` | Two overlapping animation spans validate (unlike loops) |
| `a_loop_containing_a_whole_animation_validates` | A loop that fully contains an animation is allowed |
| `a_loop_whose_bounds_match_the_animation_validates` | Containment is inclusive of equal bounds |
| `an_animation_fully_outside_a_loop_validates` | A disjoint loop/animation pair is allowed |
| `a_loop_cutting_an_animation_in_half_is_rejected` | A loop that partially overlaps an animation is rejected |
| `a_loop_starting_inside_an_animation_is_rejected` | A loop starting mid-animation (partial overlap) is rejected |
| `explicit_group_range_does_not_move_an_animation_member` | An `animation` in an explicit-range group keeps its own span (region + loop validation) |
| `compile_rejects_two_animations_sharing_an_id` | Two `animation` objects with the same `id` fail compilation (`validate_loops`); distinct ids compile |

### Auto-advance object — `tests/autoadvance.rs`

| Test | Verifies |
|------|----------|
| `auto_advance_regions_collects_specs_with_default_delay` | An `auto_advance` compiles to an `AutoAdvanceRegion`; omitted `delay_ms` defaults to 5000 (5 s) |
| `auto_advance_regions_carries_an_explicit_delay_and_range` | Explicit `delay_ms` and frame range pass through to the region |
| `auto_advance_draws_nothing_into_the_frames` | The marker renders nothing into the static frames (only the label is painted) |
| `auto_advance_region_follows_an_explicit_group_range` | An `auto_advance` in an explicit-range group emits its region on the group's range |

### Circle object — `tests/circle.rs`

| Test | Verifies |
|------|----------|
| `diameter_10_circle_is_round_and_filled` | A diameter-10 circle fills its 20-col central rows, has narrower round caps, vertical symmetry, an empty bounding-box corner, and a filled centre |
| `fill_char_is_customizable` | The circle is drawn with the chosen `ch` (and never the default `@`) |
| `circle_is_horizontally_symmetric` | Every filled row's left and right gaps match (centred span) |
| `circle_is_hidden_outside_its_frame_range` | The circle renders only on the frames in its range |
| `huge_diameter_only_visits_the_canvas` | A diameter-40000 circle completes quickly; the canvas shows its empty bounding-box corner |

### Morph object — `tests/morph.rs`

| Test | Verifies |
|------|----------|
| `morph_shows_from_on_first_frame_and_to_on_last` | First frame is fully `from`, last frame fully `to` (mode-independent endpoints) |
| `morph_wipe_right_is_half_done_at_the_midpoint` | `wipe-right` flips the left half before the right at progress 0.5 |
| `morph_pads_the_smaller_grid_with_transparent_space` | Cells beyond the smaller grid are transparent spaces (smaller shape grows/shrinks) |
| `morph_horizontal_wipes_flip_a_column_together_in_ragged_rows` | With ragged rows, `wipe-right`/`wipe-left` flip each column on the same frame in every row (thresholds use the grid's width, not each row's) |
| `morph_is_hidden_outside_its_range` | The morph emits nothing before/after its frame range |
| `an_explicit_group_range_gates_a_morph_without_retiming_it` | A morph in a narrowing or widening explicit-range group keeps its own blend timing: the group only decides where it shows (`from` before, `to` after a widened range) |

### Command object — `tests/command.rs`

| Test | Verifies |
|------|----------|
| `command_compiles_to_region_spec` | A command compiles to a `CommandRegion` spec |
| `animated_command_regions_follow_the_drawn_box` | An animated command yields one region per run of frames with the same box geometry; each region's interior and status cell match the box drawn on its frames |
| `command_starting_at_usize_max_yields_one_region_and_no_split` | A `Command` with `frames.start` = `usize::MAX` compiles to its single unsplit region (no overflow, no region on a playable frame) |
| `command_draws_a_clean_placeholder_box_into_the_static_frame` | A command draws a clean placeholder box into the static frame |
| `box_height_follows_the_height_field` | Box height follows the `height` field |
| `border_can_be_disabled_for_a_frameless_region` | The border can be disabled for a frameless region |
| `command_output_renders_clipped_into_region` | Command output renders clipped into the region |
| `command_region_follows_an_explicit_group_range` | An explicit group range moves the `CommandRegion` to the frames the box is drawn on |
| `layout_output_drops_osc_and_other_escape_strings` | OSC (incl. OSC 8 hyperlinks, BEL- or ST-terminated), DCS, charset and CSI sequences leave only their visible text |
| `layout_output_drops_c0_controls_but_keeps_lines_and_tabs` | BEL, BS, SO, SI, VT, FF, DEL, CR and a stray ESC never reach a cell; newlines split rows and tabs become spaces |
| `layout_output_honours_c1_introducers_and_unterminated_strings` | 8-bit C1 CSI/OSC introducers are stripped with their payload; an unterminated OSC ends at the next ESC; an nF escape keeps a following newline |
| `layout_output_never_lets_a_cut_sequence_swallow_a_line_break` | A CSI or OSC cut short at a line end stops there, so the following lines still show |
| `layout_output_keeps_wide_and_zero_width_chars_to_one_column_each` | A CJK char or emoji becomes a one-column `?`, combining marks and variation selectors are dropped, and single-width non-ASCII passes through, so each row is exactly the box width on screen |

### Docs ↔ code sync — `tests/docs.rs`

The markdown docs (`AGENTS.md`, which `CLAUDE.md` and `GEMINI.md` import;
`README.md`, `TESTS.md`, `PRESENTATION_FORMAT.md`, the bs-deck skill) are
load-bearing for AI agents; these tests turn their sync rules into build failures.

| Test | Verifies |
|------|----------|
| `every_test_function_is_listed_in_tests_md` | Every test function in `tests/` and `src/` has a row in this file |
| `tests_md_names_no_test_that_does_not_exist` | This file names no phantom test (rows survive renames/deletes) |
| `documented_test_counts_match_the_code` | The totals at the top of this file and AGENTS.md's count sentence match the code |
| `doc_referenced_repo_paths_exist` | Every backticked `src/…`/`tests/…`/`docs/…`/`examples/…`/`scripts/…` path in the docs exists |
| `object_type_docs_match_the_scene_object_enum` | The object-type lists/counts in AGENTS.md (conventions list and module map), PRESENTATION_FORMAT.md's catalog, and the bs-deck skill (§3 catalog row, a section per tag, §14 self-check tag list) match the `SceneObject` enum (serde snake_case tags) |
| `doc_json_examples_parse_and_compile` | Every complete JSON example in the docs parses with the current serde structs; full decks also validate, compile, and render |
| `agent_doc_stubs_only_import_agents_md` | CLAUDE.md and GEMINI.md are bare `@AGENTS.md` imports, so the agent guide exists in exactly one copy |
| `agents_md_module_map_covers_every_core_module` | Every `.rs` file in `src/`, `src/editor/`, `src/engine/`, `src/renderer/`, `src/player/` is mentioned in AGENTS.md (per-object files under `src/engine/objects/` are exempt) |
| `presentation_format_section_references_resolve` | Every `§N` and `#anchor` reference in `PRESENTATION_FORMAT.md` names an existing heading |

## Inline unit tests (`src/`)

### Compile pipeline — `src/compile.rs`

| Test | Verifies |
|------|----------|
| `compile_json_round_trips_a_deck_into_playable_json` | Source JSON in → playable JSON out: the result parses as a `PlayablePresentation` with the deck's contract and one frame per `frame_count` |
| `compile_json_reports_a_parse_error_rather_than_panicking` | Malformed JSON surfaces an error message instead of panicking — the wasm module depends on this (a panic there is an unrecoverable trap) |
| `compile_rejects_overlapping_loops` | `validate_loops` runs as a hard gate before compiling, and its failure is reported as `invalid loops or animations` |
| `compile_rejects_a_deck_with_no_frames` | `frame_count` 0 is an error, not an empty playable the player would crash on |
| `compile_rejects_an_oversized_canvas` | A 65535×65535 canvas (over `MAX_CANVAS_CELLS`) is an error up front, not a multi-GB allocation that aborts the CLI or traps the wasm module |
| `compile_rejects_an_oversized_frame_count` | A `frame_count` over `MAX_FRAME_COUNT` is an error up front, not billions of scenes |
| `compile_rejects_a_deck_that_stores_too_many_cells` | A deck within the contract limits whose frames would store over `MAX_STORED_CELLS` cells (full frame + diff changes) is an error, not a multi-GB playable |
| `compile_rejects_a_frame_of_too_many_overlapping_objects` | Hundreds of overlapping full-canvas objects on one frame (within every other limit) are an error once the frame draws over `MAX_FRAME_OPS` cells, checked object by object so memory stays bounded |
| `rendering_stops_once_the_frames_draw_too_much_in_total` | `render_scenes` stops once the frames' draw ops together pass the deck budget (a small one standing in for `MAX_DECK_OPS`), naming the frame; a normal deck compiles |
| `compile_rejects_too_many_cells_in_total` | A deck within the canvas and frame limits but over `MAX_DECK_CELLS` (`frame_count × width × height`) is an error |

### Old-format migration — `src/migrate.rs`

| Test | Verifies |
|------|----------|
| `links_a_coord_to_its_existing_animation_by_span` | An old `{…,start_frame,end_frame}` coord is rewritten to `{from,to,anim}` referencing the id of the `animation` whose span matches; old span fields dropped; result parses |
| `synthesizes_an_animation_for_an_orphan_span` | An animated coord with no matching `animation` gets a synthesized sidecar (correct exclusive span, `auto_play: false`) which it then references |
| `a_migrated_deck_that_a_loop_would_cut_is_not_written` | A migrated deck that fails what compile checks first (a loop cutting a synthesized span) is refused with "left the file unchanged"; the same deck without the loop migrates |
| `two_coords_sharing_a_span_share_one_animation` | x and y over the same span resolve to one shared animation id |
| `already_migrated_is_a_noop` | A current-format file is left byte-identical (`Report::unchanged`) |
| `synthesized_ids_avoid_existing_ones` | A synthesized animation's id is past the largest existing id (no collision) |

### Property editing — `src/editor/properties.rs`

| Test | Verifies |
|------|----------|
| `coordinate_rejects_non_finite_values_and_clamps_to_the_cell_range` | A typed coordinate of `inf`/`1e999`/`-inf`/`nan` is rejected; finite values clamp to `0..=65535` and keep fractions |
| `label_properties_roundtrip` | `Label` properties round-trip through get/set |
| `hline_properties_roundtrip` | `HLine` properties round-trip through get/set |
| `rect_properties_roundtrip` | `Rect` properties round-trip through get/set |
| `header_properties_roundtrip` | `Header` properties round-trip through get/set |
| `arrow_properties_roundtrip` | `Arrow` properties round-trip through get/set |
| `art_properties_roundtrip` | `Art` properties round-trip through get/set |
| `table_properties_roundtrip` | `Table` properties round-trip through get/set |
| `morph_properties_roundtrip` | `Morph` properties round-trip through get/set |
| `animation_properties_roundtrip` | `Animation` properties round-trip through get/set |
| `auto_advance_properties_roundtrip` | `AutoAdvance` properties round-trip through get/set |
| `group_properties_roundtrip_and_bounds` | `Group` properties round-trip and bounds compute; explicit range shows values + override note |
| `explicit_group_note_says_nested_groups_are_not_overridden` | An explicit range's panel note says it has no effect when no direct member can be gated (only a nested group), and names nested groups as not overridden when one is a member |
| `auto_group_shows_blank_frames_and_no_note` | An auto group shows blank first/last frame and no override note |
| `command_properties_roundtrip` | `Command` properties round-trip through get/set |
| `command_args_round_trip_exactly_through_the_field` | The args field shows arguments shell-quoted (empty, spaced, `'`, `"`, `\`) and committing it unchanged gives back the identical list; plain space-separated args and double quotes split as in a shell |
| `command_args_reject_an_unbalanced_quote_and_keep_the_args` | An unbalanced `'`/`"` or a trailing backslash in the args field is an error and leaves the args unchanged |
| `list_properties_roundtrip` | `List` properties round-trip through get/set |
| `loop_properties_roundtrip` | `Loop` properties round-trip; editing `delay_ms`/`bounce` sticks |
| `circle_properties_roundtrip` | `Circle` properties (diameter, fill char, colours, frames) round-trip through get/set |
| `unknown_property_is_rejected` | An unknown property name is rejected |
| `coordinate_get_set_roundtrips` | Coordinate get/set round-trips |
| `resize_group_scales_members_with_fractional_precision` | `resize_group` scales members with fractional precision |
| `moving_an_hline_or_arrow_into_the_left_edge_keeps_its_shape` | An HLine at `x_start` 0 moved left keeps its length, and an Arrow moved into the edge stops there with its shape kept; right/down moves are unlimited |
| `moving_a_group_into_the_left_edge_keeps_its_members_offsets` | A group (and a set moved together, as the paste ghost) stops at the edge as a whole: members keep their relative offsets, the free axis still moves |
| `nested_group_bounds_move_and_resize_cover_the_inner_members` | An outer group's bounds, `move_group` and `resize_group` reach the members of a nested group, ignore non-placed members (a `Loop`), and a self-listing group terminates |
| `common_properties_intersects_shared_editable_props` | `common_properties` keeps the geometry/colour/flag/frame props a Label and Rect share, dropping type-specific ones (and `Text`-kind) |
| `common_properties_shrinks_for_heterogeneous_types` | A Label + Loop selection intersects down to just `first_frame`/`last_frame` |
| `common_properties_value_is_the_first_members` | The representative value shown/seeded is the first member's |
| `rgb_color_rejects_non_hex_without_panicking` | A `#` colour that is not six ASCII hex digits (e.g. a multi-byte char) is an error, not a panic; valid `#rrggbb` still parses |
| `label_exposes_align_and_valign_dropdowns` | `Label` lists `align`/`valign` as dropdown properties with the expected option lists and defaults (`left`/`top`) |

### Loop stepping — `src/player/mod.rs`

| Test | Verifies |
|------|----------|
| `non_bounce_wraps_to_start` | Forward sweep wraps to the start; each wrap is one completed pass |
| `bounce_ping_pongs_without_duplicating_endpoints` | Bounce plays `5,6,7,8,7,6,5` (endpoints once per turn); one pass per round-trip |
| `single_frame_range_completes_each_tick` | A one-frame loop stays put and counts a pass per tick |
| `a_finite_loop_entered_mid_range_plays_a_full_pass_first` | A `count: 1` loop entered on its last frame (← from the slide after it) shows its start and plays a whole pass before exiting, bounce on and off; entered on its start, one pass is unchanged |
| `two_frame_bounce_alternates` | A two-frame bounce alternates `5,6,5,6` |
| `a_command_keeps_running_only_while_playback_stays_in_its_region` | `keeps_command`: stepping within one command region (or a one-frame loop tick) keeps the command; entering, leaving, or crossing into a region with different geometry does not |
| `every_command_on_a_frame_is_active_not_only_the_first` | Every command region covering a frame is active (not just the first), and moving frames keeps or stops each one on its own |
| `two_commands_on_one_frame_both_start_and_finish` | Two command boxes on one frame both start (not twice on a repeat call) and each finishes with its own success / failure |
| `a_loop_left_by_break_out_stays_off_until_playback_leaves_its_span` | A loop pinned to the deck's last frame is not re-armed by a no-op → or a step within its span after a break-out, nor after a jump key (`leave_loop`) lands back inside it; leaving and re-entering arms it again |

### Animation auto-play stepping — `src/player/mod.rs`

| Test | Verifies |
|------|----------|
| `only_navigation_keys_restart_the_auto_advance_timer` | Only the player's navigation keys (→/←, Space, Enter, PageDown/PageUp, Home, End) re-arm the auto-advance timer; `f`, Esc and stray keys leave the time already waited alone, as in the web viewer |
| `loop_wait_prices_the_step_actually_taken` | A loop's wait is the auto-play delay of the boundary its next step crosses — both bounce turnarounds step back inside the animation and use its delay; a non-bounce wrap uses the loop's own delay |
| `auto_advance_delay_covers_only_internal_boundaries` | A span auto-advances only across boundaries internal to it (the exclusive-end boundary is excluded) |
| `auto_advance_delay_takes_the_minimum_over_overlapping_animations` | Where auto-play spans overlap, the boundary delay is the minimum of theirs |
| `auto_advance_delay_handles_backward_boundaries` | Backward stepping uses the boundary below the frame |
| `auto_advance_delay_ignores_non_auto_play_animations` | A non-auto-play animation never drives auto-advance |
| `animation_cluster_spans_a_single_animation` | The skip cluster for a covered frame is the animation's span; frames outside (incl. the exclusive end) yield `None` |
| `animation_cluster_merges_overlapping_animations` | Overlapping auto-play spans merge into one cluster, so a skip clears them all in one keypress |
| `animation_cluster_keeps_disjoint_and_touching_animations_separate` | Spans that only touch at a boundary (share no frame) stay separate clusters |
| `animation_cluster_ignores_non_auto_play_animations` | A non-auto-play animation forms no skip cluster |
| `animation_cluster_skip_target_clamps_to_the_last_frame` | `→` skip target `hi.min(last)` lands on the last frame when the animation ends there; `←` target is the slide before the earliest start |

### Per-frame auto-advance stepping — `src/player/mod.rs`

| Test | Verifies |
|------|----------|
| `frame_auto_advance_delay_covers_its_range_but_not_the_last_frame` | A marker drives every frame in `[start, end)`; the exclusive end is not covered |
| `frame_auto_advance_delay_is_suppressed_on_the_final_frame` | A deck-spanning marker never auto-advances the last frame (nowhere to go) |
| `frame_auto_advance_delay_takes_the_minimum_over_overlapping_markers` | Where markers overlap, the per-frame delay is the minimum of theirs |
| `effective_auto_delay_combines_animation_and_per_frame_markers` | The effective delay is the min of the auto-play animation boundary delay and the per-frame marker |

### Play guard — `src/player/mod.rs`

| Test | Verifies |
|------|----------|
| `play_rejects_a_presentation_with_no_frames` | A playable with no frames is an error before the terminal enters raw mode, not a `frames[0]` panic |

### Command process handling — `src/player/mod.rs`

| Test | Verifies |
|------|----------|
| `needed_terminal_adds_the_two_bars_without_overflow` | The player's terminal-size requirement (canvas + menu bar + status bar) is computed in u32, so a canvas 65535 rows tall asks for 65537 rows ("Terminal too small") instead of overflowing |
| `a_stop_signal_becomes_a_stop_request_instead_of_killing_the_player` | While the player's signal guard is installed, SIGTERM only sets the stop flag the event loop checks (so the command group is killed and the terminal restored) |
| `a_command_leads_its_own_session_without_a_terminal` | Linux: a command leads its own session (so the group kill still covers it) and has no controlling terminal, so opening `/dev/tty` fails at once instead of printing a prompt over the slide and stopping |
| `a_command_is_killed_when_its_spawning_thread_dies_without_teardown` | Linux backstop for a SIGKILLed player: with no destructor run, `PR_SET_PDEATHSIG` still SIGKILLs the command when the thread that spawned it exits |
| `reaping_a_command_that_could_not_be_killed_never_blocks` | Linux: reaping a command the player could not signal (a `sudo` leader) gives up within `REAP_WAIT` instead of blocking the UI thread, and a background thread reaps it once it exits |
| `a_command_that_exited_in_time_is_not_timed_out_when_noticed_late` | A command that exits 0 before its timeout but is first polled after the deadline reports success, not `[timed out]` (exit is checked before the deadline) |
| `timeout_finishes_even_when_a_grandchild_holds_the_pipe` | A timed-out command finalizes (✗, `[timed out]`) even when a grandchild holds its output pipe |
| `a_grandchild_left_running_is_killed_when_the_command_exits` | When the command exits, its process group is killed, so a backgrounded grandchild neither delays finalizing nor survives |
| `a_descendant_outside_the_group_holding_the_pipe_does_not_block` | A descendant that escaped the group (`setsid`) and holds the pipe does not delay finalizing past the drain grace |
| `append_output_matches_plain_concatenation_under_the_caps` | Output under the caps is kept byte for byte, however it is chunked |
| `append_output_truncates_a_line_past_its_cap` | Bytes of a line past the per-line cap are dropped until its newline |
| `append_output_returns_to_the_line_start_on_a_lone_cr` | A lone `\r` drops the line's text so far (a progress line shows its latest state) while `\r\n` is a newline, also when the CR or CRLF straddles reader chunks |
| `append_output_keeps_a_bounded_tail_with_the_same_last_lines` | A flood of lines stays within the buffer bound and lays out to the same last rows as the full output |
| `a_flooding_command_keeps_a_bounded_buffer` | A command streaming ~2 MB keeps the player's captured buffer bounded throughout, ending with the right last row |

### Word-wrap — `src/engine/objects/wrap.rs`

| Test | Verifies |
|------|----------|
| `a_word_longer_than_the_width_is_hard_broken` | A word longer than the width is hard-broken |
| `a_long_first_word_never_strands_the_marker_or_drops_the_indent` | On the first row a space inside the list marker or leading indentation is not a break point: `- `/`1. ` lines and an indented line hard-break instead (no lone bullet, blank row or lost indent); a space after real content still soft-breaks |
| `continuation_indent_is_clamped_below_the_width` | The continuation indent is clamped below the width |

### Word-wrap (indexed) — `src/engine/objects/table.rs`

| Test | Verifies |
|------|----------|
| `indexed_wrap_matches_plain_wrap_and_maps_indices` | Indexed wrap matches plain wrap and maps source indices |
| `indexed_wrap_counts_newlines_in_source_offsets` | Indexed wrap counts newlines in source offsets |
| `caret_blank_pos_opens_a_new_line_after_trailing_newline` | Caret-blank position opens a new line after a trailing newline |
| `caret_blank_pos_on_a_newline_ends_its_own_line` | A cell-edit caret on a mid-text newline sits at the end of its own line, and one on an empty line at that line's column 0 (insert-before) |

### Terminal session — `src/term.rs`

| Test | Verifies |
|------|----------|
| `key_press_ignores_releases_and_non_key_events` | The shared `key_press` filter yields only key *presses*, so a release (reported on Windows) can neither discard unsaved decks at the editor's quit confirmation nor repeat a player step/toggle |

### Text buffer — `src/editor/textedit.rs`

| Test | Verifies |
|------|----------|
| `insert_and_motion` | Insert and cursor motion |
| `insert_str_normalises_line_breaks_and_drops_them_when_single_line` | Pasted text is inserted at the cursor with `\r\n`/`\r` as `\n` (or dropped in a single-line field), a tab as a space and other control chars dropped |
| `multiline_line_col_and_vertical_motion` | Multiline line/col tracking and vertical motion |
| `newline_inserts_rather_than_commits` | Newline inserts rather than commits |

### Object defaults — `src/editor/object_defaults.rs`

| Test | Verifies |
|------|----------|
| `object_type_registry_is_complete` | The `SceneObject` enum, `OBJECT_TYPES`, and the documented sub-menu-only set name exactly the same types (exhaustive-match guard forces every new variant through this check) |
| `create_default_covers_every_object_type` | `create_default` builds the expected variant for every `OBJECT_TYPES` index |
| `every_type_has_a_unique_shortcut_key` | Each type has a unique quick-add key (case-insensitive, not the global `f`) carried on its `OBJECT_TYPES` descriptor |

### Key bindings — `src/editor/config.rs`

| Test | Verifies |
|------|----------|
| `ctrl_shift_binding_requires_both_modifiers` | `Ctrl-Shift-` bindings need both modifiers (char case-insensitive); Ctrl-only still matches the plain `Ctrl-` binding |
| `capital_letter_binding_matches_every_shift_encoding` | A capital-letter binding (e.g. save-as `S`) fires on Shift+S however the terminal encodes it (`Char('S')`±SHIFT, or `Char('s')+SHIFT`), never on a plain `s` or with Ctrl |
| `lowercase_binding_does_not_fire_on_a_shifted_letter` | A lowercase binding (`s`) matches a plain `s` but not a shifted one, so it can't swallow a capital-letter binding |

### Animate sub-menu fields — `src/editor/input.rs`

| Test | Verifies |
|------|----------|
| `table_cell_editing_inserts_before_the_caret` | Table cell text editing follows the insert-before caret: Left then type lands before the shown caret, Backspace deletes the char before it, typing at the cell start prepends, Enter saves |
| `animate_add_frames_after_a_morph_animates_the_same_object` | Animate with add frames on a label listed after a morph ending there animates the label (now one index later, behind the morph's continuation), the menu follows it, and `clipboard_sources` is remapped |
| `converge_add_frames_on_a_morphs_last_frame_keeps_its_blend` | Converge with add frames on a morph's last frame: every morph slide renders as before and the inserted frames hold its finished state |
| `converge_add_frames_keeps_an_earlier_animations_span` | Converge with add frames on an earlier animation's last frame leaves that Animation's span alone: the motion it drives on its slides is unchanged and holds its end value on the inserted frames |
| `add_frames_refuses_a_span_that_does_not_start_on_the_current_frame` | With add frames on, Animate's and Converge's `[s]` refuse a span whose start is not the current frame (where the frames would be inserted), leaving the deck untouched and the menu open; without add frames the span is accepted |
| `animate_refuses_an_out_of_range_end_without_inserting` | Animate's and Converge's `[s]` with an end past the deck-size limit (add frames on) or past the deck (off) is refused with an error, inserting nothing and keeping the menu open |
| `single_object_delete_prunes_an_animation_it_alone_drove` | Confirming `[d]` on the only object an `Animation` drives removes that `Animation` too |
| `animate_two_axis_layout_exposes_x_and_y_fields` | A position (two-axis) animation lists `x from/to` and `y from/to` (10 fields), values per axis |
| `animate_single_axis_layout_has_one_from_to_pair` | A 1-D coordinate (width/height) lists a single `from/to` pair (8 fields) |
| `gap_strobes_even_without_add_frames` | `apply_animation` with gap > 0 strobes the element onto every `gap+1`th frame even when `add frames` is off (works on existing frames) |
| `re_applying_a_gapped_animation_does_not_stack_orphan_copies` | Re-applying clears prior strobe copies first (idempotent); gap 0 removes them entirely |
| `editing_a_strobe_copy_edits_the_whole_set` | Moving a gap-strobe copy (arrow key) or retyping its text reaches the whole set, so a later span edit shows the element exactly once per sample frame |
| `a_span_edit_beside_a_strobe_copy_does_not_overwrite_other_objects` | Bulk-editing `[Animation, copy]`'s `last_frame` re-strobes the set; the strobe sync then stays out (it only mirrors pure in-place edits), so an unrelated object is never cloned over the set |
| `moving_or_resizing_a_group_edits_its_members_whole_strobe_set` | Moving or resizing a `Group` that lists a strobed element's original reaches every copy, so a later span edit shows the element exactly once per sample frame |
| `a_linked_edit_of_a_strobed_element_reaches_its_whole_set` | A text edit of a strobed label linked to a pasted clone reaches the clone and every strobe copy, so a later span edit shows one label per sample frame |
| `editing_a_strobe_copy_reaches_the_originals_linked_clone` | Editing a strobe copy (on a later sample frame) propagates to the original's linked clone as well as the whole strobe set |
| `a_linked_paste_of_a_strobe_copy_links_to_the_original` | A linked paste copied from a later sample's strobe copy is linked to the set's original, so an edit through the original reaches it and re-applying the animation keeps the link |
| `linked_tables_share_cell_and_column_edits` | A cell typed into or a column added to one linked table is copied onto its linked sibling, so both keep the same cells and column layout |
| `linked_table_edits_skip_a_diverged_table` | A column removed (via its confirm) from, or a cell typed into, a linked table reaches a linked table with the same content, while one whose cells already differed keeps all of them and the status says it was not synced |
| `add_frames_hold_survives_later_edits_of_the_earlier_animation` | After add frames from an earlier animation's last frame, its element holds its end on the new frames (gap 0; a gap-1 strobe keeps to its samples); re-applying the animation with a new end and editing its span longer and shorter keep exactly one copy of the element on every frame it shows on |
| `a_split_element_keeps_its_pieces_through_a_re_apply` | An element driven by a gapped and a non-gapped animation, split by a block copy, keeps its head, linked continuation and the copied block intact when the gapped animation is re-applied (and without a gap); a longer span extends only the continuation's outer edge |
| `converging_a_linked_paste_locks_both_to_the_span` | A linked stamp is not taken for a split piece: converging it with its source locks both to the whole converge span |
| `a_gap_strobe_is_refused_for_a_member_of_an_explicit_group` | Animate and Converge `[s]` with gap frames on a member of an explicit-range group, making a strobed direct member's auto group explicit, and (in a hand-edited deck) a span edit of the gapped animation are refused with the deck unchanged; without a gap the animation applies, and an outer group around the strobed member's group may still take a range |
| `re_animating_from_a_strobe_copy_makes_one_set` | Animate `[s]` from a strobe copy re-animates the set's original: one set with the new motion, not a second set |
| `editing_a_gapped_animations_span_restrobes_it` | Editing a gapped `Animation`'s last frame re-strobes the element over the new span (one single-frame copy per sample, no stretched duplicates), and `remove_animation` then leaves one object |
| `animating_another_coordinate_keeps_a_gap_strobe` | Animating another coordinate of a gap-strobed element keeps the first animation's strobe over its own span |
| `removing_one_of_two_animations_keeps_the_other_ones_strobe` | Deleting one of two animations on an element re-locks it to the other one's strobe (or span) as one unit, so a later span edit or re-apply leaves no stacked duplicates |
| `select_action_submenu_offers_copy_converge_delete_and_edit_props` | The post-multi-select action sub-menu lists Copy, Converge, Delete, then Edit Props |
| `converge_field_rows_omits_the_per_object_from_fields` | The Converge config lists only the shared `x to`/`y to` + span/toggles (8 fields) — no per-object `from` |
| `converge_animates_each_object_from_its_own_spot_to_the_shared_point` | `apply_converge` animates each member's x/y from its own current position to the shared target; both axes (and all members) reference **one** shared animation id over the span |
| `re_timing_a_converge_from_one_member_re_locks_every_member` | Re-animating one member of a Converge with a longer span re-locks every member to the new span: both visible to the end and both reach the target |
| `re_strobing_a_converge_from_one_member_leaves_no_stale_copies` | Changing a shared Converge animation's gap from one member (1 → 2 → 0) re-strobes every member on exactly the new sample frames, with no stale copies |
| `converge_remaps_a_member_above_another_members_gap_copies` | Converging a member that sits above another member's gap-strobe copies still converges that member (indices remapped after the copies are cleared; no out-of-bounds panic) |
| `bulk_setting_x_skips_an_animated_member_and_leaves_no_orphan` | Bulk-setting `x` on an animated + fixed pair skips the animated member (its motion and `Animation` stay, indices unchanged) and sets the fixed one |
| `animate_menu_revert_keeps_the_index_on_the_label_after_pruning_below_it` | The animate menu's `[x]` revert prunes orphan animations stored before the label and returns to the label's shifted index |
| `animate_menu_apply_keeps_the_index_on_the_label_after_pruning_below_it` | The animate menu's `[s]` apply orphans an animation stored before the label and returns to the label's shifted index |
| `toggling_back_to_independent_keeps_the_linked_stamps_links` | A copy stamped while Linked stays linked when `[l]` is toggled back to Independent before Esc; a copy stamped as Independent stays unlinked |
| `linked_paste_after_a_prune_below_the_copied_object_links_that_object` | After copying an object, a prune below it shifts `clipboard_sources` with it, so a linked paste links the copied object, not whatever took its old index |
| `note_removed_objects_shifts_sources_and_forgets_a_removed_one` | `note_removed_objects` shifts `clipboard_sources` past each removal and forgets them all when a copied original is removed |
| `apply_animation_keeps_clipboard_sources_valid` | `apply_animation`'s orphan prune shifts `clipboard_sources`, and its gap-copy clear keeps a copied strobe copy's source (the set's original) |
| `animate_menu_revert_keeps_a_copied_strobe_copys_source` | `[x]` clears the strobe copy that was copied; its clipboard source, anchored on the set's original, survives and stays valid |
| `converge_keeps_clipboard_sources_valid` | `apply_converge`'s orphan prune shifts `clipboard_sources`, and its gap-copy clear keeps a copied strobe copy's source (the set's original) |
| `replacing_an_auto_advance_marker_keeps_clipboard_sources_valid` | Replacing a frame's auto-advance marker stored below a copied object shifts `clipboard_sources` with it |
| `animating_a_second_coordinate_does_not_retime_the_first` | Animating `width` on an object whose `x` is already animated creates a second animation and leaves `x`'s span untouched |
| `toggling_bold_on_mixed_table_cells_sets_one_value` | Toggling bold on a mixed multi-cell table selection sets every cell to the opposite of the shown (first) cell's value, then back to plain with the style cleared |
| `a_rejected_frame_value_leaves_an_auto_group_auto` | A rejected first-frame value (`x`, `-1`, `1.5`) on an auto group shows an error and leaves it auto and the deck clean; a valid one still makes the range explicit |
| `a_pasted_morph_shows_what_it_showed_where_it_was_copied` | A morph copied on its finished slide (or mid-blend) and pasted elsewhere shows the art it showed where it was copied, not `from` |
| `a_pasted_animated_label_lands_where_it_was_copied` | An animated label copied on frame 3 and pasted on frame 15 lands at its frame-3 position (plus the paste nudge), also when its `Animation` is deleted between copy and paste |
| `copying_a_loop_or_marker_with_a_label_pastes_only_the_label` | An object copy leaves out a selected `Loop` and `AutoAdvance` marker (noted in the status), so pasting inside the loop adds neither and keeps the deck valid; copying only span behaviours is refused |
| `editing_an_animation_span_updates_one_animation_not_two` | Re-applying with a changed span updates the *same* `Animation` in place (same id, widened span, object range re-locked) — never spawns a second, the reported orphan-duplicate bug |
| `copy_paste_never_duplicates_an_animation` | Object copy leaves `Animation`s out of the clipboard, so a paste never adds a second `Animation` with the same id; copying only an `Animation` keeps the clipboard and explains why |
| `editing_an_animation_span_past_the_deck_is_rejected` | Editing a gapped `Animation`'s first/last frame in the props panel to an empty span or one ending past the deck is refused with an error before any re-strobe, leaving the deck untouched; an in-deck span applies |
| `a_frame_value_off_the_deck_is_rejected` | A typed `first_frame`/`last_frame` off the deck (incl. `usize::MAX`) or making the range empty is refused with the object unchanged; an in-deck value applies |
| `a_paste_lands_whole_in_a_text_field_and_is_ignored_elsewhere` | A bracketed paste is ignored in Normal mode (its characters never act as commands), lands whole with its line breaks in the text overlay, and loses them in a single-line field; a digit-only field keeps only the digits (Settings within its cap) and a table column field acts on the pasted number |
| `a_stale_group_member_is_dropped_on_open_and_never_used` | A group whose `members` name a missing object (or repeat one) opens with those entries dropped and a status note, and copying or converging a group with a stale member set after open never indexes it |
| `a_stale_link_index_is_dropped_on_open_and_never_used` | A deck whose `links` name a missing object opens with the entry dropped (and a status note), and a linked property edit never indexes a stale entry; a repeated entry counts once |
| `linked_edit_keeps_endpoint_geometry_per_copy` | Editing a linked `Arrow` copy's endpoint leaves its sibling's geometry alone, while an appearance prop still propagates |

### Editor animation-skip navigation — `src/editor/input.rs`

| Test | Verifies |
|------|----------|
| `frame_ops_are_refused_on_an_over_limit_deck` | On a deck past `MAX_FRAME_COUNT` the frame menu does not open and a huge frame selection is refused at once, with an over-the-limit status |
| `frame_inserts_are_refused_at_the_deck_limit` | At `MAX_FRAME_COUNT`, adding or copying a frame and block-copying frames are refused with the `validate_contract` error, leaving the deck unchanged |
| `shift_skip_cluster_spans_a_single_animation` | The editor's skip cluster for a frame inside a span is that span (exclusive end); frames outside yield `None` |
| `shift_skip_cluster_merges_overlapping_animations` | Overlapping spans merge into one cluster, so a skip clears the whole group |
| `shift_skip_cluster_keeps_disjoint_animations_separate` | Spans that only touch at a boundary stay separate clusters |
| `shift_skip_cluster_includes_non_auto_play_animations` | Unlike the player's cluster, the editor's includes non-auto-play animations (no auto-advance distinction while authoring) |
| `shift_arrow_through_handle_key_jumps_across_an_animation` | End-to-end through `handle_key`: Shift+→/← inside a span land just past/before it, fall back to the ±10 scrub off-span, and the `[`/`]` aliases do the same jump |

### Deck identity, save-as and quit — `src/editor/mod.rs`

| Test | Verifies |
|------|----------|
| `same_file_sees_through_spelling_whether_or_not_the_file_exists` | `a.json` and `dir/./a.json` are one file both before and after it exists; different names are not |
| `frame_paste_is_refused_at_the_deck_limit` | Pasting the cross-deck frame clipboard into a deck at `MAX_FRAME_COUNT` is refused, leaving it unchanged |
| `opening_from_the_hub_keeps_the_unknown_field_warning` | Opening a deck through the presentations hub (`open_or_focus`) keeps open's notes, e.g. that an unknown key will be dropped on save |
| `a_frame_block_paste_forgets_the_copied_from_objects` | A cross-deck frame paste that splits a target object (shifting later indices) clears `clipboard_sources`, so a later linked paste never links an unrelated object |
| `open_many_opens_the_same_file_once` | Opening the same file under several spellings yields a single deck |
| `save_as_refuses_another_decks_file_and_confirms_an_overwrite` | Save-as onto a file bound to another open deck is refused; onto an existing file it only warns the first time and overwrites on the second Enter |

### Frame operations — `src/editor/state.rs`

| Test | Verifies |
|------|----------|
| `copy_frame_clones_objects_independently` | Copy deep-clones the frame's objects; editing the copy doesn't change the original |
| `copy_frame_keeps_a_spanning_background_shared` | A deck-wide/spanning object is extended (stays one object), not cloned |
| `overlay_frame_pastes_clones_onto_existing_frame_without_growing_deck` | Overlay deep-clones the source frame's objects onto an existing frame; clones are independent and `frame_count` is unchanged |
| `overlay_frame_skips_objects_already_on_the_target` | An object already visible on the target frame (a spanning background) is not re-cloned/duplicated |
| `overlay_frame_onto_itself_is_a_noop` | Overlaying a frame onto itself pastes nothing |
| `overlay_frame_repoints_cloned_group_members` | A cloned group on overlay points at its cloned member, not the original |
| `a_copied_auto_group_keeps_only_members_on_the_copied_slide` | Copying or overlaying a slide clones an auto group with only the members shown there: a member on another slide stays out of the copy, so moving the copy leaves the original slides unchanged |
| `a_block_copy_does_not_share_a_split_member_into_the_cloned_group` | A block copy whose insert point a group member crosses (split around the copy, no frame in the block) leaves that member out of the cloned auto group, so moving the clone leaves its original slides unchanged |
| `a_block_copy_clones_an_auto_group_with_its_cloned_members` | A block copy clones an auto group whose members it clones (it stays auto), holding exactly those clones and never a member off the copied block; the original group is unchanged |
| `copy_frame_clones_an_explicit_group_member_it_overrides` | Copying a slide clones a member whose explicit group narrows it to that slide, so the original slide keeps the member and the copy shows the clone |
| `copy_frame_drops_a_member_another_explicit_group_shows` | A cloned explicit group drops a member it doesn't clone (shown via another explicit group), so it can't steal it from the original slide |
| `frame_copy_and_overlay_never_clone_a_loop_onto_one_frame` | Overlaying a looped frame onto a frame inside another loop, or copying a loop's last frame, adds no `Loop` and leaves `validate_loops` Ok |
| `block_copy_refuses_to_put_a_loop_inside_a_loop_or_animation` | `copy_frames` refuses (deck unchanged) to copy a block holding a `Loop` into the middle of another loop or an animation (a loop counted by the range an explicit group gives it), and copies it outside them with `validate_loops` Ok |
| `frame_block_paste_refuses_to_put_a_loop_inside_a_loop` | `paste_frame_block` likewise refuses a clipboard holding a `Loop` inside another loop (judged by the range an explicit group gives it), and pastes it outside one |
| `block_copies_take_a_contained_loop_but_not_a_cut_one` | `copy_frames` and `copy_frame_block` copy a `Loop` wholly inside the block but not one the block cuts |
| `overlay_frame_clones_an_explicit_group_member_it_overrides` | Overlaying such a slide clones the overridden member too; the original slide keeps it, the target shows the clone |
| `copy_frames_clones_an_explicit_group_member_it_overrides` | Copying a block containing such a slide does the same |
| `blank_frame_leaves_a_single_frame_object_behind` | Blank insert does not extend the source frame's object into the new frame |
| `blank_frame_still_shifts_later_objects` | Objects on later frames slide forward past an inserted blank frame |
| `delete_frame_fixes_group_member_indices` | Pruning a collapsed object on delete keeps surviving group member indices correct |
| `moving_a_frame_out_of_a_loop_or_animation_is_refused` | A move that takes a slide out of (or into) a loop, animation or multi-frame auto-advance marker — or out of an explicit group overriding a loop's range — is refused (`move_frames_refusal`), incl. the two-loop overlap probe |
| `moving_a_slide_out_of_a_multi_slide_label_splits_it_by_membership` | Moving one slide of a 3-slide label to the end splits it: the original stays on its remaining slides, a linked piece (placed right after it) shows on the moved slide, and it is on no slide in between |
| `moving_a_foreign_slide_into_a_multi_slide_label_does_not_show_it_there` | A slide moved into a label's run does not gain the label; the split piece joins the label's auto group |
| `a_torn_explicit_group_member_is_not_split` | A move that keeps an explicit group whole but tears its member's own range creates no piece (the member is drawn on the group's range) |
| `a_split_piece_joins_the_originals_existing_link_family` | A split piece joins the original's existing link family rather than opening a second one |
| `tearing_a_morph_explicit_group_or_animated_object_is_refused` | `move_frames_refusal` refuses a move that tears a morph, an explicit-range group or an animated object |
| `moving_frames_that_keep_every_span_whole_is_allowed` | Moving a slide outside every span, a whole loop, or reordering inside a loop is allowed, and the moved deck still validates |
| `move_frame_relocates_single_frame_objects_after_target` | Moving a frame after a target remaps single-frame object ranges |
| `move_frame_relocates_before_target` | Moving a frame before a target remaps ranges and returns the new index |
| `move_frame_keeps_a_whole_deck_object_spanning_the_whole_deck` | A whole-deck object still spans the whole deck after a move |
| `move_frame_is_a_noop_onto_itself` | Moving a frame relative to itself is a no-op |
| `move_frames_relocates_a_contiguous_block_after_target` | `move_frames` relocates a multi-frame block after the target, remapping ranges and returning the block's new first index |
| `move_frames_block_before_target` | A frame block dropped before the target lands ahead of it, pushing later frames right |
| `move_frames_target_inside_block_is_a_noop` | Moving a block onto a target *within* it is rejected (no reorder) |
| `move_frames_keeps_a_deck_wide_background_spanning` | A deck-wide object still spans the whole deck after a block move |
| `inserting_frames_saturates_a_hand_edited_huge_range` | Frame inserts and the 1-based first_frame display saturate on a hand-edited range near `usize::MAX` instead of panicking or wrapping, and a frame move walks only the deck's frames of it |
| `copy_frames_duplicates_a_block_after_target` | `copy_frames` inserts `count` new frames after the target and deep-clones the block's per-frame objects onto them; originals untouched |
| `copy_frames_before_front_inserts_at_the_start` | Copying a block before frame 0 inserts at the very front, shifting originals right; clones land on the new front frames |
| `copy_frames_keeps_an_interior_spanning_background_shared` | A deck-wide background the insert stretches over the new frames stays one object (not duplicated); per-frame objects are cloned |
| `parse_frame_selection_handles_lists_ranges_and_mixes` | `1,2,3` / `5-12` / mixes parse to 0-based, sorted, de-duplicated, clamped indices |
| `parse_frame_selection_rejects_bad_input` | Frame 0, non-numbers, reversed ranges, empty, and all-out-of-range are rejected |
| `parse_frame_selection_bounds_work_by_the_deck_size` | A huge range bound (up to `usize::MAX`) expands only to the deck's frames, without overflow; a range starting past the deck selects nothing |
| `delete_frames_removes_highest_first_and_keeps_one` | Multi-delete removes highest index first and never empties the deck (keeps ≥1) |
| `deleting_an_animation_span_freezes_the_coordinates_it_drove` | Deleting the frames of an animation's span freezes each coordinate it drove to what the surviving frames showed (`to` after, `from` before), leaving no dangling id |
| `next_anim_id_skips_an_id_still_referenced_by_a_coordinate` | `next_anim_id` never hands out an id a coordinate still references, even with its `Animation` gone |
| `deleting_one_sample_of_a_gap_strobe_deletes_the_element` | Deleting a copy or the original of a gap-strobed element deletes its whole strobe set, and a later span edit (re-strobing another driven element) brings none of it back |
| `copied_frames_show_a_morph_as_its_source_frames_did` | Frame copy (the morph's last slide shows `to`), overlay of a mid-blend slide, a block copy cutting the morph, a yank + paste of it, and a block copy splitting it all show the morph exactly as the source frames did (`Morph::blend` window) |
| `overlay_leaves_the_target_slides_auto_advance_alone` | Overlaying an auto-advancing slide adds no marker to a target without one, and leaves a target's own 20 s marker as its only one |
| `delete_objects_removes_a_set_and_fixes_group_member_indices` | Multi-object delete removes the whole set and re-points surviving `Group.members` (no stale indices) |
| `deleting_the_last_driver_of_an_animation_prunes_it` | Deleting one of two driven objects keeps their `Animation`; deleting the last one (or a frame delete that collapses it) prunes the now-undriven `Animation` |
| `delete_objects_special_cases_an_animation_in_the_set` | An `Animation` in the delete set is removed via `remove_animation` (motion reverted, no orphan sidecar); plain objects in the set are removed too |
| `set_frame_auto_advance_adds_replaces_and_removes` | The auto-advance helper adds a single-frame marker, replaces it in place on re-set (no duplicate), and removes it on delay 0 |
| `set_frame_auto_advance_splits_a_wider_marker_around_the_frame` | Turning a frame inside a 3-frame marker off leaves it without a delay and its neighbours with theirs; a longer delay on it takes effect; a removed one-frame marker's index is reported |
| `auto_advance_marker_shifts_with_frame_insert_and_delete` | A marker's frame range shifts with a blank-frame insert and prunes when its frame is deleted (reuses object range-remapping) |
| `delete_animation_end_frame_keeps_span_and_range_in_lockstep` | Deleting an animation's last frame shrinks the `Animation` span and the driven object's range together, so the motion still reaches `to` instead of stopping short |
| `delete_frame_range_keeps_multiple_animations_consistent` | Deleting a range straddling several animations leaves each driven object's range equal to its (also-shifted) `Animation` span |
| `unknown_fields_finds_dropped_keys_but_not_known_defaults` | `unknown_fields` lists keys a save would drop (top-level, per-object and nested — `title`, `_comment`, `styel`, `style.fgg`) but not a known field at its default (`z_order: 0`, even repeated on 400 objects), none for an editor-written deck, and all of 5000 junk keys with probing capped |
| `opening_a_deck_with_unknown_fields_warns_on_open_and_first_save` | Opening a deck with an unknown key warns that it will be dropped on save; the first save says it was dropped, later saves say nothing |
| `opening_a_deck_with_a_duplicate_animation_id_warns` | Opening (and saving) a deck whose animations share an id puts the validation error in the status line |
| `save_refuses_a_deck_that_would_not_load_back` | Saving a deck with an infinite coordinate fails (it would serialize as `null`), keeps it dirty and writes nothing |
| `opening_an_over_limit_deck_warns_in_the_status_line` | `EditorState::open` (the path `bs edit` takes) and the open/save `deck_warning` also report a deck over the size limits (`validate_contract`), e.g. a hand-edited huge `frame_count` |
| `save_as_writes_the_file_and_adopts_the_path` | `save_as` writes valid JSON to the new path, adopts it as `file_path`, and clears `dirty` |
| `failed_save_as_keeps_the_old_path` | A `save_as` whose write fails keeps the old `file_path` and the `dirty` flag |
| `replace_file_failure_leaves_the_original_intact` | A save whose write fails part-way leaves the existing deck file untouched and removes its temp file |
| `replace_file_refuses_a_read_only_target` | A save to a write-protected deck fails and leaves it untouched (checked only where the OS refuses the write, i.e. not as root) |
| `save_replaces_content_through_a_symlink_keeping_permissions` | Saving through a symlink replaces the link's target with a new file (the link stays a link), keeps the file's permissions, isn't redirected by a symlink planted on the temp name, and leaves no temp file |
| `animation_span_unions_the_referenced_animations` | `scene_object_animation_span(obj, &anims)` unions the spans of the animations a coordinate references into an exclusive `[start, end)`; `None` when nothing is animated |
| `add_frames_and_share_grows_the_deck_and_shares_elements` | Animating over N frames inserts N-1 fresh frames and extends every current-frame element to span them (shared object) |
| `a_morphs_continuation_keeps_its_draw_order` | The frozen morph continuation is placed right after the morph, so an equal-z object drawn over the morph still draws over it on the held frames, which render exactly like the morph's last slide; `add_frames_and_share` returns the moved indices |
| `a_morphs_frozen_continuation_is_not_driven_by_its_motion` | The frozen continuation of a moving morph is flattened in place, so removing the morph's motion animation leaves it on the new frames instead of stretching it back over the morph's slides |
| `add_frames_on_a_morphs_last_frame_keeps_its_blend` | `add_frames_and_share` on a morph's last frame keeps the morph's range (its blend timing): its slides render as before and a frozen continuation holds the finished state on the new frames |
| `add_frames_on_an_earlier_animations_last_frame_keeps_its_span` | `add_frames_and_share` from an earlier animation's last frame does not stretch that `Animation` or an `AutoAdvance` marker there (a `Loop` grows, so the deck still validates); the driven object keeps its motion and holds its end position on the new frames |
| `add_frames_and_share_inserts_n_minus_1_fresh_frames` | N-1 *new* frames are always inserted after the current one (existing frames shift back), not reused — even when the deck already has frames in the span |
| `ensure_animation_updates_the_same_id_in_place` | `ensure_animation` with an existing id updates that one `Animation` in place (span + config) — never spawns a second |
| `ensure_animation_appends_a_distinct_id` | `ensure_animation` with a new id appends a second `Animation` |
| `apply_gap_strobes_element_onto_every_nth_frame` | `gap_frames` keeps the original on the first sample frame and clones the element onto every `gap+1`th frame (single-frame samples keeping the animated coordinate); the `gap` frames between are blanks |
| `apply_gap_with_a_huge_gap_keeps_only_the_first_sample` | A gap of `usize::MAX` completes without overflow and keeps only the first sample |
| `apply_gap_of_zero_is_a_noop` | A gap of 0 (no empty frames) leaves the element spanning every frame (off) |
| `restrobe_clips_a_huge_gapped_span_to_the_deck` | Re-strobing an element under a hand-edited span far past the deck samples only the deck's frames, so the copies stay bounded |
| `clear_gap_clones_removes_only_matching_copies` | Clearing an element's strobe removes its single-frame clones in span but leaves the original and unrelated objects |
| `clear_gap_clones_spares_a_different_animation_with_the_same_motion` | Matching strobe copies by whole-object content (not motion alone): clearing one animation leaves an overlapping animation's gap frames intact even when both share the same from/to/span |
| `remove_animation_reverts_motion_and_drops_the_sidecar` | `remove_animation` flattens a coordinate animated over the span back to `Fixed` (its `from`), keeps the object spanning the range statically, and deletes the `Animation` sidecar |
| `remove_animation_clears_gap_strobe_copies` | Removing a gapped animation deletes the strobe clones and restores one static element across the span (no scattered samples), sidecar gone |
| `remove_animation_spares_an_overlapping_animation_on_another_span` | Removing one animation leaves an overlapping animation on a different span — its motion and its sidecar — untouched |
| `prune_orphan_animations_keeps_a_still_used_sidecar` | `prune_orphan_animations` keeps an `Animation` while a coordinate references it, and removes it once the motion is reverted |
| `flatten_coordinates_converts_animated_to_fixed_at_frame` | Pasting flattens an animated coordinate (via the `AnimSpans` table) to a `Fixed` value sampled at the frame, so the copy is static and arrow-movable on both axes |
| `expand_selection_pulls_in_group_members` | Copying a group expands the selection to include its members (deduped/sorted) |
| `copying_a_nested_group_keeps_the_inner_groups_members` | `expand_selection` pulls in a nested group's members (cycle-safe), so the cloned inner group keeps its members |
| `clone_selection_remaps_members_locally_and_is_independent` | A cloned group points at its cloned members (selection-local); clones are independent of the originals |
| `clone_selection_drops_members_outside_the_selection` | A group member not in the selection is dropped from the clone's member list |
| `link_siblings_returns_family_minus_self` | `link_siblings` returns the rest of an object's link family; empty when unlinked |
| `delete_shifts_and_prunes_link_families` | Deleting an object drops it from link families, shifts higher indices, and prunes families that fall below two members |
| `copy_frame_block_normalises_ranges_to_block_local` | `copy_frame_block` captures objects overlapping the block, clipping each range into the block's own `0..frame_count` coordinates |
| `copy_frame_block_skips_objects_outside_the_block` | An object visible only outside the copied block is not captured |
| `paste_frame_block_inserts_frames_and_shifts_ranges` | `paste_frame_block` inserts the block's frames into the target deck and shifts pasted ranges to the destination; a target background crossing the paste point is split around the pasted frames (linked continuation right after it) |
| `splitting_an_explicit_group_keeps_its_member_morphs_blend` | When a frame paste or block copy splits an explicit group, a member Morph whose own range crossed the insert point keeps its blend on every original slide, and with the group reverted to auto each piece shows only on its own slides |
| `an_unsplit_explicit_group_keeps_its_member_morphs_blend` | A member Morph whose own range crosses a paste or block-copy insert point keeps its blend stages when its explicit group lies wholly before or after the insert point (the group is not split) |
| `a_pasted_frame_block_is_not_covered_by_a_target_auto_advance_marker` | A target `AutoAdvance` marker crossing the paste point is split around the pasted frames (as a block copy does), so the pasted slides do not auto-advance |
| `a_pasted_frame_block_does_not_show_target_objects_crossing_the_paste_point` | A one-frame block pasted into the middle of a target label's range: the pasted slide renders exactly like its source slide, and every target frame renders as before |
| `paste_frame_block_assigns_fresh_anim_ids_with_no_collision` | A pasted `Animation` gets a fresh id (no collision with an identically-numbered animation already in the target) and the pasted coordinates reference it, while the target's own animation keeps its id |
| `paste_frame_block_repoints_group_members_into_destination` | Block-local `Group.members` in the clipboard are re-pointed to the new absolute indices in the destination deck |
| `copy_frame_block_flattens_animation_outside_the_block` | A coordinate driven by an animation whose span lies outside the copied block is flattened to `Fixed` at capture (no dangling animation reference) |
| `copy_frames_through_a_cut_animation_reproduces_the_frames` | A block copy cutting through an animation (or past one that lies outside it) renders the same grids as the original frames; a one-frame cut copies no `Animation` |
| `copy_frames_splits_a_reveal_straddling_the_seam` | Copying a block right after itself splits a bullet revealed mid-block (it straddles the seam without covering the block) around the copy, so every frame matches its original (its pieces keep its draw order under an equal-`z_order` cover); the continuation is linked to it and joins its auto group |
| `copy_frames_split_tolerates_a_stale_group_member` | Splitting a straddler whose auto group holds an out-of-range member index neither panics nor drops that index |
| `copy_frames_splits_a_straddler_before_the_target` | A `before` copy splits objects that end inside the block or miss it but straddle the seam, so every frame matches its original |
| `copy_frames_keeps_a_seam_straddling_animated_object_shared` | A block copy inserted inside an animated object's range keeps that object one stretched object (it doesn't re-time it) and copies no undriven cut `Animation` |
| `frame_block_paste_through_a_cut_animation_reproduces_the_frames` | The same for `copy_frame_block` + `paste_frame_block` across decks |
| `frame_delete_keeps_members_an_explicit_group_widens` | Deleting the only own frame of members an explicit group widens keeps them (adopting the group's range) and every surviving slide renders as before |
| `frame_delete_drops_members_an_explicit_group_narrowed` | Deleting the one slide an explicit group narrows its members to drops them with the group, so they don't resurface on other slides |
| `frame_delete_ignores_an_outer_groups_range_on_a_nested_group` | A nested group isn't pruned by its outer explicit group's range (groups draw nothing), so its members keep rendering on the surviving slides |
| `copy_frame_block_captures_a_member_widened_by_its_group` | A member shown in the block only through its explicit group's override is captured, block-local on the group's range |

### Frame-bar abbreviation — `src/editor/timeline.rs`

| Test | Verifies |
|------|----------|
| `clip_cols_cuts_a_line_to_the_row` | The timeline's line clip cuts to a column count (chars, not bytes), so the status row and frame-bar note never run past the terminal edge |
| `frame_bar_note_replaces_the_bar_for_an_empty_or_oversized_deck` | A deck with no frames or over `MAX_FRAME_COUNT` gets a one-line note instead of the per-frame bar, so a huge `frame_count` cannot hang or exhaust memory on redraw |
| `pick_indices_shows_three_at_each_edge_plus_the_current_window` | The abbreviated bar selects the first 3 segments, a 3-wide window around the current one, and the last 3 |
| `pick_indices_dedups_when_groups_overlap_near_an_edge` | When the cursor sits near the start/end, the current window merges into the first/last group (no duplicates) |
| `abbreviated_indices_prefers_three_edges_when_it_fits` | A wide row shows the full first-3 / current / last-3 view |
| `abbreviated_indices_shrinks_edges_on_a_narrow_row` | A narrow row drops segments (edge groups shrink 3→2→1) while always keeping the current frame |
| `abbreviated_indices_never_overflow_a_tiny_row_and_keep_the_current` | On a row too narrow for even the 1-edge grouping, segments farthest from the current one are dropped until the bar fits; the current segment always stays |

### Morph stepping — `src/engine/objects/morph.rs`

| Test | Verifies |
|------|----------|
| `frozen_at_draws_what_the_morph_shows_on_that_frame` | `Morph::frozen_at` (from = to = `art_at(frame)`) draws exactly the cells the morph draws on that frame, for every mode and ragged rows |
| `a_huge_blend_window_saturates_instead_of_overflowing` | A hand-edited `blend` window at `i64::MAX`/`MIN` (length `usize::MAX`) keeps progress in `[0, 1]` and `keep_blend` from overflowing |
| `progress_runs_zero_to_one_across_the_range` | `progress` is 0 on the first frame, 1 on the last, linear between |
| `first_frame_is_from_last_frame_is_to` | Resolve emits the `from` glyphs at progress 0 and the `to` glyphs at progress 1 |
| `wipe_right_flips_left_cells_before_right_cells` | `wipe-right` thresholds flip left columns before right at the midpoint |
| `out_of_grid_cells_are_transparent_spaces` | Cells past one grid's extent resolve to (transparent) spaces |
| `outside_the_range_emits_nothing` | Resolve emits no ops outside the frame range |
| `mode_string_round_trips` | `MorphMode::as_str` / `from_str_opt` round-trip; unknown strings are rejected |

### Circle aspect — `src/engine/objects/circle.rs`

| Test | Verifies |
|------|----------|
| `columns_widen_the_diameter_by_the_aspect_ratio` | `Circle::columns` widens the diameter ~2× (and never below 1 column) |
| `rows_for_width_inverts_columns` | `Circle::rows_for_width` inverts `columns` so a width-resize maps back to a diameter |

### Art library — `src/art_library.rs`

| Test | Verifies |
|------|----------|
| `builtins_are_present_and_named` | Built-in art pieces are present and named (incl. the `ball`/`square` morph pair) |
| `load_file_normalises_crlf_and_expands_tabs` | An art file's CRLF / lone-CR line endings become `\n` and tabs expand to 8-column stops, so no `\r`/`\t` reaches the deck |

### Editor panels — `src/editor/panel.rs`

| Test | Verifies |
|------|----------|
| `text_fields_and_object_rows_print_no_control_chars` | A deck string with ESC/BEL (an OSC 52 / RIS payload) prints no control char through a text field (`draw_caret_line`, one char per column so the caret stays put) or an object's Select-list summary (`scene_object_summary`) |

### Canvas preview — `src/editor/preview.rs`

| Test | Verifies |
|------|----------|
| `border_fits_needs_a_spare_ring_and_never_overflows` | The preview outline is drawn only with a one-cell ring of room, and a width/height near `u16::MAX` is "doesn't fit" instead of a u16 overflow |
| `selecting_an_outer_group_focuses_the_nested_groups_members` | Selecting a group that contains a group highlights the inner group's members (its leaves, as `move_group` moves them), not the draw-nothing inner group |
| `focused_preview_applies_explicit_group_ranges_like_compile` | With a focus set (a selected group), the preview gates an explicit-range group's member on the group's range exactly like `Engine::compile` (widening and narrowing), and `objects_on_current_frame` lists the member on exactly those frames |
| `focused_preview_bounds_frame_ops_and_frames` | With an object selected, the preview's own resolve loop stops a frame of hundreds of overlapping full-canvas labels at `MAX_FRAME_OPS` with an error, and (focused or not) resolves only frames `0..=last` |
| `focused_object_is_drawn_on_top_even_at_extreme_z_orders` | The selected object's preview z-boost cannot overflow: it is drawn on top against `i32::MAX`/`i32::MIN` z_orders, including when it is emitted first and must win the tie at `i32::MAX` |

### Layout — `src/editor/ui.rs`

| Test | Verifies |
|------|----------|
| `property_panel_modes_get_a_right_panel` | Every panel mode (incl. `EditMultiProperties`) is allocated right-panel width in `Layout::compute`, so the panel can't silently vanish; navigation modes keep the full canvas |

### CLI — `src/main.rs`

| Test | Verifies |
|------|----------|
| `compile_refuses_to_write_over_its_source` | `bs compile` refuses an output that is its source file (same path, `./`-spelling, symlink), leaving the source untouched; a different output still compiles |

## Not covered (intentional)

These run only in the interactive TUI / at play time and are verified manually.

| Area | Reason |
|------|--------|
| `Command` run-loop | Spawn, piped I/O, timeout, ✓/✗ status — runs at play time in the TUI |
| `Loop` run-loop | Timer-based auto-advance, bounce playback, arrow-key break-out — play time in the TUI (the pure `loop_next` step fn is unit-tested) |
| `Animation` run-loop | Auto-advance across spans + arrow-key skip — play time in the TUI (the pure `auto_advance_delay` and `animation_cluster` are unit-tested) |
| Editor | Mode FSM transitions, immediate-edit-on-add, panel rendering — interactive TUI |
