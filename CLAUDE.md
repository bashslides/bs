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

## Module Map

| Path | Role |
|------|------|
| `src/main.rs` | CLI entry point |
| `src/types.rs` | Shared types: `Color`, `Style`, `Cell`, `DrawOp`, `Frame`, `PlayablePresentation`, `CommandRegion` |
| `src/engine/source.rs` | `SourcePresentation` (+ `command_regions()`), `SceneObject`, `Coordinate` (Fixed/Animated), `FrameRange` |
| `src/engine/objects/` | Ten object types: `Label`, `HLine`, `Rect`, `Header`, `Group`, `Arrow`, `Table`, `Art`, `Command`, `List` — each implements `Resolve`. See the module-doc checklist in `mod.rs` for every site a new type touches. `List` (ordered/unordered) shares `Label`'s text-editing UX and the shared `wrap` helper |
| `src/art_library.rs` | Built-in + user ASCII-art palette (`~/.config/bs/art/`, one file per piece); pieces are copied into self-contained `Art` objects when added |
| `src/renderer/mod.rs` | Rasterizes DrawOps into cell grid; diffs frames |
| `src/player/mod.rs` | Playback loop, keyboard nav (arrows, space, q, f=fullscreen); runs `Command` objects (piped, async, timeout) and overlays output |
| `src/editor/mod.rs` | Editor lifecycle, raw mode setup, main loop |
| `src/editor/state.rs` | `EditorState`, `Mode` enum (~18 variants, incl. table sub-modes + art picker) |
| `src/editor/config.rs` | `KeyBindings` — all bindings configurable via `~/.config/bs/editor.json` |
| `src/editor/input.rs` | All key event handling. Property browse/edit/dropdown flows (object + table cell-style) share helpers: `TextEdit` (text fields), `dropdown_key`/`DropdownKey` (list nav), and the `ep_*` `Mode::EditProperties` constructors |
| `src/editor/textedit.rs` | `TextEdit` — reusable text-buffer + cursor used by every text field (property values, the multi-line overlay, cell-style values); translates key events into edits (insert/delete/arrows/home-end/newline) |
| `src/editor/panel.rs` | Left panel (Add Object), right panel (Properties incl. `Bool` checkboxes + colour swatches), object selection overlay, and the centred multi-line text-editing overlay (`render_text_overlay`). Every text field draws its caret through one shared helper, `draw_caret_line` (see "Text caret convention" below) |
| `src/editor/properties.rs` | `Editable` trait — one impl per object type holds its property list, setter, coordinate + geometry accessors; generic dispatch (`get_properties`, `set_property`, …) is type-agnostic. `PropertyKind::Bool` flags toggle in place (Space/Enter) |
| `src/editor/preview.rs` | Canvas preview using Engine+Renderer |
| `src/editor/timeline.rs` | Frame bar and status line |
| `src/editor/menubar.rs` | Context-sensitive menu bar |
| `src/editor/ui.rs` | Layout computation |

## Editor Mode FSM

```
Normal ──a──→ AddObject ──Enter──→ Normal (object added)
       ──e──→ SelectObject ──Enter──→ EditProperties ──a──→ AnimateProperty
       ──d──→ (delete selected)
```

- **Normal**: frame navigation (←/→), +/- add/remove frames, g presentation settings (frame size), Ctrl-s save, q quit
- **Settings**: edit the output frame size (width × height in cells); ↑↓/Tab switch field, Enter apply, Esc cancel
- **AddObject**: choose object type from list. After Enter, most types land in `EditProperties` (browse); `Label` and `List` jump straight into the centred multi-line text overlay (empty buffer) so you can type content immediately — Esc keeps the default text, Enter commits
- **SelectObject**: pick object visible on current frame
- **SelectedObject**: move (arrows), `r` → resize mode, `e` → edit props, `d` delete; Shift+arrows also grow
- **ResizeObject**: arrow-key resize (←→ width, ↑↓ height) — a terminal-robust path since many terminals capture Shift+↑/↓ for scrollback; Enter/Esc exit
- **EditProperties**: edit typed properties; color fields show dropdown; text fields support multi-line (Alt-Enter = newline); property list scrolls vertically
- **AnimateProperty**: set from/to/start_frame/end_frame for coordinate animation

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
| `tests/header.rs` | `Header`: glyph fill, custom fill char, inter-glyph spacing |
| `tests/rect.rs` | `Rect`: border + blank interior, title on the top edge |
| `tests/group.rs` | `Group`: members render independently / group emits nothing; group frame range doesn't gate members |
| `tests/engine.rs` | `Engine::compile`: one scene per frame, empty deck, object outside `frame_count` |
| `tests/renderer.rs` | Renderer + `grid_at`: equal-z-order source order, clamp past end, out-of-bounds diff skip |

Inline unit tests also live in `src/` (e.g. `editor/properties.rs`,
`engine/objects/wrap.rs`, `editor/textedit.rs`, `editor/object_defaults.rs`).
The suite totals 95 tests (72 integration + 23 inline); `TESTS.md` is the
authoritative per-test list.

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
