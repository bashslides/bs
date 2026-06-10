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

## Module Map

| Path | Role |
|------|------|
| `src/main.rs` | CLI entry point |
| `src/types.rs` | Shared types: `Color`, `Style`, `Cell`, `DrawOp`, `Frame`, `PlayablePresentation` |
| `src/engine/source.rs` | `SourcePresentation`, `SceneObject`, `Coordinate` (Fixed/Animated), `FrameRange` |
| `src/engine/objects/` | Seven object types: `Label`, `HLine`, `Rect`, `Header`, `Group`, `Arrow`, `Table` — each implements `Resolve` |
| `src/renderer/mod.rs` | Rasterizes DrawOps into cell grid; diffs frames |
| `src/player/mod.rs` | Playback loop, keyboard nav (arrows, space, q, F11) |
| `src/editor/mod.rs` | Editor lifecycle, raw mode setup, main loop |
| `src/editor/state.rs` | `EditorState`, `Mode` enum (~16 variants, incl. table sub-modes) |
| `src/editor/config.rs` | `KeyBindings` — all bindings configurable via `~/.config/bs/editor.json` |
| `src/editor/input.rs` | All key event handling (~2,057 lines — monolithic; ~34% is table handlers) |
| `src/editor/panel.rs` | Left panel (Add Object), right panel (Properties), object selection overlay |
| `src/editor/properties.rs` | Type-aware property getter/setter for all object types |
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

- **Normal**: frame navigation (←/→), +/- add/remove frames, Ctrl-s save, q quit
- **AddObject**: choose object type from list
- **SelectObject**: pick object visible on current frame
- **EditProperties**: edit typed properties; color fields show dropdown; text fields support multi-line (Alt-Enter = newline); property list scrolls vertically
- **AnimateProperty**: set from/to/start_frame/end_frame for coordinate animation

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

Integration tests live in `tests/` (the editor/TUI is not unit-tested; coverage
targets the pure, deterministic core):

| File | Covers |
|------|--------|
| `tests/common/mod.rs` | Helpers: `render_json` (run a JSON presentation through `Engine::compile` + `Renderer::render`), `frame_lines` / `char_at` (reconstruct the visible char grid by replaying the full frame + diffs) |
| `tests/units.rs` | `Coordinate::evaluate` (fixed flooring, animation interpolation/clamping), `FrameRange` exclusivity, the number-or-object coordinate deserializer |
| `tests/pipeline.rs` | End-to-end: label placement, full-vs-diff frames, animation moving + clearing cells, z-order, exclusive frame ranges, off-grid clipping |
| `tests/table.rs` | Table layout math, `normalize_cells`, add/remove column rescaling, border/borderless/header rendering, height padding, `col_pixel_range` |

Pattern: write a presentation in the documented JSON format, render it, and
assert on the reconstructed grid — so tests pin behavior without coupling to the
editor. Expected geometry is hand-derived from the layout spec, not snapshotted.

## Status & known issues

Recent fixes (table): `Table.height` now pads short tables (never clips taller
content) via a shared `Table::row_heights`; `col_pixel_range` now includes the
column's border columns per its doc.

Outstanding maintainability work (from a code review; not yet done):

- `editor/input.rs` is a 2,057-line monolith with three `handle_table_cell_style_*`
  handlers that duplicate the generic `EditProperties` navigation/dropdown/edit logic.
- `editor/properties.rs` exposes object properties via ~64 per-type match arms across
  ~8 functions; adding one property touches 6+ places. A trait-based property schema
  would collapse this.
- The `Mode` FSM (~16 variants, some with 7–15 fields) grows with every object type.
- No "how to add an object type" checklist exists; word-wrap is duplicated between
  `label.rs` and `table.rs`.
