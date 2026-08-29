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

## Web viewer (GitHub Pages)

`web/` is a dependency-free static page that plays a **compiled** presentation
in the browser — black-and-white terminal chrome, the deck's own colours intact.
It is a port of the player's pure logic (frame replay, loops, auto-play
animations, auto-advance markers), so playback matches the terminal; the one
thing it cannot do is run `Command` objects, whose placeholder box the compiler
has already baked into the frames.

```bash
cargo run -- compile my-talk.json web/presentation.json   # the deck it serves
python3 -m http.server -d web 8000                        # http://localhost:8000
```

A deck can also be dropped onto the page, opened with `o`, or passed as
`?deck=<url>`. `.github/workflows/pages.yml` publishes `web/` to GitHub Pages on
every push to `main` that touches it; it creates the Pages site itself
(`configure-pages` with `enablement: true`), so no Settings click is needed. See
`web/README.md` for the keys and the Rust→JS mapping.

## For AI coding agents

Start with `AGENTS.md` — the vendor-neutral quickstart (hard rules, exact
build/test commands, architecture, and a "where to look" map). It points on to
`docs/AI_MAINTAINABILITY.md` (compact architecture + invariants), `CLAUDE.md`
(the full reference), and `PRESENTATION_FORMAT.md` (source-format spec). Gemini
/ Vertex agents are routed to the same content via `GEMINI.md`.

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
        "y": { "animated": { "from": 2, "to": 8, "anim": 1 } }
      },
      "style": { "fg": "red", "bold": true },
      "frames": { "start": 0, "end": 8 },
      "z_order": 1
    },
    { "type": "animation", "id": 1, "frames": { "start": 0, "end": 4 } }
  ]
}
```

- Object types (JSON `type` tag): `label`, `h_line`, `rect`, `header`, `group`,
  `arrow`, `table`, `art`, `command`, `list`, `loop`, `morph`, `animation`,
  `auto_advance`, `circle` (15 total)
- An animated coordinate references an `animation` object by `anim` id; the span
  lives only on that object (its `frames`), never on the coordinate. See
  `PRESENTATION_FORMAT.md` for the full format.
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
