# bs

A terminal-native presentation engine written in Rust. Presentations are ASCII art animations that render in the terminal.

## Working in this repo (read first)

- **Never commit or push.** Do not run `git commit`, `git push`, `git add`, or any
  history-mutating git command. The harness around Claude makes commits
  automatically — just edit files and leave them in the working tree.
- **Build/test toolchain.** `cargo` lives under `~/.cargo` — run
  `source "$HOME/.cargo/env"` first. Rust needs a C linker; if the sandbox has
  none and you lack root, a local gcc is extracted at `~/toolchain` (see
  `README.md` for how to recreate it). Run tests with:

  ```bash
  source "$HOME/.cargo/env"
  export PATH="$HOME/toolchain/bin:$PATH"
  export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER="$HOME/toolchain/bin/cc"
  cargo test
  ```

  On a normal machine with `cargo` + `build-essential` on PATH, plain
  `cargo test` is enough.
- `cargo test` also compiles `examples/hello.rs`, so keep that example building
  when object structs change.

## CLI

```bash
cargo run -- compile source.json out.json   # compile source → playable
cargo run -- edit source.json               # interactive editor
cargo run -- play out.json                  # play compiled presentation
```

## Architecture

Three-stage pipeline with clean separation:

```
SourcePresentation (JSON)
  → Engine::compile()     → Vec<ResolvedScene>  (DrawOps per frame)
  → Renderer::render()    → PlayablePresentation (Frame::Full / Frame::Diff)
  → Player::play()        → terminal output
```

The editor runs the full Engine+Renderer pipeline live for WYSIWYG preview.

**Runtime exception — `Command` objects.** Every object is baked into static
frames at compile time *except* `Command`, which runs a binary whose output and
exit status are only known at play time. A `Command` resolves to just a bordered
box (a safe placeholder shown in the editor — the binary is **never** run while
editing or compiling); its spec is emitted as a `CommandRegion` sidecar on
`PlayablePresentation`. The `Player` executes the binary with **piped** stdio
(it can't touch the real terminal), reads it on background threads (so arrow
keys always interrupt and navigate — a slow/hung command can never trap the
deck), enforces an optional timeout (`timeout_secs`; omitted ⇒ no timeout),
paints stdout/stderr into the box interior, and marks
the result with a green `✓` (exit 0) or red `✗` (non-zero / timeout / spawn
failure) on the top edge. Navigation does not branch on exit status — you always
stay on the slide and move on with the arrow keys.

**Runtime exception — `Loop` objects.** Also a play-time behavior that can't be
baked into frames. A `Loop` draws **nothing** (like `Group`); its `frames` range
*is* the loop range, and it emits a `LoopRegion` sidecar on `PlayablePresentation`.
At play time the `Player` enters the loop when navigation lands inside its range
and auto-advances on a timer (`delay_ms`, default 500). With `bounce` (default on)
it ping-pongs forward then backward (`5,6,7,8,7,6,…`); otherwise it restarts
(`5,6,7,8,5,6,…`). `count` plays before moving on (`0` = forever; a finite loop
auto-continues just past its range). The presenter breaks out with the arrow keys:
`→` jumps to the first frame *after* the loop, `←` to the first frame *before* it
(Home/End/q also tear it down). Loops may **not overlap or nest**, and a loop may
**not bisect an `Animation`** (it must contain each animation span wholly or not
at all, since it replays whole animations) — `SourcePresentation::validate_loops`
enforces all three both live in the editor (status warning) and at compile time
(hard error). The pure step function is `player::loop_next`. Loops are added
through the normal **Add Object** menu, so they reuse property editing and frame
insert/delete/move range-remapping for free.

**Runtime exception — `Animation` objects.** A first-class *animation span*,
also play-time. Like `Loop` it draws **nothing**; the motion itself is already
baked into the frames via the objects' `Coordinate::Animated` fields. An
`Animation` records the *span* those coordinates play over (`frames`) plus its
**auto-play** config (`auto_play`, `delay_ms`) and emits an `AnimationRegion`
sidecar. When `auto_play` is set, the `Player` auto-advances across the span on a
timer. Unlike loops, animations **may overlap** freely — where several auto-play
animations cover the same frame boundary, the effective advance delay is the
**minimum** of their `delay_ms` (`Player::auto_advance_delay`). Animations are
created by the editor's **animate sub-menu** (not Add Object); they are still
selectable/editable like a `Loop`. The animate flow's `add_frames` toggle inserts
the spanned frames and **shares** the current frame's elements across them (one
range-extended object per element ⇒ editing one edits all), and X and Y of an
object animated over the same span share **one** `Animation` (reuse-by-span in
`state::upsert_animation`).

