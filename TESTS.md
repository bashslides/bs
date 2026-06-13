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
| `framed_label_at_the_origin_keeps_its_text_visible_inside_the_border` | A framed label at (0,0) shifts its text inside the border instead of hiding under it |
| `align_center_centres_text_within_the_width` | `align: center` centres each row within `width` |
| `align_right_pushes_text_to_the_right_edge` | `align: right` right-aligns text within `width` |
| `valign_center_offsets_text_down_within_the_height` | `valign: center` vertically centres the rows within `height` |
| `valign_bottom_places_text_on_the_last_row` | `valign: bottom` pushes the rows to the bottom of `height` |
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
| `text_word_wraps_when_too_wide_for_the_canvas` | Header word-wraps onto the next glyph line when too wide for the canvas, breaking on word boundaries |

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

### Morph object — `tests/morph.rs`

| Test | Verifies |
|------|----------|
| `morph_shows_from_on_first_frame_and_to_on_last` | First frame is fully `from`, last frame fully `to` (mode-independent endpoints) |
| `morph_wipe_right_is_half_done_at_the_midpoint` | `wipe-right` flips the left half before the right at progress 0.5 |
| `morph_pads_the_smaller_grid_with_transparent_space` | Cells beyond the smaller grid are transparent spaces (smaller shape grows/shrinks) |
| `morph_is_hidden_outside_its_range` | The morph emits nothing before/after its frame range |

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
| `loop_properties_roundtrip` | `Loop` properties round-trip; editing `delay_ms`/`bounce` sticks |
| `unknown_property_is_rejected` | An unknown property name is rejected |
| `coordinate_get_set_roundtrips` | Coordinate get/set round-trips |
| `resize_group_scales_members_with_fractional_precision` | `resize_group` scales members with fractional precision |

### Loop stepping — `src/player/mod.rs`

| Test | Verifies |
|------|----------|
| `non_bounce_wraps_to_start` | Forward sweep wraps to the start; each wrap is one completed pass |
| `bounce_ping_pongs_without_duplicating_endpoints` | Bounce plays `5,6,7,8,7,6,5` (endpoints once per turn); one pass per round-trip |
| `single_frame_range_completes_each_tick` | A one-frame loop stays put and counts a pass per tick |
| `two_frame_bounce_alternates` | A two-frame bounce alternates `5,6,5,6` |

### Animation auto-play stepping — `src/player/mod.rs`

| Test | Verifies |
|------|----------|
| `auto_advance_delay_covers_only_internal_boundaries` | A span auto-advances only across boundaries internal to it (the exclusive-end boundary is excluded) |
| `auto_advance_delay_takes_the_minimum_over_overlapping_animations` | Where auto-play spans overlap, the boundary delay is the minimum of theirs |
| `auto_advance_delay_handles_backward_boundaries` | Backward stepping uses the boundary below the frame |
| `auto_advance_delay_ignores_non_auto_play_animations` | A non-auto-play animation never drives auto-advance |
| `animation_cluster_spans_a_single_animation` | The skip cluster for a covered frame is the animation's span; frames outside (incl. the exclusive end) yield `None` |
| `animation_cluster_merges_overlapping_animations` | Overlapping auto-play spans merge into one cluster, so a skip clears them all in one keypress |
| `animation_cluster_keeps_disjoint_and_touching_animations_separate` | Spans that only touch at a boundary (share no frame) stay separate clusters |
| `animation_cluster_ignores_non_auto_play_animations` | A non-auto-play animation forms no skip cluster |
| `animation_cluster_skip_target_clamps_to_the_last_frame` | `→` skip target `hi.min(last)` lands on the last frame when the animation ends there; `←` target is the slide before the earliest start |

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
| `every_type_has_a_unique_shortcut_key` | Each type has a unique quick-add key (case-insensitive, not the global `f`) aligned with `OBJECT_TYPES` |

### Key bindings — `src/editor/config.rs`

| Test | Verifies |
|------|----------|
| `ctrl_shift_binding_requires_both_modifiers` | `Ctrl-Shift-` bindings need both modifiers (char case-insensitive); Ctrl-only still matches the plain `Ctrl-` binding |

### Animate sub-menu fields — `src/editor/input.rs`

