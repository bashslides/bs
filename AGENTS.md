# AGENTS.md — the one guide for AI agents working on `bs`

`bs` is a terminal-native presentation engine written in Rust (`edition =
"2024"`). Presentations are JSON-described ASCII-art animations that render in
the terminal.

> **This is the only agent-instructions file.** Every tool reads it: Codex,
> Cursor and most agents read `AGENTS.md` directly; `CLAUDE.md` and `GEMINI.md`
> are one-line `@AGENTS.md` imports so Claude Code and Gemini CLI load this same
> text. Edit guidance **here only** — never add content to those two stubs
> (`tests/docs.rs` fails if they grow beyond the import). The first sections
> are the operating rules; everything from "Architecture" on is the detailed,
> module-by-module reference.

## Hard rules (do not break)

1. **Never commit or push.** No `git commit`, `git push`, `git add`,
   `git rebase`, `git reset --hard` or any other history-mutating git command.
   The harness commits automatically — just edit files and leave them in the
   working tree.
2. **Keep `examples/hello.rs` compiling.** `cargo test` builds it, so any change
   to the public object structs must be reflected there.
3. **Don't run the interactive `edit`/`play` TUIs in an automated run** — they
   need a real TTY and will hang. Exercise behavior through the test suite and
   the `compile` verb instead.
4. **Docs are load-bearing.** If you change behavior, update the matching doc
   in the same edit (this file, `PRESENTATION_FORMAT.md`, `TESTS.md`, the
   bs-deck skill). `tests/docs.rs` checks the mechanical half — when it fails,
   fix the doc it names, don't weaken the test.

## Build & test (copy-paste)

`bs` needs Rust and a C linker. `scripts/install-toolchain.sh` sets up both
(idempotent): it installs Rust via rustup if `cargo` is missing, and uses the
system `cc`, or `build-essential` when you have root, or else unpacks a
self-contained gcc into `~/toolchain` and writes `~/toolchain/env.sh`, which
must then be sourced. So:

```bash
./scripts/install-toolchain.sh --no-wasm   # once; drop --no-wasm for the browser tools
source "$HOME/.cargo/env"
[ -f "$HOME/toolchain/env.sh" ] && source "$HOME/toolchain/env.sh"
cargo test                                 # full suite; also builds examples/hello.rs
```

Only source `~/toolchain/env.sh` when it exists: it points cargo's linker at
`~/toolchain/bin/cc`, which is absent on a machine with a system `cc`. On a
normal machine with `cargo` + `build-essential` on `PATH`, plain `cargo test`
is enough. Docs-only change? `cargo test --test docs` runs just the doc↔code
sync checks. Engine wasm-cleanliness: `cargo build --no-default-features`.

Web tools (`web/`, deployed to GitHub Pages — never hand-assemble the site):

```bash
./scripts/build-web.sh --serve   # engine → wasm, assemble _site/, serve on :8000
./scripts/test-web.sh            # headless end-to-end check (needs firefox)
```

## CLI

```bash
cargo run -- compile source.json out.json   # compile source → playable (safe, non-interactive)
cargo run -- edit source.json [more.json …] # interactive editor, one or more decks (needs a TTY)
cargo run -- play out.json                  # play compiled presentation (needs a TTY)
cargo run -- migrate source.json            # upgrade an old source file in place (writes source.json.bak)
```

`bs edit` accepts **multiple files** — each opens as a parallel *deck*. Switch
between them, open more, and copy frames across decks from the **[p]resentations**
hub (see "Multiple presentations" below).

## Where to look

| I want to… | Go to |
|------------|-------|
| **Add a new object type** | the checklist in the module doc of `src/engine/objects/mod.rs` — it enumerates *every* touch site (the compiler catches only some) |
| Author or hand-edit a `.json` presentation (source format) | `PRESENTATION_FORMAT.md`, or the packaged Claude skill at `.claude/skills/bs-deck/SKILL.md` (same rules, condensed) |
| See what each test covers before adding one | `TESTS.md` (authoritative per-test list, enforced by `tests/docs.rs`) |
| Find the module that owns something | the "Module Map" below |
| Build/run/install as a user | `README.md` |
| The browser tools (title / instructions / compile / present) | `web/README.md` and "Web tools" below |

## Conventions & landmines

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
- **Keep the engine wasm-clean.** `src/compile.rs`, `src/engine/`,
  `src/renderer/` and `src/types.rs` must stay free of `std::fs`, `std::env`,
  `std::time`, threads and `crossterm` — they compile to
  `wasm32-unknown-unknown` for the browser compiler. Anything terminal-shaped
  belongs behind the `tui` feature (see "Feature gating" below).
- **The web viewer is a JS port of the player.** Changing the player's pure
  logic in `src/player/mod.rs` or `Cell::printable` in `src/types.rs` means
  mirroring it in `web/viewer.js` in the same edit — `cargo test` cannot catch
  that drift (see "Web tools" below).
- **Prefer the shared helper over a new one** — see "Shared helpers" below.

## Core invariants

- `FrameRange.end` is exclusive.
- Coordinates render in terminal cells; fixed floats are floored.
- Higher `z_order` draws on top. Equal `z_order` keeps object order, later wins.
- `Animation` objects own timing spans. Animated coordinates reference an
  animation by id and only store motion endpoints.
- `Loop`, `Animation`, `AutoAdvance`, and `Command` are runtime sidecar concepts.
  Static compile/render must remain safe and deterministic.
- The editor must not run `Command` binaries. It renders placeholders only.
- Frame copy/paste and cross-deck paste must remap ranges, group members, and
  animation ids without creating dangling references.
- Auto `Group.frames = None` means members keep their own ranges. Explicit group
  frames override direct members' ranges (one level; a nested group's members
  are gated by that group, not the outer one).
- There must always be at least one frame after frame deletion.
- If editor preview and compiled playback disagree, look first for a runtime
  sidecar object or duplicated frame-replay logic.

## Change playbooks

- **Adding an object type:** follow the checklist in `src/engine/objects/mod.rs`
  (more complete than what the compiler catches), then search for
  `SceneObject::` and review every match the compiler did not force you to
  touch (especially sidecars and special editor flows). Also update
  `PRESENTATION_FORMAT.md`, the bs-deck skill, `README.md` if the user-facing
  catalog changes, and `TESTS.md`. `SceneObject::type_name()` is the one source
  of editor-facing names; `object_defaults::OBJECT_TYPES` keeps each addable
  type's name and shortcut in one `AddableObjectType` record — don't split it
  into parallel arrays. Exhaustive `match`es over `SceneObject` are deliberate
  guardrails; prefer them to wildcards in schema, rendering and editor dispatch.
- **Changing source JSON:** update the serde structs, validation, examples,
  `PRESENTATION_FORMAT.md`, the bs-deck skill and tests. If saved decks should
  keep working, update `src/migrate.rs` or add backward-compatible
  deserialization.
- **Changing timeline or frame operations:** start in `src/editor/state.rs`.
  Verify object ranges, links, groups, animations, auto-advance markers and
  clipboard behavior; favor pure helper tests in `state.rs` plus integration
  tests where rendering changes.
- **Changing playback:** keep the run loop thin and move calculations into pure,
  unit-tested helpers (loop stepping, animation-cluster merging and effective
  auto-delay already work this way) — and mirror them in `web/viewer.js`.
- **Changing editor UI:** expect coordinated edits across `state.rs`,
  `input.rs`, `panel.rs`, `menubar.rs` and possibly `timeline.rs`. Prefer a
  small helper to another large inline rendering branch.
- **Where tests go:** rendering behavior → an integration test in
  `tests/<object>.rs` driven by JSON source; pure geometry/layout helpers →
  inline unit tests next to the helper; TUI-only interactions → isolate the
  state transition or calculation into a helper and test that.

## Architecture

Three-stage pipeline with clean separation:

```
SourcePresentation (JSON)
  → bs::compile::compile()  validate → Engine::scenes (DrawOps per frame)
                            → Renderer::render_bounded (Frame::Full / Frame::Diff)
                            → attach Command/Loop/Animation/AutoAdvance sidecars
  → PlayablePresentation
  → Player::play()          → terminal output
```

