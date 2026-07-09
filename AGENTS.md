# AGENTS.md — start here (all AI agents)

Vendor-neutral onboarding for AI coding agents (Codex/OpenAI, Gemini/Vertex,
Claude, Cursor, and others). This is the shared "read me first." It is
intentionally short and points at deeper docs rather than duplicating them, so
the docs can't drift.

> **How the agent docs fit together.** Different tools look for different
> filenames, so this project keeps a small, deliberate set:
>
> - **`AGENTS.md`** (this file) — the canonical quickstart. Codex, Cursor, and
>   most agents read this by convention. There is intentionally no separate
>   `CODEX.md`: Codex reads `AGENTS.md`.
> - **`GEMINI.md`** — a thin pointer here, for the file Gemini CLI reads. Vertex
>   AI serves Gemini-family models, so Vertex agents use `GEMINI.md` too (there
>   is no separate `VERTEX.md`).
> - **`docs/AI_MAINTAINABILITY.md`** — a compact architecture + invariants +
>   change-playbook companion; read it before the long `CLAUDE.md`.
> - **`CLAUDE.md`** — the full, module-by-module architecture reference Claude
>   loads automatically.
>
> If you ever change the hard rules or the build/test commands below, mirror the
> change in `CLAUDE.md`.

## What this project is

`bs` is a terminal-native presentation engine in Rust. Presentations are
JSON-described ASCII-art animations that render in the terminal. It ships four
CLI verbs (`compile`, `edit`, `play`, `migrate`) — two of them (`edit`, `play`)
are interactive TUIs — over an Engine → Renderer → Player pipeline (see
[`README.md`](README.md)).

## Hard rules (do not break)

1. **Never run history-mutating git.** No `git commit`, `git push`, `git add`,
   `git rebase`, `git reset --hard`, etc. The harness commits automatically —
   just edit files and leave them in the working tree.
2. **Keep `examples/hello.rs` compiling.** `cargo test` builds it, so any change
   to the public object structs must be reflected there.
3. **Don't run the interactive `edit`/`play` TUIs in an automated run** — they
   need a real TTY and will hang. Exercise behavior through the test suite and
   the `compile` verb instead.

## Build & test (copy-paste)

`cargo` is under `~/.cargo`; Rust needs a C linker. On a sandbox without root a
local gcc is provided at `~/toolchain`. Reproducible invocation:

```bash
source "$HOME/.cargo/env"
export PATH="$HOME/toolchain/bin:$PATH"
export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER="$HOME/toolchain/bin/cc"
cargo test          # full suite; also builds examples/hello.rs
```

On a normal machine with `cargo` + `build-essential` on `PATH`, plain
`cargo build` / `cargo test` is enough. To (re)create the toolchain, run
`./scripts/install-toolchain.sh` (see [`README.md`](README.md)).

CLI:

```bash
cargo run -- compile source.json out.json   # source → playable (safe, non-interactive)
cargo run -- edit    source.json [more…]     # TUI editor  (needs a TTY — don't run headless)
cargo run -- play    out.json                # TUI player  (needs a TTY — don't run headless)
cargo run -- migrate source.json             # upgrade an old source file in place
```

## Architecture in one breath

Three-stage pipeline with clean separation (the editor runs the same
Engine+Renderer live for WYSIWYG preview):

```
SourcePresentation (JSON)
  → Engine::compile()   → Vec<ResolvedScene>   (DrawOps per frame)
  → Renderer::render()  → PlayablePresentation (Frame::Full / Frame::Diff)
  → Player::play()      → terminal output
```

Rust `edition = "2024"`. `src/lib.rs` exposes the crate; `src/main.rs` is the CLI.

## Where to look

| I want to… | Go to |
|------------|-------|
| A compact architecture map, invariants, and change playbooks | **[`docs/AI_MAINTAINABILITY.md`](docs/AI_MAINTAINABILITY.md)** (read this first) |
| Understand the whole architecture, editor FSM, every module's role | **[`CLAUDE.md`](CLAUDE.md)** (canonical, detailed) |
| **Add a new object type** | the checklist in the module doc of `src/engine/objects/mod.rs` — it enumerates *every* touch site (the compiler catches only some) |
| Author or hand-edit a `.json` presentation (source format) | **[`PRESENTATION_FORMAT.md`](PRESENTATION_FORMAT.md)** |
| See what each test covers before adding one | **[`TESTS.md`](TESTS.md)** (authoritative per-test list) |
| Build/run/install | **[`README.md`](README.md)** |

## Conventions & landmines

- **Docs are load-bearing.** `CLAUDE.md`, `docs/AI_MAINTAINABILITY.md`,
  `PRESENTATION_FORMAT.md`, and `TESTS.md` describe real behavior other agents
  rely on. If you change behavior, update the matching doc in the same edit.
  When adding tests, add the row to `TESTS.md` and keep its total count line
  accurate.
- **The 15 object types** and their JSON `type` tags (snake_case) are the enum in
  `src/engine/source.rs`: `label`, `h_line`, `rect`, `header`, `group`, `arrow`,
  `table`, `art`, `command`, `list`, `loop`, `morph`, `animation`, `auto_advance`,
  `circle`. `animation` and `auto_advance` are created via editor sub-menus, not
  the Add-Object menu, so they are absent from `OBJECT_TYPES`.
- **Animation model:** an animated coordinate is
  `{ "animated": { "from", "to", "anim": <id> } }`; the *span* lives only on the
  referenced `Animation` object (`{ "type": "animation", "id", "frames" }`) — the
  single source of truth. Never store a span on the coordinate. `src/migrate.rs`
  upgrades the old `{start_frame,end_frame}` shape.
- **Tests target the pure core**, not the TUI: author a presentation as JSON,
  run it through `Engine::compile` + `Renderer::render`, and assert on the
  reconstructed char grid (`tests/common/mod.rs` helpers). The editor/player
  run-loops stay manually tested; their pure step functions have inline unit
  tests (e.g. `player::loop_next`).
- **Prefer the shared helper over a new one:** word-wrap (`engine::objects::wrap`),
  frame replay (`PlayablePresentation::grid_at`), text carets
  (`panel.rs::draw_caret_line`), object properties (the `Editable` trait). See
  CLAUDE.md's "Status & known issues" for the consolidation already done and the
  refactors still outstanding.