**`Group` frame range — auto vs. explicit override.** `Group.frames` is an
`Option<FrameRange>`. A group is a logical container whose members are ordinary
top-level objects that render themselves.

- `None` (*auto*, the default for newly created groups): the group has no range
  of its own; members render on their own ranges. The group's effective span is
  the **union** of its members' ranges (`SourcePresentation::effective_frame_range`),
  and the props panel shows blank first/last-frame fields.
- `Some(range)` (*explicit*): the range **overrides** every member — at compile
  time each member is gated on the group's range instead of its own (can widen or
  narrow it). `Engine::compile` applies this via `SourcePresentation::member_overrides`
  (clone the member, substitute its `frames`, then resolve). The props panel shows
  the values plus a `PropertyKind::Note` warning.

In the editor: entering a first/last-frame value materialises an explicit range
(seeded from the derived union); blanking either field reverts to auto. Because
the stored range is now optional, `state::scene_object_frame_range[_mut]` return
`Option<&[mut] FrameRange>` (None for an auto group) and frame insert/delete skip
auto groups (their members shift instead).

## Module Map

| Path | Role |
|------|------|
| `src/main.rs` | CLI entry point |
| `src/types.rs` | Shared types: `Color`, `Style`, `Cell`, `DrawOp`, `Frame`, `PlayablePresentation`, `CommandRegion`, `LoopRegion` |
| `src/engine/source.rs` | `SourcePresentation` (+ `command_regions()`, `loop_regions()`, `animation_regions()`, `validate_loops()`, `link_siblings()`, and a `links` sidecar — editor-only families of object indices for *linked* paste, ignored by the engine), `SceneObject`, `Coordinate` (Fixed/Animated), `FrameRange` |
| `src/engine/objects/` | Thirteen `SceneObject` types: `Label`, `HLine`, `Rect`, `Header`, `Group`, `Arrow`, `Table`, `Art`, `Command`, `List`, `Loop`, `Morph`, `Animation` — each implements `Resolve`. See the module-doc checklist in `mod.rs` for every site a new type touches. `List` (ordered/unordered) shares `Label`'s text-editing UX and the shared `wrap` helper. `Loop` (like `Group`) draws nothing; its `frames` range is the loop range and it emits a `LoopRegion` sidecar. `Morph` blends two inline ASCII-art grids (`from`→`to`) across its `frames` range — each cell flips to the `to` glyph once playback progress passes that cell's per-cell threshold (`MorphMode`: `dissolve` or four directional wipes). Fully baked into static frames in `resolve`, so the editor preview shows it for free. `Animation` (also draws nothing) marks an auto-play *span* and emits an `AnimationRegion` sidecar; it is created by the animate sub-menu, not the Add-Object menu (the only type absent from `OBJECT_TYPES`) |
| `src/art_library.rs` | Built-in + user ASCII-art palette (`~/.config/bs/art/`, one file per piece); pieces are copied into self-contained `Art` objects when added. Includes a matched `ball`/`square` pair used as the default `Morph` endpoints. The picker (`Mode::AddArt`/`LoadArtFile`) carries an `ArtPick` purpose so the same flow serves a standalone `Art` or the two-stage `from`/`to` pick of a `Morph` |
| `src/renderer/mod.rs` | Rasterizes DrawOps into cell grid; diffs frames |
| `src/player/mod.rs` | Playback loop, keyboard nav (arrows, space, q, f=fullscreen); runs `Command` objects (piped, async, timeout) and overlays output; drives `Loop` regions (timer-based auto-advance + bounce + arrow-key break-out) via the pure `loop_next` step fn; auto-advances across auto-play `Animation` spans (`auto_deadline`), using `auto_advance_delay` = the **min** `delay_ms` over the animations covering each boundary, with the loop's own delay as the fallback for gaps inside a loop |
| `src/editor/mod.rs` | Editor lifecycle, raw mode setup, main loop |
| `src/editor/state.rs` | `EditorState` (incl. `clipboard` + `clipboard_sources` for copy/paste), `Mode` enum (~23 variants, incl. table sub-modes, art picker, frame sub-menu/move/overlay, `MultiSelect`, `PastePlacing`). Frame ops: `insert_blank_frame`, `copy_frame` (deep-clone duplicate into a *new* frame), `overlay_frame` (deep-clone paste onto an *existing* frame, no new frame), `move_frame`. Copy/paste helpers: `expand_selection` (pull in a group's members), `clone_selection` (self-contained deep clone with selection-local member remap). Object delete fixes both `Group.members` and `links` families (`adjust_group_members_after_delete`) as objects are pruned |
| `src/editor/config.rs` | `KeyBindings` — all bindings configurable via `~/.config/bs/editor.json` |
| `src/editor/input.rs` | All key event handling. Property browse/edit/dropdown flows (object + table cell-style) share helpers: `TextEdit` (text fields), `dropdown_key`/`DropdownKey` (list nav), and the `ep_*` `Mode::EditProperties` constructors |
| `src/editor/textedit.rs` | `TextEdit` — reusable text-buffer + cursor used by every text field (property values, the multi-line overlay, cell-style values); translates key events into edits (insert/delete/arrows/home-end/newline) |
| `src/editor/panel.rs` | Left panel (Add Object), right panel (Properties incl. `Bool` checkboxes + colour swatches), object selection overlay, and the centred multi-line text-editing overlay (`render_text_overlay`). Every text field draws its caret through one shared helper, `draw_caret_line` (see "Text caret convention" below) |
| `src/editor/properties.rs` | `Editable` trait — one impl per object type holds its property list, setter, coordinate + geometry accessors; generic dispatch (`get_properties`, `set_property`, …) is type-agnostic. `PropertyKind::Bool` flags toggle in place (Space/Enter); `PropertyKind::Note` renders a non-editable free-form warning line (the whole `value`, no `name:`) — the mechanism for surfacing per-object warnings in the panel |
| `src/editor/preview.rs` | Canvas preview using Engine+Renderer |
| `src/editor/timeline.rs` | Frame bar and status line. Frames under an auto-play `Animation` collapse into a single range cell (`[10-20]`); strictly-overlapping auto-play spans merge into one range (continuous auto-advance), adjacent-but-disjoint ones stay separate |
| `src/editor/menubar.rs` | Context-sensitive menu bar |
| `src/editor/ui.rs` | Layout computation |

## Editor Mode FSM

```
Normal ──a──→ AddObject ──Enter──→ Normal (object added)
       ──e──→ SelectObject ──Enter──→ EditProperties ──a──→ AnimateProperty
       ──d──→ (delete selected)
```

- **Normal**: frame navigation (←/→), `f` opens the frame sub-menu, g presentation settings (frame size), Ctrl-s save, q quit, Shift+F fullscreen
- **FrameMenu**: frame operations — `a` add blank frame, `c` copy (duplicate) current frame, `o` overlay (paste) current frame's objects onto another existing frame, `d` delete current frame (with confirm), `m` move current frame, Esc back. `add` calls `state::insert_blank_frame` (the "make room" primitive — a new empty frame). `copy` calls `state::copy_frame`, which inserts a blank frame and then **deep-clones** every object on the source frame onto it, so the copy's objects are independent of the original (editing one never changes the other). Deck-wide/spanning objects stay shared (extended across the new frame) rather than cloned, so they remain a single continuous object
- **FrameOverlay**: paste the current (source) frame's objects *on top of* another existing frame, **without** inserting a new frame. ←/→ scroll the deck to a target frame; Enter calls `state::overlay_frame`, which **deep-clones** every object on the source frame onto the target (same positions/styles/z-order), appended after the target's existing objects so they render over it. Objects already visible on the target (e.g. a deck-wide background spanning both frames) are skipped rather than duplicated. Unlike copy/move, the deck's `frame_count` is unchanged
- **FrameMove → FrameMovePlace**: relocate the current slide. In FrameMove, ←/→ scroll the deck to a target slide; Enter opens FrameMovePlace, where Enter drops the moved slide *after* the target and `b` drops it *before* (`state::move_frame` remaps object ranges through the new frame ordering)
- **Settings**: edit the output frame size (width × height in cells); ↑↓/Tab switch field, Enter apply, Esc cancel
- **AddObject**: choose object type from the list (↑/↓ + Enter) or press its **quick-add shortcut** — one unique letter per type, shown as `[l] Label` and defined by `object_defaults::OBJECT_TYPE_KEYS` (`object_type_for_key` maps a keypress to the type). Either path runs the shared `commit_add_object`. After committing, most types land in `EditProperties` (browse); `Group`/`Art` enter their member/library pickers; `Morph` runs the art-library picker **twice** (pick the `from` piece, then the `to` piece) before landing in `EditProperties`; `Label` and `List` jump straight into the centred multi-line text overlay (empty buffer) so you can type content immediately — Esc keeps the default text, Enter commits
- **SelectObject**: pick object visible on current frame
- **Copy/paste** (`c` copy, `v` paste, configurable): **copy** captures objects to `EditorState.clipboard` as self-contained deep clones — either one object (`c` in `SelectedObject`) or a `MultiSelect{Copy}` toggle set (`c` in Normal); a copied `Group` pulls in its members (`expand_selection`). **Paste** (`v`) enters `PastePlacing`: clones land on the current frame (re-anchored to it, animated coordinates flattened to `Fixed` at that frame via `state::flatten_coordinates` so the copy is static and arrow-nudgeable, then nudged off the source) as a movable **ghost** that rides the arrow keys; **Enter** drops the set and re-arms a fresh ghost (rubber-stamp loop — stamp N copies), **Esc** discards the un-dropped ghost and finishes. `l` toggles **Independent** vs **Linked**: a *linked* paste records one `links` family **per clipboard object** (its source + each stamp's clone of it), so editing a non-placement property of any member propagates to its siblings (`apply_property` → `SourcePresentation::link_siblings`; placement = `x/y/width/height/first_frame/last_frame/z_order` stays per-copy). Distinct objects copied together never cross-sync. The ghost clones live in `objects` (tail indices in `pending`), so the WYSIWYG preview shows them; Esc truncates that tail
- **SelectedObject**: move (arrows), `r` → resize mode, `e` → edit props, `d` delete; Shift+arrows also grow
- **ResizeObject**: arrow-key resize (←→ width, ↑↓ height) — a terminal-robust path since many terminals capture Shift+↑/↓ for scrollback; Enter/Esc exit
- **EditProperties**: edit typed properties; color fields show dropdown; text fields support multi-line (Alt-Enter = newline); property list scrolls vertically
- **AnimateProperty**: eight fields — `from`/`to`/`start`/`end` (the coordinate
  animation), `add frames`/`auto play` (toggles, Space/Enter), `delay ms`, and
  `gap frames`. `[s]` applies via `input::apply_animation`: optionally inserts the
  spanned frames + shares the current frame's elements (`state::add_frames_and_share`),
  sets the `Coordinate::Animated`, keeps the object's own range in lock-step
  (`scene_object_animation_span`), and creates/updates the `Animation` span
  entity (`state::upsert_animation`, reuse-by-span so X+Y stay one animation).
  `gap frames` > 1 then strobes the element via `state::apply_gap`: it shows only
  on every Nth frame of the span (single-frame samples at the interpolated
  position, empty gaps between — a stop-motion look). Gap clones are independent
  objects, so it runs only on a freshly-created span (same `!span_exists` guard as
  add-frames). `[x]` reverts the coordinate to `Fixed`. Defaults: add-frames on,
  auto-play on, 500 ms, gap 1 (off). Re-animating a span reseeds its auto-play
  settings (`enter_animate`)

## Text caret convention

The editor separates two distinct concepts that used to look alike:

- **Reverse video = "this row/field is active/selected."** Used for selected
  list rows, the highlighted property row, the active input field, and the
  timeline's current frame. Unchanged.
- **Underline = the text insertion caret.** It marks the gap *before* the char
  at that column — the next keystroke lands there and pushes the rest right
  (insert-before; never overwrite). At end-of-text it underlines the trailing
  append slot. Drawn only via `panel.rs::draw_caret_line(stdout, x, y, display,
  caret, reverse, width)`, which rasterizes one pre-composed line and composes
  the two attributes (an active field still shows its caret).

The text-edit model (`editor/textedit.rs::TextEdit`) is a gap buffer (cursor is a
char index in `0..=len`, `insert_char` inserts at the cursor and advances) — the
underline render just makes the picture match that model. `TextEdit` stays
render-agnostic by design; callers lay out the line (prefix, horizontal scroll)
and pass `display` + the caret column to `draw_caret_line`. Short single-line
dialogs (load-art-file, table column number) don't horizontally scroll, so a
caret past `width` scrolls off — acceptable since those fields are short.

## Key Data Structures

```rust
// Coordinate supports linear-interpolated animation.
// Fixed is f64 (group-scaling uses fractional precision); evaluate() floors it.
enum Coordinate {
    Fixed(f64),
    Animated { from: u16, to: u16, start_frame: usize, end_frame: usize },
}

// EditProperties carries full editing state
Mode::EditProperties {
    object_index: usize,
    selected_property: usize,
    editing_value: Option<String>,
    cursor: usize,
    scroll: usize,       // horizontal scroll within cursor's line
    panel_scroll: usize, // vertical scroll of the property list
    dropdown: Option<usize>,
}
```

## Source File Format (praesi.json)

```json
{
  "width": 80, "height": 24, "frame_count": 8,
  "objects": [
    {
      "type": "label",
      "text": "Hello",
      "position": {
        "x": { "fixed": 10 },
        "y": { "animated": { "from": 2, "to": 8, "start_frame": 0, "end_frame": 4 } }
      },
      "style": { "fg": "red", "bold": true },
      "frames": { "start": 0, "end": 8 },
      "z_order": 1
    }
  ]
}
```

- `style` is optional (omit for defaults)
- `frames.end` is exclusive
- Color values: named strings (`"black"`, `"red"`, `"green"`, `"yellow"`, `"blue"`,
  `"magenta"`, `"cyan"`, `"white"` — the 8 `NamedColor`s only) or `{ "rgb": [r, g, b] }`
- Many numeric fields (widths, heights, `hline` endpoints) accept either a bare
  number or a `Coordinate` object, via `deserialize_coord_compat`
- A `loop` object has no geometry — just a range plus playback options
  (`delay_ms` default 500, `count` default 0 = forever, `bounce` default true):

  ```json
  { "type": "loop", "frames": { "start": 4, "end": 8 },
    "delay_ms": 500, "count": 0, "bounce": true }
  ```

- An `animation` object likewise has no geometry — just a range plus auto-play
  config (`auto_play` default true, `delay_ms` default 500). It records the span
  an animated coordinate plays over; the motion itself lives on the objects'
  `Coordinate::Animated` fields:

  ```json
  { "type": "animation", "frames": { "start": 0, "end": 5 },
    "auto_play": true, "delay_ms": 500 }
  ```

## Dependencies

- `crossterm 0.28` — terminal raw mode, colors, cursor, events
- `serde` / `serde_json` — JSON serialization
- `anyhow` — error handling
- `serde_json` is also a dev-dependency (integration tests author presentations as JSON)

## Tests

See `TESTS.md` for a per-file list of every test case.

Integration tests live in `tests/` (the editor/TUI is not unit-tested; coverage
targets the pure, deterministic core):

| File | Covers |
|------|--------|
| `tests/common/mod.rs` | Helpers: `render_json` (run a JSON presentation through `Engine::compile` + `Renderer::render`), `frame_lines` / `char_at` (reconstruct the visible char grid by replaying the full frame + diffs) |
| `tests/units.rs` | `Coordinate::evaluate` (fixed flooring, animation interpolation/clamping), `FrameRange` exclusivity, the number-or-object coordinate deserializer |
| `tests/pipeline.rs` | End-to-end: label placement, full-vs-diff frames, animation moving + clearing cells, z-order, exclusive frame ranges, off-grid clipping |
| `tests/table.rs` | Table layout math, `normalize_cells`, add/remove column rescaling, border/borderless/header rendering, height padding, `col_pixel_range` |
| `tests/art.rs` | `Art` object: per-line placement, positioning, and space-transparency |
| `tests/list.rs` | `List` object: ordered/unordered markers, custom bullet, default vs custom inter-item spacing, wrapped-row indentation, multi-digit alignment, height clip, background fill |
| `tests/command.rs` | `Command` object: compiled `CommandRegion` spec, the placeholder box drawn into the static frame, and `player::layout_output` (ANSI-strip + tail + clip). The spawn/timeout run-loop is TUI and stays manual |
| `tests/label.rs` | `Label`: `framed` border, `frame_style`, background fill + height pad, height clip, width wrap |
| `tests/arrow.rs` | `Arrow`: horizontal/vertical/leftward body + auto head, diagonal L-routing, head-disabled, zero-length point |
| `tests/hline.rs` | `HLine`: span (end-exclusive) and custom draw char |
| `tests/header.rs` | `Header`: glyph fill, custom fill char, inter-glyph spacing, canvas-width word wrap |
| `tests/rect.rs` | `Rect`: border + blank interior, title on the top edge |
| `tests/group.rs` | `Group`: members render independently / group emits nothing; auto range doesn't gate members; explicit range overrides members (narrows + widens) |
| `tests/looping.rs` | `Loop`: compiled `LoopRegion` sidecar (defaults + explicit fields) and `validate_loops` (disjoint OK; overlap/nesting/past-end/empty rejected). The auto-advance run-loop is TUI; the pure `loop_next` step fn is tested inline in `player/mod.rs` |
| `tests/animation.rs` | `Animation`: compiled `AnimationRegion` sidecar (defaults + explicit) and the loop/animation rules in `validate_loops` (animations may overlap; a loop must contain a whole animation or none of it — bisecting is rejected). The auto-advance/min-delay run-loop is TUI; the pure `auto_advance_delay` is tested inline in `player/mod.rs` |
| `tests/morph.rs` | `Morph`: end-to-end blend — `from` on the first frame / `to` on the last, `wipe-right` half-done at the midpoint, smaller grid padded with transparent space, hidden outside its range. The per-cell threshold/progress fns are tested inline in `engine/objects/morph.rs` |
| `tests/engine.rs` | `Engine::compile`: one scene per frame, empty deck, object outside `frame_count` |
| `tests/renderer.rs` | Renderer + `grid_at`: equal-z-order source order, clamp past end, out-of-bounds diff skip |

Inline unit tests also live in `src/` (e.g. `editor/properties.rs`,
`engine/objects/wrap.rs`, `editor/textedit.rs`, `editor/object_defaults.rs`,
`editor/state.rs` — frame copy/blank-insert/move/delete + `add_frames_and_share`
+ `upsert_animation`, `player/mod.rs` — `loop_next` bounce/wrap stepping +
`auto_advance_delay` min-over-overlap; copy/paste `expand_selection` +
`clone_selection` + `link_siblings` + link-family delete maintenance). The suite
totals 161 tests (95 integration
+ 66 inline); `TESTS.md` is the authoritative per-test list.

Pattern: write a presentation in the documented JSON format, render it, and
assert on the reconstructed grid — so tests pin behavior without coupling to the
editor. Expected geometry is hand-derived from the layout spec, not snapshotted.

## Status & known issues

Recent work: object property handling is now a single `Editable` trait with one
impl per type (was ~64 per-type match arms across ~8 functions) — adding a
property touches only that type's impl. Table fixes: `Table.height` now pads
short tables (never clips taller content) via a shared `Table::row_heights`;
`col_pixel_range` now includes the column's border columns per its doc.

Menu/property UX overhaul: `PropertyKind::Bool` renders as a checkbox and toggles
in place on Space/Enter (no text detour); `Text` values edit in a centred
multi-line overlay over the canvas instead of the cramped ~21-col panel field;
colour rows/dropdowns show a swatch. The three `handle_table_cell_style_*`
handlers no longer duplicate the object-property flow — text editing goes through
the shared `TextEdit` buffer (`textedit.rs`), dropdown navigation through
`dropdown_key`, and `Mode::EditProperties` is built via the `ep_*` constructors
(which also fixed browse-mode scroll not following the selection).

Recent maintainability work (from a code review):

- A "how to add an object type" checklist now lives in the module doc of
  `src/engine/objects/mod.rs`, enumerating every touch site (the compiler only
  catches some).
- Word-wrap is no longer duplicated: `label.rs` and `table.rs` both call the
  shared `engine::objects::wrap` helper (`wrap_line_indexed` + `indexed_to_chars`),
  so the glyphs and their source indices can't drift.
- Frame replay is unified in `PlayablePresentation::grid_at` (`types.rs`); the
  player (`rebuild_grid`), the editor preview, and the test harness
  (`frame_lines`) all go through it instead of re-implementing diff replay.
- Text-caret rendering is unified in `panel.rs::draw_caret_line`; all nine text
  fields (Settings, load-art-file, AnimateProperty, table add/remove-column,
  table cell content + cell-style, the property-panel inline editor, and the
  `render_text_overlay` text box) call it instead of each open-coding a caret.
  This replaced three divergent styles (reverse-video block, a spliced `█`
  glyph, a bold char on a reversed line). See "Text caret convention" below.

Outstanding maintainability work (from a code review; not yet done):

- The `Mode` FSM (~16 variants, some with 7–15 fields) grows with every object type.
- `panel.rs::render_right_panel` is one ~900-line function covering 12 modes.
  The caret rendering is now shared (`draw_caret_line`), but the list-row and
  dropdown render patterns are still duplicated inline.
- The nine `Editable` impls repeat near-identical `set()` arms and geometry
  accessors for the common x/y/width/height/style/frame fields.