`Engine::compile` + `Renderer::render` alone yield only the static frames — no
play-time sidecars and no validation — which is all the tests need (the editor
preview instead streams `Engine::scenes` through `compile::render_scenes`, under
compile's limits); anything that plays or exports a deck goes through
`bs::compile::compile`.

The editor runs the full Engine+Renderer pipeline live for WYSIWYG preview.

**Multiple presentations (parallel decks).** `Editor` (`src/editor/mod.rs`) owns a
`Vec<EditorState>` (`decks`) plus an `active` index and a single cross-deck
`frame_clip: Option<FrameClipboard>`. Each deck is a fully independent
`EditorState` (its own `source`, `file_path`, `current_frame`, `mode`, dirty
flag, object clipboard). The main loop runs `input::handle_event` on the **active**
deck and renders it; all of `state.rs`/`input.rs`/`panel.rs` stay
`&[mut] EditorState`-only. Because a deck can't see its siblings, cross-deck
operations flow through new `Action` variants the `Editor` interprets:
`SwitchDeck(i)`, `OpenDeck(path)` (`open_or_focus` — focuses an already-open file
instead of opening a duplicate; files are compared by normalised path, so
`a.json`/`./a.json` match, and `open_many` dedupes its arguments the same way),
`SaveAs(path)` (`Editor::save_as` — refused while another open deck is bound to
that file; an existing file needs a second Enter to overwrite),
`CopyFrameBlock{lo,hi}`, `PasteFrameBlock{target, before}`. Before each redraw the `Editor` mirrors a read-only `WorkspaceView`
(deck names + dirty markers, active index, frame-clipboard length) into the active
deck (`sync_workspace_view`) so the menu bar and the switcher panel can show
cross-deck info without new signatures. Quit blocks while **any** deck is dirty
(`handle_quit` lists them; q-again discards all, Ctrl-s saves the active deck).
The **[p]resentations** hub (`Mode::PresentationMenu`, top-level `p`) lists the
open decks (↑/↓ + Enter to switch), opens another file (`o` → `Mode::OpenFile`
path prompt), and also hosts **save-as** (`s`), **settin[g]s** (`g`) and
**[f]ullscreen** — which is why Normal's menu bar no longer lists those three
(they moved into this hub; their global keys still work).

**Cross-deck frame copy/paste.** A `state::FrameClipboard` is a self-contained,
deck-independent capture of a contiguous frame block: object ranges normalised to
the block's own `0..frame_count`, `Group.members` block-local (outside members
dropped), `Animation` ids block-local, an animation the block cuts clipped to it
with its driven coordinates re-timed (so the copied frames match the originals,
up to one cell of rounding mid-cut; `copy_frames` does the same for the objects
it clones, not for one it stretches across the insert point), a `Morph` the
block, a single-frame copy/overlay or a split cuts keeps its part of the blend
exactly (`Morph::blend`, set by `Morph::keep_blend`; one the insert stretches
across the insert point is still re-timed), and any
coordinate driven by an animation
*outside* the block flattened to `Fixed` so nothing dangles
(`state::copy_frame_block`). Pasting into another deck
(`state::paste_frame_block`) inserts the frames, shifts ranges/members into the
destination, and — the key correctness step — assigns each cloned `Animation` a
**fresh id** (remapping its coordinates) so a pasted animation never collides with
one already in the target. UX: in a deck, **[f]rame → [s]elect** a contiguous
range → **[y]** yanks it to the frame clipboard; switch decks, then **[f]rame →
[p]aste frames** (offered only when the clipboard is non-empty) opens
`Mode::FramePastePlace` (←/→ pick target, Enter after / `b` before).

**Runtime exception — `Command` objects.** Every object is baked into static
frames at compile time *except* `Command`, which runs a binary whose output and
exit status are only known at play time. A `Command` resolves to just a bordered
box (a safe placeholder shown in the editor — the binary is **never** run while
editing or compiling); its spec is emitted as a `CommandRegion` sidecar on
`PlayablePresentation` (one per run of frames with the same box geometry, so an
animated command's output follows its box). The `Player` executes the binary with **piped** stdio
(it can't touch the real terminal), reads it on background threads (so arrow
keys always interrupt and navigate — a slow/hung command can never trap the
deck), enforces an optional timeout (`timeout_secs`; omitted ⇒ no timeout),
runs it in its own session (`setsid`: its own process group, no controlling
terminal, so a `/dev/tty` prompt fails fast) and kills that whole group when the command
exits or times out, playback leaves its region or the player quits — including when
`bs` gets SIGHUP (terminal closed), SIGTERM or SIGINT, which end playback
through the same teardown (so nothing it started outlives it or holds its
output pipes open; only a SIGKILLed `bs` skips that, and then on Linux just the
command itself is killed, via `PR_SET_PDEATHSIG`, not what it started; and
stopping never blocks on reaping: a leader the player cannot signal, such as
`sudo`, is left to a background reaper and, on a timeout, its box says `[could
not stop the command]` — `reap_without_blocking`),
paints stdout/stderr into the box interior (a lone `\r` drops the line's text so far — not a terminal's overwrite — so a progress line redrawn in place shows its latest state), and marks
the result with a green `✓` (exit 0) or red `✗` (non-zero / timeout / spawn
failure) on the top edge. Navigation does not branch on exit status — you always
stay on the slide and move on with the arrow keys. A command runs once per entry
into its frames: stepping within one `CommandRegion` (arrows, a loop, auto-play)
keeps it running, or keeps its finished output, instead of re-running it
(`player::keeps_command`, decided per command and used by every navigation
path via `go_to_frame`). Every command box covering a frame runs, each with
its own output and mark (`Player::regions_for`, `Player::commands`).

**Runtime exception — `Loop` objects.** Also a play-time behavior that can't be
baked into frames. A `Loop` draws **nothing** (like `Group`); its `frames` range
*is* the loop range, and it emits a `LoopRegion` sidecar on `PlayablePresentation`.
At play time the `Player` enters the loop when navigation lands inside its range
and auto-advances on a timer (`delay_ms`, default 500). With `bounce` (default on)
it ping-pongs forward then backward (`5,6,7,8,7,6,…`); otherwise it restarts
(`5,6,7,8,5,6,…`). `count` plays before moving on (`0` = forever; a finite loop
auto-continues just past its range). A pass counts only once the loop has shown
its start since it was entered, so a loop entered mid-range (← from the slide
after it, a ±10 jump) first plays on to its start (`player::loop_step`). The presenter breaks out with the arrow keys:
`→` jumps to the first frame *after* the loop, `←` to the first frame *before* it
(Home/End/Shift+arrows and q also tear it down). A loop left by any of those
keys (or finished) is not re-entered until playback leaves its range
(`loop_exclude`), so at a deck edge a loop pinned to the first/last frame stays
stopped — and a loop spanning the whole deck stays stopped for good. Loops may **not overlap or nest**, and a loop may
**not bisect an `Animation`** (it must contain each animation span wholly or not
at all, since it replays whole animations) — `SourcePresentation::validate_loops`
enforces all three, plus unique `Animation` ids, both live in the editor (status
warning, also on open and save) and at compile time (hard error). The pure step
function is `player::loop_next`. Loops are added through the normal **Add
Object** menu, so they reuse property editing and frame insert/delete/move
range-remapping for free.

**Runtime exception — `Animation` objects.** A first-class *animation span*,
also play-time. Like `Loop` it draws **nothing**; the motion itself is baked into
the frames via the objects' `Coordinate::Animated` fields. The `Animation` is the
**single source of truth for the span**: its `frames` range *is* the span, and
every animated coordinate references it by a stable `id` (`Coordinate::Animated {
from, to, anim }`) rather than storing its own copy of the span — so a coordinate
can never disagree with its animation about timing, and editing a span updates
exactly one object (no orphan duplicates). The `Animation` also carries the
**auto-play** config (`auto_play`, `delay_ms`) and emits an `AnimationRegion`
sidecar. When `auto_play` is set, the `Player` auto-advances across the span on a
timer, and an arrow key **skips** the whole span — `→` to the first frame past
the last-ending overlapping animation (clamped to the last frame), `←` to the
slide before the earliest-starting one (`Player::animation_cluster`). Unlike
loops, animations **may overlap** freely — where several auto-play
animations cover the same frame boundary, the effective advance delay is the
**minimum** of their `delay_ms` (`Player::auto_advance_delay`). Animations are
created by the editor's **animate sub-menu** (not Add Object); they are still
selectable/editable like a `Loop`, and **editing the `Animation`'s first/last
frame in the props panel just works** — the driven coordinates reference it by
id, so only the driven objects' *visibility* ranges are re-locked (no span to
propagate), a gap strobe being re-made over the new span
(`state::restrobe_driven`); a span that is empty or ends past the deck is
refused like in the animate menu (`check_anim_span`), and strobing clips every
span to the deck. The animate flow's `add_frames` toggle inserts the spanned frames and
**shares** the current frame's elements across them (one range-extended object per
element ⇒ editing one edits all; an earlier `Animation` ending there and an
`AutoAdvance` marker keep their spans, so an earlier motion is not re-timed and
its objects hold their end position on the new frames; a `Morph` ending there
(one the new animation does not drive) keeps its range — its blend timing — and
a frozen copy (`Morph::frozen_at`, coordinates flattened, placed right after it
so equal-`z_order` draw order holds) shows its finished state on the new frames
— a range reaching past
the spans that every later re-lock keeps (`state::restrobe` takes the spans
before the edit to tell a hold from a shortened span); a gap-strobed element
shows on its samples only and is not stretched — while a `Loop` there grows
so it still contains the new animation), and X and Y of an object share **one**
`Animation` by *reference* (reuse-by-id: `apply_animation` reuses the coordinate's
existing `anim` id, allocating a fresh one only for a brand-new animation;
`state::ensure_animation` sets the span + config; `state::next_anim_id` allocates).
An animation has **two halves** — the motion (`Coordinate::Animated` on the driven
objects) and the `Animation` object — so removal cleans up both: **deleting the
`Animation` object** reverts every coordinate it drives back to a static `Fixed`
(at its `from`), widens those objects to span the range statically, drops their
gap-strobe copies (an element another animation still drives is instead
re-locked to, and strobed by, that one), and deletes the object
(`state::remove_animation`, by id). The
in-menu `[x]` revert, every apply, and every object or frame delete run
`state::prune_orphan_animations`, which drops any `Animation` no coordinate
references — so a span edit, a zeroed-out motion or deleting an animation's last
driver can never leave an orphan. Resolution threads the id→span table through the
engine: `Coordinate::evaluate(frame, &AnimSpans)` and `Resolve::resolve(&ResolveCtx)`
(the renderer and player are unchanged — the compiled frames are identical).

**Runtime exception — `AutoAdvance` objects.** A play-time per-frame
auto-transition. Like `Loop`/`Animation` it draws **nothing**: its `frames` range
is the set of frames that advance on their own (end exclusive) and it emits an
`AutoAdvanceRegion` sidecar (`start_frame`, `end_frame`, `delay_ms`, default 5000
= 5 s). At play time the `Player` advances to the next frame after `delay_ms` for
every covered frame — suppressed on the **last frame** (nowhere to go) and while a
`Loop` drives playback; where an auto-play `Animation` also covers a frame the
effective delay is the **minimum** of the two (`Player::frame_auto_advance_delay` +
`effective_auto_delay` feed the existing `auto_deadline`/`schedule_auto` timer).
The presenter can still navigate manually at any time; only a navigation key
restarts the timer (`player::navigates`), so `f` or a stray key does not delay it. An `AutoAdvance` is created
from the **frame** sub-menu's auto-advance action (`t`, configurable
`frame_auto`), *not* the Add-Object menu — so, like `Animation`, it is absent from
`OBJECT_TYPES`. The action opens `Mode::FrameAutoInput` (type the delay in
seconds; `0`/empty turns it off), seeded with the current frame's delay if it
already auto-advances; `state::set_frame_auto_advance` adds/replaces/removes the
single-frame marker (splitting a wider marker that covers the frame, so its
other frames keep their delay) and `state::frame_auto_advance_delay` reads it. Being an
ordinary object, it reuses frame insert/copy/move/delete range-remapping for free
(a deleted frame collapses its range and prunes it) and is selectable/editable
(delay in the props panel, in ms) like a `Loop`.