| Test | Verifies |
|------|----------|
| `animate_two_axis_layout_exposes_x_and_y_fields` | A position (two-axis) animation lists `x from/to` and `y from/to` (10 fields), values per axis |
| `animate_single_axis_layout_has_one_from_to_pair` | A 1-D coordinate (width/height) lists a single `from/to` pair (8 fields) |
| `gap_strobes_even_without_add_frames` | `apply_animation` with gap > 0 strobes the element onto every `gap+1`th frame even when `add frames` is off (works on existing frames) |

### Frame operations — `src/editor/state.rs`

| Test | Verifies |
|------|----------|
| `copy_frame_clones_objects_independently` | Copy deep-clones the frame's objects; editing the copy doesn't change the original |
| `copy_frame_keeps_a_spanning_background_shared` | A deck-wide/spanning object is extended (stays one object), not cloned |
| `overlay_frame_pastes_clones_onto_existing_frame_without_growing_deck` | Overlay deep-clones the source frame's objects onto an existing frame; clones are independent and `frame_count` is unchanged |
| `overlay_frame_skips_objects_already_on_the_target` | An object already visible on the target frame (a spanning background) is not re-cloned/duplicated |
| `overlay_frame_onto_itself_is_a_noop` | Overlaying a frame onto itself pastes nothing |
| `overlay_frame_repoints_cloned_group_members` | A cloned group on overlay points at its cloned member, not the original |
| `blank_frame_leaves_a_single_frame_object_behind` | Blank insert does not extend the source frame's object into the new frame |
| `blank_frame_still_shifts_later_objects` | Objects on later frames slide forward past an inserted blank frame |
| `delete_frame_fixes_group_member_indices` | Pruning a collapsed object on delete keeps surviving group member indices correct |
| `move_frame_relocates_single_frame_objects_after_target` | Moving a frame after a target remaps single-frame object ranges |
| `move_frame_relocates_before_target` | Moving a frame before a target remaps ranges and returns the new index |
| `move_frame_keeps_a_whole_deck_object_spanning_the_whole_deck` | A whole-deck object still spans the whole deck after a move |
| `move_frame_is_a_noop_onto_itself` | Moving a frame relative to itself is a no-op |
| `move_frames_relocates_a_contiguous_block_after_target` | `move_frames` relocates a multi-frame block after the target, remapping ranges and returning the block's new first index |
| `move_frames_block_before_target` | A frame block dropped before the target lands ahead of it, pushing later frames right |
| `move_frames_target_inside_block_is_a_noop` | Moving a block onto a target *within* it is rejected (no reorder) |
| `move_frames_keeps_a_deck_wide_background_spanning` | A deck-wide object still spans the whole deck after a block move |
| `copy_frames_duplicates_a_block_after_target` | `copy_frames` inserts `count` new frames after the target and deep-clones the block's per-frame objects onto them; originals untouched |
| `copy_frames_before_front_inserts_at_the_start` | Copying a block before frame 0 inserts at the very front, shifting originals right; clones land on the new front frames |
| `copy_frames_keeps_an_interior_spanning_background_shared` | A deck-wide background the insert stretches over the new frames stays one object (not duplicated); per-frame objects are cloned |
| `parse_frame_selection_handles_lists_ranges_and_mixes` | `1,2,3` / `5-12` / mixes parse to 0-based, sorted, de-duplicated, clamped indices |
| `parse_frame_selection_rejects_bad_input` | Frame 0, non-numbers, reversed ranges, empty, and all-out-of-range are rejected |
| `delete_frames_removes_highest_first_and_keeps_one` | Multi-delete removes highest index first and never empties the deck (keeps ≥1) |
| `delete_animation_end_frame_keeps_coord_and_range_in_lockstep` | Deleting an animation's last frame shrinks the inclusive `end_frame` with the exclusive range end (`>= deleted`), so the motion still reaches `to` instead of stopping short |
| `delete_frame_range_keeps_multiple_animations_consistent` | Deleting a range straddling several animations leaves each coord span, object range, and auto-play sidecar mutually consistent |
| `save_as_writes_the_file_and_adopts_the_path` | `save_as` writes valid JSON to the new path, adopts it as `file_path`, and clears `dirty` |
| `animation_span_unions_animated_coordinates_and_makes_end_exclusive` | `scene_object_animation_span` unions every animated coordinate's window into an exclusive `[start, end)`; `None` when nothing is animated |
| `add_frames_and_share_grows_the_deck_and_shares_elements` | Animating over N frames inserts N-1 fresh frames and extends every current-frame element to span them (shared object) |
| `add_frames_and_share_inserts_n_minus_1_fresh_frames` | N-1 *new* frames are always inserted after the current one (existing frames shift back), not reused — even when the deck already has frames in the span |
| `upsert_animation_reuses_a_matching_span` | Animating X then Y over the same span keeps one `Animation` (updated in place) |
| `upsert_animation_appends_a_distinct_span` | A different (even overlapping) span creates a second `Animation` |
| `apply_gap_strobes_element_onto_every_nth_frame` | `gap_frames` keeps the original on the first sample frame and clones the element onto every `gap+1`th frame (single-frame samples keeping the animated coordinate); the `gap` frames between are blanks |
| `apply_gap_of_zero_is_a_noop` | A gap of 0 (no empty frames) leaves the element spanning every frame (off) |
| `clear_gap_clones_removes_only_matching_copies` | Clearing an element's strobe removes its single-frame clones in span but leaves the original and unrelated objects |
| `clear_gap_clones_spares_a_different_animation_with_the_same_motion` | Matching strobe copies by whole-object content (not motion alone): clearing one animation leaves an overlapping animation's gap frames intact even when both share the same from/to/span |
| `remove_animation_reverts_motion_and_drops_the_sidecar` | `remove_animation` flattens a coordinate animated over the span back to `Fixed` (its `from`), keeps the object spanning the range statically, and deletes the `Animation` sidecar |
| `remove_animation_clears_gap_strobe_copies` | Removing a gapped animation deletes the strobe clones and restores one static element across the span (no scattered samples), sidecar gone |
| `remove_animation_spares_an_overlapping_animation_on_another_span` | Removing one animation leaves an overlapping animation on a different span — its motion and its sidecar — untouched |
| `remove_orphan_animation_keeps_a_still_used_sidecar` | `remove_orphan_animation` keeps the sidecar while a coordinate still drives its span, and removes it only once the motion is reverted |
| `flatten_coordinates_converts_animated_to_fixed_at_frame` | Pasting flattens an animated coordinate to a `Fixed` value sampled at the frame, so the copy is static and arrow-movable on both axes |
| `expand_selection_pulls_in_group_members` | Copying a group expands the selection to include its members (deduped/sorted) |
| `clone_selection_remaps_members_locally_and_is_independent` | A cloned group points at its cloned members (selection-local); clones are independent of the originals |
| `clone_selection_drops_members_outside_the_selection` | A group member not in the selection is dropped from the clone's member list |
| `link_siblings_returns_family_minus_self` | `link_siblings` returns the rest of an object's link family; empty when unlinked |
| `delete_shifts_and_prunes_link_families` | Deleting an object drops it from link families, shifts higher indices, and prunes families that fall below two members |

