# bs

A terminal-native presentation engine written in Rust. Presentations are
JSON-described ASCII-art animations that render in the terminal.

## Install the toolchain

`bs` needs a Rust toolchain and a C linker. The bundled script sets up both —
it installs Rust if missing, and provides a C linker (system `build-essential`
when you have root, otherwise a self-contained gcc unpacked into `~/toolchain`
with no root required):

```bash
./scripts/install-toolchain.sh
```

If it set up the local (no-root) toolchain, load it into your shell before
building:

```bash
source ~/toolchain/env.sh
```

On a normal machine you can also just do it by hand:

```bash
curl https://sh.rustup.rs -sSf | sh        # Rust (rustc + cargo)
sudo apt-get install -y build-essential    # C linker — macOS: xcode-select --install
```

## Build, run, test

```bash
cargo build
cargo test                                   # full suite (also builds examples)

cargo run -- compile source.json out.json    # compile source → playable
cargo run -- edit source.json                # interactive editor
cargo run -- play out.json                   # play a compiled presentation
cargo run --example hello                     # minimal programmatic example
```

## How it works

A three-stage pipeline with clean separation:

```
SourcePresentation (JSON)
  → Engine::compile()   → Vec<ResolvedScene>   (DrawOps per frame)
  → Renderer::render()  → PlayablePresentation (Frame::Full / Frame::Diff)
  → Player::play()      → terminal output
```

The interactive editor runs the same Engine + Renderer pipeline live for a
WYSIWYG preview. See `CLAUDE.md` for the full architecture and module map.

## Source format

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

- Object types (JSON `type` tag): `label`, `h_line`, `rect`, `header`, `group`,
  `arrow`, `table`, `art`
- `style` is optional; `frames.end` is exclusive
- Colors: named (`black`, `red`, `green`, `yellow`, `blue`, `magenta`, `cyan`,
  `white`) or `{ "rgb": [r, g, b] }`

### ASCII-art pieces

The `art` object embeds a pre-made ASCII-art drawing. In the editor, **add →
Art** opens a palette of built-in pieces (`human`, `ghost`, `tree`) plus any
files you drop in `~/.config/bs/art/` (one piece per file, the file stem is its
name). The palette's **Load from file…** entry imports an art file by path at
runtime. Whatever you pick is copied into the object, so saved presentations
never depend on the library afterwards.