**`Group` frame range — auto vs. explicit override.** `Group.frames` is an
`Option<FrameRange>`. A group is a logical container whose members are ordinary
top-level objects that render themselves.

- `None` (*auto*, the default for newly created groups): the group has no range
  of its own; members render on their own ranges. The group's effective span is
  the **union** of its members' ranges (`SourcePresentation::effective_frame_range`),
  and the props panel shows blank first/last-frame fields.
- `Some(range)` (*explicit*): the range **overrides** every direct member — at compile
  time each member is gated on the group's range instead of its own (can widen or
  narrow it). `SourcePresentation::effective_objects` applies this (clone the
  member, substitute its `frames`); `Engine::compile`, the `Command`/`Loop`/
  `AutoAdvance` sidecars and `validate_loops` all read it, so a member's runtime
  region follows the group too. An `Animation` member keeps its own span, and a
  `Morph` member its own blend timing (`Morph::timing`; the group only gates it).
  The override is one level deep: a member that is itself a `Group` gates its own
  members by its own range, not the outer one (`member_overrides` reads the raw
  objects). The props panel shows the values plus a `PropertyKind::Note` warning
  (saying so when a member is a nested group, or that the range has no effect).
  Because the override replaces a member's own range, a gap strobe (which
  narrows that range to its samples) cannot apply to an explicit group's member:
  Animate/Converge `[s]` with gap frames, a gapped animation's span edit, and
  making the group of a strobed direct member explicit are refused
  (`state::strobe_group_error`).

In the editor: entering a first/last-frame value materialises an explicit range
(seeded from the derived union); blanking either field reverts to auto. Because
the stored range is now optional, `state::scene_object_frame_range[_mut]` return
`Option<&[mut] FrameRange>` (None for an auto group) and frame insert/delete skip
auto groups (their members shift instead).

## Module Map