### Morph stepping — `src/engine/objects/morph.rs`

| Test | Verifies |
|------|----------|
| `progress_runs_zero_to_one_across_the_range` | `progress` is 0 on the first frame, 1 on the last, linear between |
| `first_frame_is_from_last_frame_is_to` | Resolve emits the `from` glyphs at progress 0 and the `to` glyphs at progress 1 |
| `wipe_right_flips_left_cells_before_right_cells` | `wipe-right` thresholds flip left columns before right at the midpoint |
| `out_of_grid_cells_are_transparent_spaces` | Cells past one grid's extent resolve to (transparent) spaces |
| `outside_the_range_emits_nothing` | Resolve emits no ops outside the frame range |
| `mode_string_round_trips` | `MorphMode::as_str` / `from_str_opt` round-trip; unknown strings are rejected |

### Art library — `src/art_library.rs`

| Test | Verifies |
|------|----------|
| `builtins_are_present_and_named` | Built-in art pieces are present and named (incl. the `ball`/`square` morph pair) |

## Not covered (intentional)

These run only in the interactive TUI / at play time and are verified manually.

| Area | Reason |
|------|--------|
| `Command` run-loop | Spawn, piped I/O, timeout, ✓/✗ status — runs at play time in the TUI |
| `Loop` run-loop | Timer-based auto-advance, bounce playback, arrow-key break-out — play time in the TUI (the pure `loop_next` step fn is unit-tested) |
| `Animation` run-loop | Auto-advance across spans + arrow-key skip — play time in the TUI (the pure `auto_advance_delay` and `animation_cluster` are unit-tested) |
| Editor | Mode FSM transitions, immediate-edit-on-add, panel rendering — interactive TUI |
