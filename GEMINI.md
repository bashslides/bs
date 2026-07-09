# GEMINI.md

This file exists so Gemini / Vertex agents find their entry point. To avoid
copies of the same guidance drifting apart, the actual instructions live in one
place:

➡️ **Read [`AGENTS.md`](AGENTS.md) first** — it is the canonical, vendor-neutral
quickstart (hard rules, exact build/test commands, architecture, and a "where to
look" map). From there, [`docs/AI_MAINTAINABILITY.md`](docs/AI_MAINTAINABILITY.md)
is the compact architecture companion, and [`CLAUDE.md`](CLAUDE.md) is the full
module-by-module reference.

Do not duplicate onboarding content here — update `AGENTS.md` instead.

## The non-negotiables (also in AGENTS.md, repeated so you can't miss them)

- **Never** run `git commit` / `git push` / `git add` or any history-mutating
  git command — the harness commits automatically; just leave edits in the tree.
- Keep `examples/hello.rs` compiling (`cargo test` builds it).
- Don't launch the interactive `edit`/`play` TUIs in a headless run — they need
  a real terminal and will hang. Use `cargo test` and the `compile` verb instead.
- Build/test:

  ```bash
  source "$HOME/.cargo/env"
  export PATH="$HOME/toolchain/bin:$PATH"
  export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER="$HOME/toolchain/bin/cc"
  cargo test
  ```