| Path | Role |
|------|------|
| `src/main.rs` | CLI entry point (`compile`/`edit`/`play`/`migrate`); `compile` refuses an output that is its source file (`editor::same_file`), since a playable can't be turned back into a source deck |
| `src/lib.rs` | Crate root — declares the public modules so integration tests and `examples/hello.rs` drive the pipeline as a library |
| `src/menubar.rs` | Shared `print_menu_item` helper (bolds the `[k]` shortcut inside a dim menu string) used by both the player and `src/editor/menubar.rs` |
| `src/term.rs` | `TerminalGuard` — the raw-mode/alternate-screen session (autowrap off, so an over-long line is cut at the edge instead of scrolling the screen; bracketed paste on, so a paste arrives as one `Event::Paste` — the editor inserts it into the active text field via `TextEdit::insert_str`, line breaks only in multi-line fields, and ignores it elsewhere; the player ignores it) shared by the editor and the player; restores the terminal on drop **and** from a panic hook (installed once, runs before the default hook prints), so a panic leaves a usable shell with a readable message. Also `key_press`, the press-only key filter both use (Windows reports every release) |
| `src/compile.rs` | **The single compile path** — `compile(&SourcePresentation) -> PlayablePresentation` (validate the deck contract limits — `SourcePresentation::validate_contract`, also enforced by the editor's Settings and preview — and loops → `Engine::scenes`, failing a frame that draws over `MAX_FRAME_OPS` cells (checked object by object) → `Renderer::render_bounded` via `render_scenes`, failing once the frames draw over `MAX_DECK_OPS` cells in total (bounding the total draw ops/renderer work, not resolve work that emits few ops) or past `MAX_STORED_CELLS` stored cells → attach the `Command`/`Loop`/`Animation`/`AutoAdvance` sidecars) plus the string-in/string-out `compile_json`. Both `bs compile` (`src/main.rs`) and the browser's WebAssembly compiler (`wasm/src/lib.rs`) call it, so the two cannot drift. Free of I/O and of anything `wasm32-unknown-unknown` lacks — no fs, clock, env, threads or terminal |
| `src/migrate.rs` | One-shot upgrade of old-format source JSON to the current animation model — works on the raw `serde_json::Value` (the current structs can't parse the old shape), assigns `id`s to `animation` objects, rewrites `{"animated":{…,start_frame,end_frame}}` coords to `{from,to,anim}` by span match (synthesizing a sidecar with `auto_play: false` for orphan spans, so playback stays manual as before). Idempotent; self-verifies before writing in place (`<file>.bak` backup) — the result must parse as a `SourcePresentation` and pass `validate_contract` + `validate_loops`, else the file is left unchanged |
| `src/types.rs` | Shared types: `Color`, `Style`, `Cell` (`Cell::printable` — the char the player and editor preview send to the terminal, exactly one column: control/zero-width chars as a space, chars not known to be single-width (CJK, emoji) as `?`, via the shared `narrow_char` table that `player::layout_output` also uses), `DrawOp`, `Frame`, `PlayablePresentation`, `CommandRegion`, `LoopRegion`, `AnimationRegion`, `AutoAdvanceRegion` |
| `src/engine/source.rs` | `SourcePresentation` (+ `command_regions()`, `loop_regions()`, `animation_regions()`, `auto_advance_regions()`, `validate_loops()`, `link_siblings()` (valid indices only; the editor drops stale entries on open via `sanitize_links()`, and stale `Group.members` via `sanitize_group_members()`), and a `links` sidecar — editor-only families of object indices for *linked* paste, ignored by the engine), `SceneObject` (including the canonical `type_name()` display names), `Coordinate` (Fixed / Animated{from,to,anim}), `AnimId` + `AnimSpans` (the id→span table; `Coordinate::evaluate(frame, &AnimSpans)` looks a coordinate's span up there), `FrameRange` |
| `src/engine/mod.rs` | `Engine::compile` — walks `0..frame_count`, applies `Group` member overrides, resolves every object into per-frame `DrawOp`s |
| `src/engine/objects/` | The 15 `SceneObject` types: `Label`, `HLine`, `Rect`, `Header`, `Group`, `Arrow`, `Table`, `Art`, `Command`, `List`, `Loop`, `Morph`, `Animation`, `AutoAdvance`, `Circle` — each implements `Resolve`. See the module-doc checklist in `mod.rs` for every site a new type touches. `List` (ordered/unordered) shares `Label`'s text-editing UX and the shared `wrap` helper. `Loop` (like `Group`) draws nothing; its `frames` range is the loop range and it emits a `LoopRegion` sidecar. `Morph` blends two inline ASCII-art grids (`from`→`to`) across its `frames` range — each cell flips to the `to` glyph once playback progress passes that cell's per-cell threshold (`MorphMode`: `dissolve` or four directional wipes). Fully baked into static frames in `resolve`, so the editor preview shows it for free. `Animation` (also draws nothing) **owns** the animation span (its `frames`) — the single source of truth — plus an `id` that driven `Coordinate::Animated { anim }` fields reference; it emits an `AnimationRegion` sidecar and is created by the animate sub-menu, not the Add-Object menu. `AutoAdvance` (also draws nothing) makes its `frames` auto-transition to the next slide after `delay_ms` (default 5 s); it emits an `AutoAdvanceRegion` sidecar and is created by the **frame** sub-menu's auto-advance action — so `Animation` and `AutoAdvance` are the two types absent from `OBJECT_TYPES`. `Circle` is a **parametric** filled circle (unlike the static `Art` pieces): editable `diameter` (rows) + fill `ch` (default `@`), with the column extent derived from the diameter (`Circle::columns`, ~2× for the terminal's 2:1 cell aspect) so it stays round; added from the **Add-Object** menu (quick-add `o`) and baked into static frames in `resolve`. Each type implements `Resolve::resolve(&ResolveCtx, ops)` (the `ResolveCtx` carries `frame`, `canvas_width`/`canvas_height`, and the `&AnimSpans` table; renderers cap their loops at the canvas via `ResolveCtx::visible`, so a huge size field costs only the visible cells) |
| `src/art_library.rs` | Built-in + user ASCII-art palette (`~/.config/bs/art/`, one file per piece; `load_file` turns CRLF/lone CR into `\n` and expands tabs to 8-column stops); pieces are copied into self-contained `Art` objects when added. Includes a matched `ball`/`square` pair used as the default `Morph` endpoints. The picker (`Mode::AddArt`/`LoadArtFile`) carries an `ArtPick` purpose so the same flow serves a standalone `Art` or the two-stage `from`/`to` pick of a `Morph` |
| `src/renderer/mod.rs` | Rasterizes DrawOps into cell grid; diffs frames |
| `src/player/mod.rs` | Playback loop, keyboard nav (arrows, Shift+←/→ jump ±10 frames, space, PageDown/PageUp = →/← for presentation clickers as in the web viewer, q, f=fullscreen); runs `Command` objects (piped, async, timeout) and overlays output; drives `Loop` regions (timer-based auto-advance + bounce + arrow-key break-out) via the pure `loop_next` step fn; auto-advances across auto-play `Animation` spans (`auto_deadline`), using `auto_advance_delay` = the **min** `delay_ms` over the animations covering each boundary, with the loop's own delay as the fallback for gaps inside a loop (each loop wait is priced on the step the loop actually takes next, so a bounce turnaround uses the animation's delay — `loop_wait`). On an auto-play animation (no loop), an arrow **skips** the whole span: `→` jumps to the first frame past the last-ending overlapping animation (clamped to the last frame), `←` to the slide before the earliest-starting one — the merged cluster comes from `animation_cluster` (connected by overlap). Also auto-advances across `AutoAdvance` regions: `frame_auto_advance_delay` is the **min** `delay_ms` over the markers covering a frame (None on the last frame), and `effective_auto_delay` = the min of that and the animation boundary delay, feeding the same `auto_deadline` timer (suppressed while a loop drives) |
| `src/editor/mod.rs` | Editor lifecycle, raw mode setup, main loop. Holds **multiple decks** (`Vec<EditorState>` + `active`) and the cross-deck `frame_clip`; interprets the cross-deck `Action`s (`SwitchDeck`/`OpenDeck`/`SaveAs`/`CopyFrameBlock`/`PasteFrameBlock`), mirrors a `WorkspaceView` into the active deck (`sync_workspace_view`), and gates quit on any-deck-dirty (`handle_quit`). `open_many(&[String])` opens N files; `open(&str)` wraps it |
| `src/editor/state.rs` | `EditorState` (incl. `clipboard` + `clipboard_sources` for copy/paste), `Mode` enum (~30 variants, incl. table sub-modes, art picker, frame sub-menu/move/overlay/jump/select/auto-input/range-place, `MultiSelect` (`MultiSelectPurpose::Group`/`Select` — copy/converge/delete/edit-props then come from the `SelectAction` sub-menu), `SelectAction` (the post-multi-select action sub-menu), `EditMultiProperties` (bulk-edit the shared props of a selection), `ConvergeConfig`, `PastePlacing`). Frame ops: `insert_blank_frame` + `insert_blank_frames_at` (N-frame generalisation), `copy_frame` (deep-clone duplicate into a *new* frame), `overlay_frame` (deep-clone paste onto an *existing* frame, no new frame), `move_frame`/`move_frames` (relocate one frame or a block — both via the shared `remap_ranges_through_pos` permutation), `copy_frames` (duplicate a contiguous block as new frames), `parse_frame_selection` (`1,2,3`/`5-12` → indices) + `delete_frames` (multi-delete, highest-first, keeps ≥1). Copy/paste helpers: `expand_selection` (pull in a group's members, through nested groups), `clone_selection` (self-contained deep clone with selection-local member remap). Cross-deck frame clipboard: `FrameClipboard` + `copy_frame_block` (capture a contiguous block, normalised to be deck-independent) + `paste_frame_block` (insert into another deck with fresh `Animation` ids + group/range remap). New modes `PresentationMenu`/`OpenFile`/`FramePastePlace`; `WorkspaceView` (Editor-mirrored deck list / active / frame-clip length, read by the menu bar + switcher panel). Opening a file notes the JSON keys a save would drop (`unknown_fields`: unknown or misspelt keys, probed so a known field left at its default is not flagged), warned on open and on the first save. Object delete fixes both `Group.members` and `links` families (`adjust_group_members_after_delete`) as objects are pruned, and every other removal (gap-copy clear, orphan-animation prune, auto-advance marker replace) shifts `clipboard_sources` with it (`EditorState::note_removed_objects`) so a linked paste never links the wrong object; `delete_objects` deletes a set at once — the single `d` too — (plain objects highest-first, `Animation`s by id via `remove_animation`, then `prune_orphan_animations` drops any animation left driving nothing; frame deletion prunes the same way) |
| `src/editor/config.rs` | `KeyBindings` — all bindings configurable via `~/.config/bs/editor.json`. `matches_binding` parses `Ctrl-`, `Alt-`, and `Ctrl-Shift-` prefixes (the last requires keyboard-enhancement to be distinguishable). Single-letter bindings are **shift-aware**: a capital `S` matches Shift+S however the terminal encodes it (`Char('S')`±SHIFT or `Char('s')+SHIFT`), and a lowercase letter never fires on a shifted press — so capital-letter shortcuts like `S`=save-as / `F`=fullscreen work across terminals |
| `src/editor/input.rs` | All key event handling. Property browse/edit/dropdown flows (object + table cell-style) share helpers: `TextEdit` (text fields), `dropdown_key`/`DropdownKey` (list nav), and the `ep_*` `Mode::EditProperties` constructors |
| `src/editor/textedit.rs` | `TextEdit` — reusable text-buffer + cursor used by every text field (property values, the multi-line overlay, table cell content and cell-style values); translates key events into edits (insert/delete/arrows/home-end/newline) |
| `src/editor/panel.rs` | Left panel (Add Object), right panel (Properties incl. `Bool` checkboxes + colour swatches), object selection overlay, and the centred multi-line text-editing overlay (`render_text_overlay`). Every text field draws its caret through one shared helper, `draw_caret_line` (see "Text caret convention" below) |
| `src/editor/properties.rs` | `Editable` trait — one impl per object type holds its property list, setter, coordinate + geometry accessors; generic dispatch (`get_properties`, `set_property`, `common_properties` = the intersection of bulk-editable props across a selection, …) is type-agnostic. `PropertyKind::Bool` flags toggle in place (Space/Enter); `PropertyKind::Note` renders a non-editable free-form warning line (the whole `value`, no `name:`) — the mechanism for surfacing per-object warnings in the panel |
| `src/editor/object_defaults.rs` | `OBJECT_TYPES` (the Add-Object menu list — each entry an `AddableObjectType { name, shortcut }` descriptor so a type's display name and quick-add letter live in one record, not index-aligned parallel arrays) + `create_default()` — the lookup table the compiler can't check when a type is added; the inline `object_type_registry_is_complete` test cross-checks it against the `SceneObject` enum (see the checklist in `src/engine/objects/mod.rs`) |
| `src/editor/preview.rs` | Canvas preview using Engine+Renderer — resolves only frames `0..=current` (what `grid_at` replays), lazily, under the same `MAX_FRAME_OPS`/`MAX_DECK_OPS`/`MAX_STORED_CELLS` limits as compile (`compile::render_scenes`), and shows "Cannot preview: …" past them |
| `src/editor/timeline.rs` | Frame bar (row 1) and mode/status line (row 2). The frame bar is always shown (a deck over `MAX_FRAME_COUNT` gets a one-line note instead, `frame_bar_note`, since the bar is built per frame on every redraw); while typing a `FrameJump`/`FrameSelectInput`, it live-highlights the slides the input resolves to and the typed field + instructions render on row 2. Frames under an auto-play `Animation` collapse into a single range cell (`[10-20]`); strictly-overlapping auto-play spans merge into one range (continuous auto-advance), adjacent-but-disjoint ones stay separate. When the bar overflows the row it abbreviates to the **first 3** segments, a 3-wide window around the current frame, and the **last 3** (with `...` for skipped gaps); the edge groups shrink 3→2→1 only when the row is too narrow (`abbreviated_indices`/`pick_indices`) |
| `src/editor/menubar.rs` | Context-sensitive menu bar |
| `src/editor/ui.rs` | Layout computation |

## Editor Mode FSM

```
Normal ──a──→ AddObject ──Enter──→ Normal (object added)
       ──s──→ MultiSelect{Select} ──Enter(1 obj)──→ SelectedObject ──e──→ EditProperties ──a──→ AnimateProperty
                                  ──Enter(2+ obj)─→ SelectAction (Copy / Converge / Delete / Edit Props)
       ──p──→ PresentationMenu ──Enter──→ (switch deck) │ ──o──→ OpenFile │ ──s/g/f──→ SaveAs/Settings/fullscreen
```

- **Normal**: frame navigation (←/→; the **big-jump** keys Shift+←/→ — and their always-deliverable aliases `[`/`]` (`anim_skip_prev`/`anim_skip_next`), since macOS Terminal.app and many tmux setups send Shift+arrow as a bare arrow — **skip the animation under the cursor** (jump just before/after its merged span, like the player's arrows; cluster from `input::animation_cluster`, target from the shared `input::jump_target`), or scrub ±10 frames clamped (`FRAMES_PER_JUMP`) when not on an animation), `f` opens the frame sub-menu (refused, with a status warning, on a hand-edited deck over `MAX_FRAME_COUNT`, since frame ops walk every frame — `state::frame_limit_error`; fix `frame_count` in the file. The editor itself cannot grow a deck that far: add, copy, block copy and frame paste are refused when the grown deck would fail `validate_contract` — `state::frame_growth_error`, as Animate/Converge add-frames), `p` opens the **[p]resentations** hub, Ctrl-s save, q quit. **Save-as** (`Mode::SaveAs` → `Action::SaveAs` → `Editor::save_as` → `state::save_as`, adopting the new path), **settings** (frame size) and **fullscreen** moved into the presentations hub and are no longer on Normal's menu bar — their global keys (`Shift+S`, `g`, `Shift+F`) still fire (capital `S`/`F` work on every terminal; `Ctrl-Shift-s` was undetectable without keyboard-enhancement). `s` enters **Select** (multi-select; copy & converge live in its action sub-menu — see below)
- **PresentationMenu** (`p`, the presentations hub): lists every open deck (active marked `●`, dirty marked `*` in the name) in the right panel; ↑/↓ move the cursor, **Enter** switches the active deck (`Action::SwitchDeck`). `o` → **OpenFile** (open another deck), `s` → **SaveAs**, `g` → **Settings**, `f` → fullscreen, Esc back. The deck list is read from the Editor-mirrored `state.workspace`.
- **OpenFile** (from the presentations hub via `o`): a path prompt (panel input, reuses `frame_text_key` + `draw_caret_line`). Enter → `Action::OpenDeck(path)`, which the Editor opens as a new deck (or focuses if already open); Esc returns to the hub.
- **FrameMenu**: frame operations — `a` add blank frame, `c` copy (duplicate) current frame, `o` overlay (paste) current frame's objects onto another existing frame, `j` jump to a frame by number (`FrameJump`), `s` select multiple frames (`FrameSelectInput` → `FrameSelected`), `t` auto-advance the current frame after a delay (`FrameAutoInput` → an `AutoAdvance` marker), `d` delete current frame (with confirm), `m` move current frame, `p` **paste frames** from the cross-deck frame clipboard (shown only when it holds frames → `FramePastePlace`), Esc back. Both input modes keep the frame bar (slide range indicator) on its own row and put the typed field + instructions on the mode/status row beneath it, live-highlighting the slides the input resolves to. **FrameJump** types a 1-based frame number (previewing the target slide); Enter jumps the deck there (clamped). **FrameSelectInput** types a list/range (`1, 2, 3` or `5-12`, mixable, `state::parse_frame_selection`); Enter → **FrameSelected**, which highlights the chosen frames in the timeline and offers `d` to delete them all (`state::delete_frames` removes highest-index-first and always keeps ≥1 frame). For a **contiguous** range it also offers `m` move and `c` copy → **FrameRangePlace** (see below), and `y` to **yank the block to the cross-deck frame clipboard** (`Action::CopyFrameBlock` → `state::copy_frame_block`) for pasting into another open deck. **FrameAutoInput** types the auto-advance delay in **seconds** (default 5, `0`/empty = off), seeded with the current frame's delay if it already auto-advances; Enter calls `state::set_frame_auto_advance`, which adds/replaces/removes a single-frame `AutoAdvance` marker (stored as `delay_ms`), first splitting any wider marker covering the frame around it. The single-frame ops: `add` calls `state::insert_blank_frame` (the "make room" primitive — a new empty frame). `copy` calls `state::copy_frame`, which inserts a blank frame and then **deep-clones** every object on the source frame onto it, so the copy's objects are independent of the original (editing one never changes the other). Deck-wide/spanning objects stay shared (extended across the new frame) rather than cloned, so they remain a single continuous object
- **FrameOverlay**: paste the current (source) frame's objects *on top of* another existing frame, **without** inserting a new frame. ←/→ scroll the deck to a target frame; Enter calls `state::overlay_frame`, which **deep-clones** every object on the source frame onto the target (same positions/styles/z-order), appended after the target's existing objects so they render over it. A cloned auto group keeps only the members shown on the slide (a member on another slide stays with the original group only — `repoint_cloned_groups`). Objects already visible on the target (e.g. a deck-wide background spanning both frames) are skipped rather than duplicated, and so are `Animation`s, `Loop`s and `AutoAdvance` markers (span behaviours, not slide content; the target keeps its own timing — `copy` skips `Animation`s and `Loop`s too but keeps a marker, since a duplicated slide keeps its timing; a block copy takes a `Loop` only when wholly inside the block). Unlike copy/move, the deck's `frame_count` is unchanged
- **FrameMove → FrameMovePlace**: relocate the current slide. In FrameMove, ←/→ scroll the deck to a target slide; Enter opens FrameMovePlace, where Enter drops the moved slide *after* the target and `b` drops it *before* (`state::move_frame` remaps object ranges through the new frame ordering *by membership*: every object keeps showing on exactly the slides it did, so a multi-slide object the move tears — a slide moved out of or into its run — is split into one object per contiguous run, the extra pieces linked to it, added to its auto groups and placed right after it; `remap_ranges_through_pos`). A move that would tear an object that can't be split faithfully — a `Loop`, `Animation` or `AutoAdvance` span, a `Morph` (its blend is timed by its range), an explicit-range `Group`, or an object with animated coordinates — is refused with a status message, deck unchanged (`state::move_frames_refusal`; FrameRangePlace's move too)
- **FrameRangePlace**: place a moved or copied **contiguous** frame block (reached from FrameSelected via `m`/`c`; the block must be contiguous — a scattered selection is rejected). ←/→ scroll the deck to a target slide; `Enter` drops the block *after* it, `b` *before* it (the `copy` flag picks the verb). **Move** calls `state::move_frames` (pure reorder; the target may not lie inside the moved block). **Copy** calls `state::copy_frames`, which inserts `count` new frames at the destination (`insert_blank_frames_at`) and deep-clones the block's content onto them — per-frame objects land on their copy frame, objects spanning within the block stay single spanning clones, a deck-wide background the insert already stretches over the new frames is *not* re-cloned, and an auto group whose members are cloned is cloned with them (holding just those clones); an object straddling the insert point without covering the whole block (a bullet revealed mid-block) is split around the copy (`split_around_copy`: the original ends at the copy, a linked continuation carries on after it; an explicit group's members' own ranges are split with it when the insert stretched them, keeping a member Morph's blend) so the copy shows it only where the block did. The deck lands on the first frame of the result. A copy of a block holding a whole `Loop` into the middle of another loop or an animation is refused with a status message (`state::copy_frames_refusal`; the deck is unchanged and the mode stays, so another slide can be picked), since the stretched span would nest the loop or be cut by it
- **FramePastePlace** (reached from the frame sub-menu's `p` paste-frames action, only when the cross-deck frame clipboard is non-empty): ←/→ scroll the deck to a target slide; `Enter` drops the pasted block *after* it, `b` *before* it (`Action::PasteFrameBlock` → `state::paste_frame_block`, which inserts the frames, shifts ranges/group-members into the destination, and assigns each cloned `Animation` a fresh id so it can't collide with the target deck's). The pasted slides show only what their source slides showed: a target object crossing the paste point — even a deck-wide background — is split around them like `copy_frames` does (`split_around_copy`, linked continuation after the block); only `Loop`/`Animation` spans stretch over them (an `AutoAdvance` marker is split too, so the pasted slides keep their own timing). An explicit group's member keeps its own range (a member Morph's blend timing) split or un-stretched with it, so its stages on the original slides do not change. A clipboard holding a `Loop` is refused inside another loop or an animation, as for FrameRangePlace (`state::paste_frame_block_refusal`). The frame clipboard lives on the `Editor`, so it persists across deck switches and re-pastes
- **Settings**: edit the output frame size (width × height in cells); ↑↓/Tab switch field, Enter apply, Esc cancel
- **AddObject**: choose object type from the list (↑/↓ + Enter) or press its **quick-add shortcut** — one unique letter per type, shown as `[l] Label` and defined together with its display name by `object_defaults::OBJECT_TYPES` (each an `AddableObjectType { name, shortcut }`; `object_type_for_key` maps a keypress to the type). Either path runs the shared `commit_add_object`. After committing, most types land in `EditProperties` (browse); `Group`/`Art` enter their member/library pickers; `Morph` runs the art-library picker **twice** (pick the `from` piece, then the `to` piece) before landing in `EditProperties`; `Label` and `List` jump straight into the centred multi-line text overlay (empty buffer) so you can type content immediately — Esc keeps the default text, Enter commits
- **Select** (`s`, the single entry point): a **multi-select** reusing the `MultiSelect` toggle flow (`MultiSelectPurpose::Select`). `Space` toggles members (the cursor object is highlighted on the canvas; a `Group` expands to its members), `d` deletes the highlighted object (the old browse-and-delete), `Enter` **acts** on the chosen set (toggled members, or the highlighted object if none toggled): **1 object → `SelectedObject`** (its move/resize/edit/delete/copy menu), **2+ objects → `SelectAction`**. There is no longer a separate single-pick `SelectObject` mode.
- **SelectAction**: the action sub-menu shown after selecting 2+ objects (`SELECT_ACTIONS`, ↑/↓ + Enter). Currently **Copy** (`copy_to_clipboard`), **Converge** (`expand_selection` → `enter_converge`), **Delete** (confirm → `state::delete_objects`, removing the whole selected set at once), and **Edit Props** (bulk-edit the shared properties → `EditMultiProperties`). Copy & converge moved here from their old top-level `c`/`Shift+C` keys; delete is the multi-object counterpart to `SelectedObject`'s single `d`.
- **EditMultiProperties** (reached via **Select → SelectAction → Edit Props**): bulk-edit the properties **common** to every selected object. The panel lists only the props all members share by name *and* kind, restricted to the bulk-editable kinds (`properties::common_properties` — geometry/colour/flags/numbers/simple dropdowns; `Text`, group-member, table-column, read-only/note are excluded). Values shown are the **first member's** (the representative seed). Editing one value writes it to **every** member: `input::apply_multi_property` just calls the single-object `apply_property` per member, so group auto-range, animation re-locking, link propagation, and loop validation all behave exactly as for a single edit. A member whose coordinate is **animated** is skipped (`properties::set_property` refuses to overwrite an `Animated` coordinate with a number, which would orphan its `Animation`; revert it with `[x]` in the animate menu). The handlers (`handle_edit_multi_properties`/`_value`/`_dropdown`, `emp_*` constructors) are slim cousins of the `EditProperties` ones — no animate/table/group-member/multi-line-text path, since those kinds never enter the common set. `Esc` returns to `SelectAction` with the selection intact.
- **Copy/paste** (`v` paste, configurable; copy is reached via `SelectedObject`'s `c` for one object or the **Select → SelectAction → Copy** sub-menu for many): **copy** captures objects to `EditorState.clipboard` as self-contained deep clones — either one object (`c` in `SelectedObject`) or a `MultiSelect{Select}` toggle set (via the action sub-menu); a copied `Group` pulls in its members (`expand_selection`); span behaviours are left out — `Animation`s (a pasted copy is static, and a cloned `Animation` would duplicate its id), `Loop`s and `AutoAdvance` markers (pasted onto one slide they would be a one-slide loop or make the slide auto-advance), noted in the status and refused when nothing else is selected; a copied group keeps its other members. **Paste** is not a standing top-level command: the `v` binding works in Normal and `SelectedObject`, but the **menu only surfaces `[v] paste` once the clipboard is non-empty** (so it appears right after a copy and stays visible while you navigate to the target frame). A paste does not consume the clipboard: it persists until the next copy replaces it, so `[v] paste` stays available and re-pastes the same objects. **Paste** (`v`) enters `PastePlacing`: clones land on the current frame (re-anchored to it; at copy time animated coordinates are flattened to `Fixed` at the copy frame via `state::flatten_coordinates`, so the copy is static, arrow-nudgeable and holds no animation ids, and a `Morph` keeps the blend it showed there via `Morph::keep_blend`, then nudged off the source) as a movable **ghost** that rides the arrow keys; **Enter** drops the set and re-arms a fresh ghost (rubber-stamp loop — stamp N copies), **Esc** discards the un-dropped ghost and finishes. `l` toggles **Independent** vs **Linked**: a *linked* paste records one `links` family **per clipboard object** (its source — for a gap-strobe copy, its set's original — + each stamp's clone of it; a stamp made while Linked stays linked whatever the toggle shows at Esc), so editing a non-placement property of any member propagates to its siblings (`apply_property` → `SourcePresentation::link_siblings`; a linked table's cell and column edits copy its cells/columns onto its linked tables that held the same content — one that already differed is left alone and the status says so (its props-panel fields, `col_N_width` included, still propagate by index), `input::sync_linked_tables`; placement = every coordinate field (`x/y/width/height`, `HLine`/`Arrow` endpoints) plus `first_frame/last_frame/z_order` stays per-copy). Distinct objects copied together never cross-sync. The ghost clones live in `objects` (tail indices in `pending`), so the WYSIWYG preview shows them; Esc truncates that tail
- **Converge** (reached via **Select → SelectAction → Converge**): animate a set
  of objects so they all meet on **one shared point**, each starting from
  *wherever it happens to be* at the span start. The chosen members flow from the
  Select multi-select (a selected `Group` is expanded via `expand_selection`),
  then `Enter` on the Converge action opens
  `Mode::ConvergeConfig` — a numeric field menu seeded with the **centroid** of
  the members' current positions as the default shared target. It reuses the
  `AnimRole` machinery but **drops the per-object `from` fields** (`CONVERGE_ROLES`
  = `x to`/`y to`/`start`/`end`/`add frames`/`auto play`/`delay ms`/`gap frames`;
  `add frames` defaults **off** so it animates over existing frames). `[s]` applies
  via `input::apply_converge`: it allocates **one** shared animation id
  (`next_anim_id` + `ensure_animation`), then for each member seeds `from` = the
  coord's `evaluate(start_frame, &anims)` (its *displayed* position, even
  mid-motion) and sets `Coordinate::Animated { anim }` toward the shared
  `to`/`to_y` on whichever axes the object has (via the shared
  `set_object_animation` helper, also used by `apply_animation`); an axis already
  at the target stays `Fixed`. Frames are inserted/shared **once** (if `add
  frames`); `prune_orphan_animations` then drops any of the members' previous
  animations the convergence left unreferenced. Convergence is just N objects
  whose animated coords reference one shared animation
- **SelectedObject**: move (arrows; an object, a group's members or the paste ghost stop at the left/top edge as a whole, keeping their shape and layout — `properties::move_objects`), `r` → resize mode, `e` → edit props, `d` delete; Shift+arrows also grow
- **ResizeObject**: arrow-key resize (←→ width, ↑↓ height) — a terminal-robust path since many terminals capture Shift+↑/↓ for scrollback; Enter/Esc exit
- **EditProperties**: edit typed properties; color fields show dropdown; text fields support multi-line (Alt-Enter = newline); property list scrolls vertically
- **AnimateProperty**: a role-based field list (`input::anim_roles`/`AnimRole`).
  Animating `x` or `y` on an object that has **both** becomes a *two-axis* session
  — fields `x from`/`x to`/`y from`/`y to` so x and y are set together; every other
  coordinate (width/height, or `y` on an `HLine`) stays single-axis with `from`/`to`.
  Then `start`/`end`, `add frames`/`auto play` (toggles, Space/Enter), `delay ms`,
  and `gap frames`. `enter_animate` seeds every value from the object's current
  coordinate(s), so an untouched axis is preserved on apply (a `Fixed` stays fixed,
  an `Animated` stays animated). `[s]` first refuses (status error, menu stays open) a span that ends past the deck, whose inserted frames would break the deck-size limits (`input::check_anim_span` → `validate_contract`), or that inserts frames (*add frames* on a new animation) but does not start on the current frame — the frames go in right after it, so any other start would not match them (Converge's `[s]` too); otherwise it applies via `input::apply_animation`:
  it reuses the coordinate's existing `anim` id (or allocates a fresh one via
  `state::next_anim_id` for a brand-new animation), optionally inserts the spanned
  frames + shares the current frame's elements (`state::add_frames_and_share`),
  records the span + config on the `Animation` (`state::ensure_animation`), sets
  the `Coordinate::Animated { anim }` on each moving axis (`from != to`), and
  re-locks the object's own range to the union of its animations'
  spans (`scene_object_animation_span(obj, &anims)`), keeping any extension
  past their previous end (`state::restrobe` gets the pre-edit spans) — which is
  what preserves an add-frames hold — and, when re-editing an
  animation other objects share (a Converge), re-locks and re-strobes every
  object it drives (`state::clear_driven_gap_clones` before the span/gap change,
  `state::restrobe_all_driven` after), like the props-panel span edit. Re-applying never spawns a
  second animation (same id updated in place); `state::prune_orphan_animations`
  drops the animation if no axis actually moved.
  `gap frames` > 0 then strobes the element (`state::restrobe`, which strobes
  it over *each* of its gapped animations' spans, so animating another
  coordinate keeps an earlier animation's strobe): `gap frames`
  is the count of *empty* frames between appearances, so the element shows every
  `gap + 1` frames of the span (single-frame samples at the interpolated position,
  `gap` blank frames between — a stop-motion look; `gap = 3` ⇒ frames `start`,
  `start + 4`, `start + 8`). It works on whatever frames the span covers,
  **independent of `add frames`** (inserted or pre-existing). Re-applying is
  idempotent: `apply_animation` first calls `state::clear_gap_clones` to remove
  the element's *own* prior strobe copies (matched by whole-object content, so an
  overlapping animation with the same motion is left intact), then re-strobes.
  A strobe set (original + copies, `state::strobe_set`) edits as **one element**:
  on a later sample frame only a copy is selectable, so `input::handle_key`
  mirrors any in-place edit of a set member (move, resize, props, table edits —
  also through a selected `Group`, whose leaves count as edited, and through link
  propagation, whose siblings do)
  onto the rest of the set (`state::sync_strobe_set`; skipped when the key also
  changed any other object, e.g. a span edit that re-strobed), and animate `[s]`/`[x]`
  and Converge act on the set's original — copies never diverge into a second
  "original" that a later re-strobe would double. The pieces of an element a
  frame copy split (`split_around_copy`: a head and a linked continuation, identical but
  for their ranges and driven by the same animations) are not strobe copies: the set and the copy clearing
  skip them, and a re-lock moves only a piece's outer edge, never the edge facing
  another piece (`state::split_bounds`), so the copied block stays uncovered. Deleting any sample deletes the
  whole set (`state::delete_objects`), so no later re-strobe can bring one back.
  `[x]` reverts the coordinate to `Fixed`, then `state::prune_orphan_animations`
  removes any `Animation` no coordinate references any more. Deleting the
  selectable `Animation` object goes the other way — `state::remove_animation(id)`
  reverts the motion of every object it drives *and* drops the object (see the
  `Animation` runtime exception above). Defaults: add-frames on, auto-play on,
  500 ms, gap 0 (off — element on every frame). Re-animating a span reseeds its
  auto-play settings (`enter_animate`)

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
// Coordinate supports linear-interpolated animation. The *motion* (from→to)
// lives here; the *span* lives only on the referenced `Animation` (id `anim`) —
// the single source of truth for timing. evaluate(frame, &AnimSpans) looks the
// span up; Fixed is f64 (group-scaling uses fractional precision), floored.
enum Coordinate {
    Fixed(f64),
    Animated { from: u16, to: u16, anim: AnimId },   // AnimId = u32
}
// AnimSpans: an id→FrameRange table built once (AnimSpans::of(source)) and
// threaded through Resolve via ResolveCtx { frame, canvas_width, canvas_height, anims }.

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
        "y": { "animated": { "from": 2, "to": 8, "anim": 1 } }
      },
      "style": { "fg": "red", "bold": true },
      "frames": { "start": 0, "end": 8 },
      "z_order": 1
    },
    { "type": "animation", "id": 1, "frames": { "start": 0, "end": 5 } }
  ]
}
```

- `style` is optional (omit for defaults)
- `frames.end` is exclusive
- Color values: named strings (`"black"`, `"red"`, `"green"`, `"yellow"`, `"blue"`,
  `"magenta"`, `"cyan"`, `"white"` — the 8 `NamedColor`s only) or an RGB object
  `{ "r": 20, "g": 20, "b": 40 }` (`Color` is an untagged enum, so the RGB form is
  a map of `r`/`g`/`b` — **not** `{"rgb": [...]}`, which fails to deserialize)
- Many numeric fields (widths, heights, `hline` endpoints) accept either a bare
  number or a `Coordinate` object, via `deserialize_coord_compat`
- A `loop` object has no geometry — just a range plus playback options
  (`delay_ms` default 500, `count` default 0 = forever, `bounce` default true):

  ```json
  { "type": "loop", "frames": { "start": 4, "end": 8 },
    "delay_ms": 500, "count": 0, "bounce": true }
  ```

- An `animation` object has no geometry — just an `id`, a range, and auto-play
  config (`auto_play` default true, `delay_ms` default 500). Its `frames` range
  **is the single source of truth for the span**; animated coordinates reference
  it by `id` (`{"animated":{"from","to","anim":<id>}}`) and carry no span of
  their own. The motion (`from`→`to`) lives on the coordinate:

  ```json
  { "type": "animation", "id": 1, "frames": { "start": 0, "end": 5 },
    "auto_play": true, "delay_ms": 500 }
  ```

- An `auto_advance` object has no geometry — just a range and a `delay_ms`
  (default 5000 = 5 s). Every frame in `[start, end)` auto-transitions to the
  next at play time after the delay (suppressed on the last frame). Created by
  the frame sub-menu's auto-advance action; widen the range to auto-advance a run
  of slides:

  ```json
  { "type": "auto_advance", "frames": { "start": 2, "end": 3 }, "delay_ms": 5000 }
  ```

- A `circle` is a filled circle drawn with a single character. `diameter` is its
  height in **rows** (default 10); the column extent is derived (~2× the
  diameter) so it looks round, and `ch` is the fill character (default `@`):

  ```json
  { "type": "circle", "position": { "x": { "fixed": 4 }, "y": { "fixed": 2 } },
    "diameter": 10, "ch": "@", "frames": { "start": 0, "end": 1 } }
  ```

## Dependencies

- `crossterm 0.28` — terminal raw mode, colors, cursor, events
- `serde` / `serde_json` — JSON serialization
- `anyhow` — error handling
- `libc` (unix, `tui` only) — process-group kill/`waitid` for `Command` objects
- `serde_json` is also a dev-dependency (integration tests author presentations as JSON)

## Tests

See `TESTS.md` for a per-file list of every test case.

Integration tests live in `tests/` (the editor/TUI is not unit-tested; coverage
targets the pure, deterministic core):

| File | Covers |
|------|--------|
| `tests/common/mod.rs` | Helpers: `render_json` (run a JSON presentation through `Engine::compile` + `Renderer::render`), `frame_lines` / `char_at` (reconstruct the visible char grid by replaying the full frame + diffs) |
| `tests/units.rs` | `Coordinate::evaluate` (fixed flooring, animation interpolation/clamping), `FrameRange` exclusivity, the number-or-object coordinate deserializer |
| `tests/pipeline.rs` | End-to-end: label placement, full-vs-diff frames, animation moving + clearing cells, z-order, exclusive frame ranges, off-grid clipping (incl. objects at the u16 edge, never wrapped) |
| `tests/table.rs` | Table layout math, `normalize_cells`, add/remove column rescaling, border/borderless/header rendering, height padding, `col_pixel_range`, a huge `rows` count |
| `tests/art.rs` | `Art` object: per-line placement, positioning, and space-transparency |
| `tests/list.rs` | `List` object: ordered/unordered markers, custom bullet, default vs custom inter-item spacing, wrapped-row indentation, a long first word kept on the marker row (also for an item starting with spaces), multi-digit alignment, height clip, background fill, a huge `spacing` |
| `tests/command.rs` | `Command` object: compiled `CommandRegion` spec (incl. following an explicit group range and an animated box), the placeholder box drawn into the static frame, and `player::layout_output` (escape/control-char strip, wide/zero-width chars to one column, tail + clip). The spawn/timeout run-loop is TUI and stays manual |
| `tests/label.rs` | `Label`: `framed` border (incl. at the canvas origin), `frame_style`, background fill + height pad, spaces opaque even without `bg`, height clip, width wrap (multi-digit `N. ` and nested `  - ` continuation indent), `align` (left/center/right within `width`) and `valign` (top/center/bottom within `height`), a huge bg-filled box |
| `tests/arrow.rs` | `Arrow`: horizontal/vertical/leftward body + auto head, diagonal L-routing, head-disabled, double-headed (`head_start` — outward heads at both ends, incl. custom-char rotation), zero-length point |
| `tests/hline.rs` | `HLine`: span (end-exclusive) and custom draw char |
| `tests/header.rs` | `Header`: glyph fill, custom fill char, inter-glyph spacing, canvas-width word wrap |
| `tests/rect.rs` | `Rect`: border + blank interior, title on the top edge (skipped on a rect too narrow for it; no z overflow at `i32::MAX`) |
| `tests/group.rs` | `Group`: members render independently / group emits nothing; auto range doesn't gate members; explicit range overrides members (narrows + widens) |
| `tests/looping.rs` | `Loop`: compiled `LoopRegion` sidecar (defaults + explicit fields) and `validate_loops` (disjoint OK; overlap/nesting/past-end/empty rejected). The auto-advance run-loop is TUI; the pure `loop_next` step fn is tested inline in `player/mod.rs` |
| `tests/animation.rs` | `Animation`: compiled `AnimationRegion` sidecar (defaults + explicit) and the loop/animation rules in `validate_loops` (animations may overlap; a loop must contain a whole animation or none of it — bisecting is rejected; duplicate animation ids are rejected). The auto-advance/min-delay run-loop is TUI; the pure `auto_advance_delay` is tested inline in `player/mod.rs` |
| `tests/autoadvance.rs` | `AutoAdvance`: compiled `AutoAdvanceRegion` sidecar (default 5 s delay + explicit delay/range) and that the marker draws nothing into the static frames. The play-time auto-advance run-loop is TUI; the pure `frame_auto_advance_delay`/`effective_auto_delay` step fns are tested inline in `player/mod.rs` |
| `tests/circle.rs` | `Circle`: filled-circle rendering — full-width central rows, narrower round caps, horizontal + vertical symmetry, custom fill char, hidden outside its frame range, and a huge diameter. The aspect helpers (`columns`/`rows_for_width`) are tested inline in `engine/objects/circle.rs` |
| `tests/morph.rs` | `Morph`: end-to-end blend — `from` on the first frame / `to` on the last, `wipe-right` half-done at the midpoint, horizontal wipes flipping a column together across ragged rows, smaller grid padded with transparent space, an explicit group range gating (not re-timing) it, hidden outside its range. The per-cell threshold/progress fns are tested inline in `engine/objects/morph.rs` |
| `tests/engine.rs` | `Engine::compile`: one scene per frame, empty deck, object outside `frame_count` |
| `tests/renderer.rs` | Renderer + `grid_at`: equal-z-order source order, clamp past end, out-of-bounds diff skip, ragged full frame normalised to the contract shape |
| `tests/docs.rs` | Docs ↔ code sync (see "Docs are load-bearing — and machine-checked" below): every test is listed in `TESTS.md` (and none is phantom), the documented test totals match, doc-referenced repo paths exist, the object-type lists match the `SceneObject` enum, every complete JSON example in the docs parses/compiles, `CLAUDE.md`/`GEMINI.md` stay pure `@AGENTS.md` imports, this file's Module Map covers every core module, and every `§N`/`#anchor` reference in `PRESENTATION_FORMAT.md` names an existing heading |

Inline unit tests also live in `src/` (e.g. `editor/properties.rs`,
`engine/objects/wrap.rs`, `editor/textedit.rs`, `editor/object_defaults.rs`,
`editor/state.rs` — frame copy/blank-insert/move/delete + `add_frames_and_share`
+ `ensure_animation`/`remove_animation`/`prune_orphan_animations` (id-based) +
`scene_object_animation_span` + `set_frame_auto_advance`/`frame_auto_advance_delay`
(add/replace/remove + remap through insert/delete), `player/mod.rs` — `loop_next`
bounce/wrap stepping + `auto_advance_delay` min-over-overlap + `animation_cluster`
overlap-merging + `frame_auto_advance_delay`/`effective_auto_delay`
(range coverage, last-frame suppression, min over markers + animations) +
`Command` process handling (timeout and lingering grandchildren never block; group kill;
bounded output buffer `append_output`);
copy/paste `expand_selection` +
`clone_selection` + `link_siblings` + link-family delete maintenance +
`delete_objects` multi-delete (plain highest-first + `Animation` by id) +
cross-deck `copy_frame_block`/`paste_frame_block` (block-local range normalisation,
fresh `Animation` ids with no collision, group-member repoint, flatten of an
animation outside the block);
`editor/input.rs` — `apply_animation`/`apply_converge` id reuse + the
"editing a span never duplicates the animation" regression;
`editor/timeline.rs` — `pick_indices`/`abbreviated_indices` (first-3 / current
window / last-3 selection, dedup near the edges, and edge-group shrink on a
narrow row)). The suite totals 469 tests (156 integration + 313 inline);
`TESTS.md` is the authoritative per-test list, and `tests/docs.rs` fails the
build if this count line or `TESTS.md` drifts from the code.

Pattern: write a presentation in the documented JSON format, render it, and
assert on the reconstructed grid — so tests pin behavior without coupling to the
editor. Expected geometry is hand-derived from the layout spec, not snapshotted.

## Docs are load-bearing — and machine-checked

The markdown docs here are the interface other agents act on, so drift is a
bug, not a cosmetic issue. `tests/docs.rs` (part of `cargo test`) enforces the
sync rules mechanically: `TESTS.md`'s per-test list and totals (and the count
line above) match the code, doc-referenced repo paths exist, the object-type
lists in this file/`PRESENTATION_FORMAT.md`/the bs-deck skill match the
`SceneObject` enum, every complete JSON example in the docs — including the
packaged skill at `.claude/skills/bs-deck/SKILL.md` — still parses (full
decks also compile and render), `CLAUDE.md` and `GEMINI.md` stay one-line
`@AGENTS.md` imports (so no second copy of this guide can drift), this file's Module Map covers
every core module, and `PRESENTATION_FORMAT.md`'s `§N`/`#anchor` references
name existing headings. Each failure message names the doc and line to fix. The
rules it can't check remain manual: if you change behavior, update the
matching doc in the same edit.

