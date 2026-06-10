# ascii-presenter

A terminal-native presentation engine written in Rust. Presentations are
JSON-described ASCII-art animations that render in the terminal.

## Install the toolchain

You need a Rust toolchain and a C linker (Rust uses the system `cc` to link).

**Standard machine:**

```bash
# Rust (installs rustc + cargo via rustup)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"

# C linker
sudo apt-get install -y build-essential     # Debian/Ubuntu
# macOS: xcode-select --install
```

**Restricted environment (no root, no system linker):** install Rust with
`rustup` as above, then fetch a C toolchain into a local prefix without root:

```bash
mkdir -p /tmp/tc && cd /tmp/tc
apt-get download gcc-13 cpp-13 gcc-13-x86-64-linux-gnu cpp-13-x86-64-linux-gnu \
  binutils binutils-x86-64-linux-gnu binutils-common libbinutils \
  libc6-dev libc-dev-bin linux-libc-dev libgcc-13-dev libcrypt-dev \
  libisl23 libmpc3 libmpfr6 libgmp10 zlib1g libsframe1 libctf0
for d in *.deb; do dpkg-deb -x "$d" "$HOME/toolchain"; done
```

Then create `~/toolchain/bin/cc` wrapping `x86_64-linux-gnu-gcc-13` with `-B`
paths into the extracted prefix and `LD_LIBRARY_PATH` set to
`~/toolchain/usr/lib/x86_64-linux-gnu`, and point cargo at it:

```bash
export PATH="$HOME/toolchain/bin:$PATH"
export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER="$HOME/toolchain/bin/cc"
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

- Object types: `label`, `hline`, `rect`, `header`, `group`, `arrow`, `table`
- `style` is optional; `frames.end` is exclusive
- Colors: named (`black`, `red`, `green`, `yellow`, `blue`, `magenta`, `cyan`,
  `white`) or `{ "rgb": [r, g, b] }`
