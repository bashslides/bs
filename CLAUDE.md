# ascii-presenter

A terminal-native presentation engine written in Rust. Presentations are ASCII art animations that render in the terminal.

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
| `src/engine/objects/` | Four object types: `Label`, `HLine`, `Rect`, `Header` — each implements `Resolve` |
| `src/renderer/mod.rs` | Rasterizes DrawOps into cell grid; diffs frames |
| `src/player/mod.rs` | Playback loop, keyboard nav (arrows, space, q, F11) |
| `src/editor/mod.rs` | Editor lifecycle, raw mode setup, main loop |
| `src/editor/state.rs` | `EditorState`, `Mode` enum (5 states) |
| `src/editor/config.rs` | `KeyBindings` — all bindings configurable via `~/.config/ascii-presenter/editor.json` |
| `src/editor/input.rs` | All key event handling (~850 lines) |
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
// Coordinate supports linear-interpolated animation
enum Coordinate {
    Fixed(u16),
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
- Color values: named strings (`"red"`, `"bright_blue"`, etc.) or `{ "rgb": [r, g, b] }`

## Dependencies

- `crossterm 0.28` — terminal raw mode, colors, cursor, events
- `serde` / `serde_json` — JSON serialization
- `anyhow` — error handling