## Web tools (`web/` → GitHub Pages)

A dependency-free installable web app — no framework, no bundler, no npm. **One
page (`web/index.html`) with a title view plus three tool views in order:
instructions → compile → present**, so the WebAssembly engine, the loaded deck
and the library are shared state and compiling then presenting reloads nothing.
The `bs` wordmark is the title page's tab (`#tab-home`) — a single paragraph
saying what the tool is and how to use it, with the view names as inline links.

The topmost bar carries the wordmark, the tabs and the present view's own
**library** / **open** actions (`#bar-actions`, hidden on the other views).
There is deliberately no second bar showing the deck name — the document title
carries it instead. Because everything lives in that one bar, it is tight on a
phone: the labels shorten to `lib` / `open` under 430px and the type tightens
under 400px, sized so the bar still fits a 320px viewport without clipping.

| Module | Does |
|--------|------|
| `web/app.js` | The shell: view routing (hash), the deck-library UI, drag-drop routing, service-worker registration |
| `web/instructions.js` | Serves `PRESENTATION_FORMAT.md` verbatim with a copy-all button; also offers the packaged Claude skill as a download |
| `web/compile.js` + `web/wasm.js` | Turns a **source** deck into a playable one by running the real engine, built to WebAssembly. Opening a file compiles it immediately; the compiled JSON is never displayed |
| `web/viewer.js` | Plays a **compiled** deck on a `<canvas>`. A JS port of the player's pure logic (see the drift warning below) |
| `web/store.js` | The deck library in IndexedDB, each deck gzip-compressed (`CompressionStream`) and inflated on open — 100 most recent within 50 MB stored, database named after the deployment path, quota-aware, same-name replaces; adopts (then frees) the `localStorage` library earlier builds kept. Async throughout, never throws |
| `web/sw.js` + `web/manifest.webmanifest` | PWA: installable, and fully usable offline including compiling. `scripts/build-web.sh` stamps the cache name with a build hash so an active build never serves another build's files; a new build waits (no `skipWaiting`) until no tab uses the old worker, so a page it controls never gets the new build's files under its old engine |

Build and test it with the two scripts in "Build & test" above — never
hand-assemble the site. `scripts/build-web.sh` produces `_site/` (gitignored): `web/` plus the built
`bs.wasm`, `PRESENTATION_FORMAT.md` (as `presentation-format.md`),
`.claude/skills/bs-deck/SKILL.md` (as `bs-deck-skill.md`) and
`examples/demo.json` (as `demo.json`, the compile page's sample). CI runs the
**same script**, so a green local build is what deploys.

**The compile tool cannot drift — by construction.** `wasm/src/lib.rs` is a
~90-line `extern "C"` shim (no wasm-bindgen, no wasm-pack, no Node) over
`bs::compile::compile_json` — the same function `bs compile` calls. Its ABI is
raw pointers into the module's exported `memory`; the JS side (`web/wasm.js`)
re-reads `exports.memory.buffer` after every call because allocation can grow
and detach it, and drops the instance after a trap (`withCompiler`; the module
aborts on panic or out-of-memory, leaking that call's heap and stack), so the
next compile starts fresh. `scripts/test-web.sh` compiles one deck both ways in a headless
browser and asserts the outputs are **byte-identical**, which is what catches an
ABI or glue mistake (an engine change moves both sides together, so the
deck-shape assertions in that script cover engine regressions).

**The grid is drawn on a canvas, one `fillText` per cell.** Never render the
deck as runs of text: a run only lines up if the device resolves a *truly*
monospaced font that also has a glyph for every character in the deck. On mobile
neither holds — the platform "monospace" can be a proportional face, and
box-drawing characters (`─ │ ┌ █`) routinely come from a fallback font with
different metrics. Either shears the grid, most visibly as spaces narrower than
everything else. `viewer.js` therefore measures the resolved font once (`measure`),
derives a cell size (`layout`, device-pixel-ratio aware), and paints each cell at
its exact origin (`paint`) — so alignment is independent of the font. Characters
in the box-drawing block are stretched to the full cell width so long runs tile
without gaps. The empty/error state is a separate `#msg` layer, since a canvas
cannot show text on its own.

**The frame bar has one tick per step, and buckets past the width it can
draw.** A step (`motionRuns`) is a frame, or a whole motion run — every
animation range, auto-play or not, overlap-merged, joined with runs of quick
auto-advances (markers of at most `MOTION_DELAY_MS` = 1 s, how generated decks
animate; slower markers are timed slides and stay separate) — so an animation
is one marker, not one per frame. One `<i>` per step only works
while each tick has a pixel to occupy; beyond that the bar overflows, widens the
page, and the resulting horizontal scrollbar shrinks `#stage` — so a long deck
rendered *smaller* than a short one. `renderFrameBar` therefore caps the tick
count to what fits (`frameBarBuckets`), each tick standing for a run of steps
(`ticks`, `[lo, hi)` frame ranges); seeking and region marking work off the range. `#framebar`
also carries `overflow: hidden` so the bar can never widen the page even if the
maths is wrong.

**Nothing in the viewer is keyboard-only.** Every key has a tap target, because
a phone has none: the footer holds real buttons — first/last/fullscreen at the
left, prev/next at the right edge under a right thumb, all 48px (20% over the
desktop 40px) on phones and touch screens — plus a small slide counter, tapping
the canvas steps, and the frame
bar jumps. Fullscreen (`body.bare`) hides **everything** — both bars, and with
them the deck name, the status note and the library panel — because any of it
overlaying a slide is a distraction while presenting; the deck name lives only in
the document title from there on. The single piece of chrome that survives is a
tiny, faint `✕` pinned top-right (`#exit-full`), padded to a real tap target
without the glyph growing — on iPhone `requestFullscreen` is refused
for non-video elements, so `toggleFullscreen` treats the bars-hidden state and
real browser fullscreen as separate concerns and always leaves a way back.

**`→` on an animation stops on the animation's own last frame** (`cluster[1] - 1`),
its finished state, rather than the first frame past the span; already there, it
falls through to a normal step so the span cannot trap the deck. This is a
deliberate divergence from `src/player/mod.rs`, which still jumps to
`hi.min(last)`. The same `→` rule covers a quick auto-advance run (`movesOn`).
**`←` inside any motion run** — an animation, auto-play or stepped by hand, or
a quick auto-advance run — goes to the slide right before it (`runAt`); the
terminal player does that only for auto-play animations, and has no notion of
a quick auto-advance run.

**The present tool *is* a port, so it can drift.** `web/viewer.js` re-implements
the player's *pure* logic in JS — `PlayablePresentation::grid_at` → `gridAt`,
`player::loop_next` → `loopNext`, `player::loop_step` → `loopTick`'s pass
counting, `Player::loop_wait` → `loopWait`, and `Player::{auto_advance_delay,
animation_cluster, frame_auto_advance_delay, effective_auto_delay}` → the
same-named camelCase functions, and `Cell::printable`/`narrow_char`
(`src/types.rs`) → `printable`/`narrowChar` (so a cell paints the same char as
in `bs play`) — plus the arrow-key semantics (animation/loop
skip, Shift+arrows = ±10). **If you change any of those in `src/player/mod.rs`
or `src/types.rs`, mirror it in `web/viewer.js` in the same edit**; `cargo test`
cannot catch this.

`Command` objects are the one runtime feature the browser cannot provide (they
run a local binary): the compiler has already baked their placeholder box into
the static frames, so the slide still renders and the viewer names the command
in the status bar instead of executing it.

**Feature gating.** `crossterm` is optional, behind the default-on `tui`
feature; `src/lib.rs` gates `editor`/`player`/`menubar`/`term` on it and the `bs`
binary declares `required-features = ["tui"]`. That is what lets
`--no-default-features` build the engine for `wasm32-unknown-unknown` (hence
"Keep the engine wasm-clean" under Conventions above).

## The packaged Claude skill

`.claude/skills/bs-deck/SKILL.md` is a Claude Code skill (frontmatter `name:
bs-deck`) that teaches an assistant to author `bs` source JSON. Living at the
conventional path, it loads automatically for anyone using Claude Code in this
repo; the instructions page also offers it for download so it can be installed
elsewhere.

It restates the source format, so it rots like any other doc — it is therefore
in `tests/docs.rs`'s `DOCS` list, which parses every JSON example in it against
the serde structs and compiles the complete decks. **If you change the format,
update the skill too**, exactly as you would `PRESENTATION_FORMAT.md`.

## Shared helpers (extend these; don't fork a parallel copy)

- Word-wrap: `engine::objects::wrap` (`wrap_line_indexed` + `indexed_to_chars`)
  — used by `Label`, `List`, and `Table` so glyphs and source indices can't drift.
- Frame replay: `PlayablePresentation::grid_at` (`src/types.rs`) — the player
  (`rebuild_grid`), the editor preview, and the test harness (`frame_lines`)
  all replay full+diff frames through it.
- Text carets: `panel.rs::draw_caret_line` — every text field renders its
  caret through it (see "Text caret convention" above).
- Printing text in the editor UI: `types::printable_text` — every panel,
  menu-bar, status-row and text-field `Print` of non-literal text goes through
  it (control chars → `·`, one char per column), so a deck string can never
  send an escape sequence to the terminal; the canvas uses `Cell::printable`.
- Object properties: the `Editable` trait (`src/editor/properties.rs`) — one
  impl per type; the panel/menu/dispatch code is type-agnostic.
- Adding an object type: follow the checklist in the module doc of
  `src/engine/objects/mod.rs`; the compiler only catches some touch sites.

## Known maintainability debt (not yet done)

- The `Mode` FSM (~30 variants, some with 7–15 fields) grows with every object
  type and editor feature.
- `panel.rs::render_right_panel` is one large multi-mode renderer. The caret
  rendering is shared (`draw_caret_line`), but the list-row and dropdown render
  patterns are still duplicated inline.
- The `Editable` impls repeat near-identical `set()` arms and geometry
  accessors for the common x/y/width/height/style/frame fields.
