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
cargo run -- edit source.json [more.json …] # interactive editor (one or more decks)
cargo run -- play out.json                  # play compiled presentation
cargo run -- migrate source.json            # upgrade an old source file in place (writes source.json.bak)
```

`bs edit` accepts **multiple files** — each opens as a parallel *deck*. Switch
between them, open more, and copy frames across decks from the **[p]resentations**
hub (see "Multiple presentations" below).

## Architecture

Three-stage pipeline with clean separation:

```
SourcePresentation (JSON)
  → Engine::compile()     → Vec<ResolvedScene>  (DrawOps per frame)
  → Renderer::render()    → PlayablePresentation (Frame::Full / Frame::Diff)
  → Player::play()        → terminal output
```

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
instead of opening a duplicate), `CopyFrameBlock{lo,hi}`, `PasteFrameBlock{target,
before}`. Before each redraw the `Editor` mirrors a read-only `WorkspaceView`
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
dropped), `Animation` ids block-local, and any coordinate driven by an animation
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
propagate). The animate flow's `add_frames` toggle inserts the spanned frames and
**shares** the current frame's elements across them (one range-extended object per
element ⇒ editing one edits all), and X and Y of an object share **one**
`Animation` by *reference* (reuse-by-id: `apply_animation` reuses the coordinate's
existing `anim` id, allocating a fresh one only for a brand-new animation;
`state::ensure_animation` sets the span + config; `state::next_anim_id` allocates).
An animation has **two halves** — the motion (`Coordinate::Animated` on the driven
objects) and the `Animation` object — so removal cleans up both: **deleting the
`Animation` object** reverts every coordinate it drives back to a static `Fixed`
(at its `from`), widens those objects to span the range statically, drops their
gap-strobe copies, and deletes the object (`state::remove_animation`, by id). The
in-menu `[x]` revert and every apply run `state::prune_orphan_animations`, which
drops any `Animation` no coordinate references — so a span edit or a zeroed-out
motion can never leave an orphan. Resolution threads the id→span table through the
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
The presenter can still navigate manually at any time. An `AutoAdvance` is created
from the **frame** sub-menu's auto-advance action (`t`, configurable
`frame_auto`), *not* the Add-Object menu — so, like `Animation`, it is absent from
`OBJECT_TYPES`. The action opens `Mode::FrameAutoInput` (type the delay in
seconds; `0`/empty turns it off), seeded with the current frame's delay if it
already auto-advances; `state::set_frame_auto_advance` adds/replaces/removes the
single-frame marker and `state::frame_auto_advance_delay` reads it. Being an
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
| `src/main.rs` | CLI entry point (`compile`/`edit`/`play`/`migrate`) |
| `src/migrate.rs` | One-shot upgrade of old-format source JSON to the current animation model — works on the raw `serde_json::Value` (the current structs can't parse the old shape), assigns `id`s to `animation` objects, rewrites `{"animated":{…,start_frame,end_frame}}` coords to `{from,to,anim}` by span match (synthesizing a sidecar for orphan spans). Idempotent; self-verifies through `SourcePresentation` before writing in place (`<file>.bak` backup) |
| `src/types.rs` | Shared types: `Color`, `Style`, `Cell`, `DrawOp`, `Frame`, `PlayablePresentation`, `CommandRegion`, `LoopRegion`, `AnimationRegion`, `AutoAdvanceRegion` |
| `src/engine/source.rs` | `SourcePresentation` (+ `command_regions()`, `loop_regions()`, `animation_regions()`, `auto_advance_regions()`, `validate_loops()`, `link_siblings()`, and a `links` sidecar — editor-only families of object indices for *linked* paste, ignored by the engine), `SceneObject`, `Coordinate` (Fixed / Animated{from,to,anim}), `AnimId` + `AnimSpans` (the id→span table; `Coordinate::evaluate(frame, &AnimSpans)` looks a coordinate's span up there), `FrameRange` |
| `src/engine/objects/` | Fifteen `SceneObject` types: `Label`, `HLine`, `Rect`, `Header`, `Group`, `Arrow`, `Table`, `Art`, `Command`, `List`, `Loop`, `Morph`, `Animation`, `AutoAdvance`, `Circle` — each implements `Resolve`. See the module-doc checklist in `mod.rs` for every site a new type touches. `List` (ordered/unordered) shares `Label`'s text-editing UX and the shared `wrap` helper. `Loop` (like `Group`) draws nothing; its `frames` range is the loop range and it emits a `LoopRegion` sidecar. `Morph` blends two inline ASCII-art grids (`from`→`to`) across its `frames` range — each cell flips to the `to` glyph once playback progress passes that cell's per-cell threshold (`MorphMode`: `dissolve` or four directional wipes). Fully baked into static frames in `resolve`, so the editor preview shows it for free. `Animation` (also draws nothing) **owns** the animation span (its `frames`) — the single source of truth — plus an `id` that driven `Coordinate::Animated { anim }` fields reference; it emits an `AnimationRegion` sidecar and is created by the animate sub-menu, not the Add-Object menu. `AutoAdvance` (also draws nothing) makes its `frames` auto-transition to the next slide after `delay_ms` (default 5 s); it emits an `AutoAdvanceRegion` sidecar and is created by the **frame** sub-menu's auto-advance action — so `Animation` and `AutoAdvance` are the two types absent from `OBJECT_TYPES`. `Circle` is a **parametric** filled circle (unlike the static `Art` pieces): editable `diameter` (rows) + fill `ch` (default `@`), with the column extent derived from the diameter (`Circle::columns`, ~2× for the terminal's 2:1 cell aspect) so it stays round; added from the **Add-Object** menu (quick-add `o`) and baked into static frames in `resolve`. Each type implements `Resolve::resolve(&ResolveCtx, ops)` (the `ResolveCtx` carries `frame`, `canvas_width`, and the `&AnimSpans` table) |
| `src/art_library.rs` | Built-in + user ASCII-art palette (`~/.config/bs/art/`, one file per piece); pieces are copied into self-contained `Art` objects when added. Includes a matched `ball`/`square` pair used as the default `Morph` endpoints. The picker (`Mode::AddArt`/`LoadArtFile`) carries an `ArtPick` purpose so the same flow serves a standalone `Art` or the two-stage `from`/`to` pick of a `Morph` |
| `src/renderer/mod.rs` | Rasterizes DrawOps into cell grid; diffs frames |
| `src/player/mod.rs` | Playback loop, keyboard nav (arrows, Shift+←/→ jump ±10 frames, space, q, f=fullscreen); runs `Command` objects (piped, async, timeout) and overlays output; drives `Loop` regions (timer-based auto-advance + bounce + arrow-key break-out) via the pure `loop_next` step fn; auto-advances across auto-play `Animation` spans (`auto_deadline`), using `auto_advance_delay` = the **min** `delay_ms` over the animations covering each boundary, with the loop's own delay as the fallback for gaps inside a loop. On an auto-play animation (no loop), an arrow **skips** the whole span: `→` jumps to the first frame past the last-ending overlapping animation (clamped to the last frame), `←` to the slide before the earliest-starting one — the merged cluster comes from `animation_cluster` (connected by overlap). Also auto-advances across `AutoAdvance` regions: `frame_auto_advance_delay` is the **min** `delay_ms` over the markers covering a frame (None on the last frame), and `effective_auto_delay` = the min of that and the animation boundary delay, feeding the same `auto_deadline` timer (suppressed while a loop drives) |
| `src/editor/mod.rs` | Editor lifecycle, raw mode setup, main loop. Holds **multiple decks** (`Vec<EditorState>` + `active`) and the cross-deck `frame_clip`; interprets the cross-deck `Action`s (`SwitchDeck`/`OpenDeck`/`CopyFrameBlock`/`PasteFrameBlock`), mirrors a `WorkspaceView` into the active deck (`sync_workspace_view`), and gates quit on any-deck-dirty (`handle_quit`). `open_many(&[String])` opens N files; `open(&str)` wraps it |
| `src/editor/state.rs` | `EditorState` (incl. `clipboard` + `clipboard_sources` for copy/paste), `Mode` enum (~30 variants, incl. table sub-modes, art picker, frame sub-menu/move/overlay/jump/select/auto-input/range-place, `MultiSelect` (`MultiSelectPurpose::Group`/`Select` — copy/converge/delete/edit-props then come from the `SelectAction` sub-menu), `SelectAction` (the post-multi-select action sub-menu), `EditMultiProperties` (bulk-edit the shared props of a selection), `ConvergeConfig`, `PastePlacing`). Frame ops: `insert_blank_frame` + `insert_blank_frames_at` (N-frame generalisation), `copy_frame` (deep-clone duplicate into a *new* frame), `overlay_frame` (deep-clone paste onto an *existing* frame, no new frame), `move_frame`/`move_frames` (relocate one frame or a block — both via the shared `remap_ranges_through_pos` permutation), `copy_frames` (duplicate a contiguous block as new frames), `parse_frame_selection` (`1,2,3`/`5-12` → indices) + `delete_frames` (multi-delete, highest-first, keeps ≥1). Copy/paste helpers: `expand_selection` (pull in a group's members), `clone_selection` (self-contained deep clone with selection-local member remap). Cross-deck frame clipboard: `FrameClipboard` + `copy_frame_block` (capture a contiguous block, normalised to be deck-independent) + `paste_frame_block` (insert into another deck with fresh `Animation` ids + group/range remap). New modes `PresentationMenu`/`OpenFile`/`FramePastePlace`; `WorkspaceView` (Editor-mirrored deck list / active / frame-clip length, read by the menu bar + switcher panel). Object delete fixes both `Group.members` and `links` families (`adjust_group_members_after_delete`) as objects are pruned; `delete_objects` deletes a multi-selected set at once (plain objects highest-first, `Animation`s by id via `remove_animation`) |
| `src/editor/config.rs` | `KeyBindings` — all bindings configurable via `~/.config/bs/editor.json`. `matches_binding` parses `Ctrl-`, `Alt-`, and `Ctrl-Shift-` prefixes (the last requires keyboard-enhancement to be distinguishable). Single-letter bindings are **shift-aware**: a capital `S` matches Shift+S however the terminal encodes it (`Char('S')`±SHIFT or `Char('s')+SHIFT`), and a lowercase letter never fires on a shifted press — so capital-letter shortcuts like `S`=save-as / `F`=fullscreen work across terminals |
| `src/editor/input.rs` | All key event handling. Property browse/edit/dropdown flows (object + table cell-style) share helpers: `TextEdit` (text fields), `dropdown_key`/`DropdownKey` (list nav), and the `ep_*` `Mode::EditProperties` constructors |
| `src/editor/textedit.rs` | `TextEdit` — reusable text-buffer + cursor used by every text field (property values, the multi-line overlay, cell-style values); translates key events into edits (insert/delete/arrows/home-end/newline) |
| `src/editor/panel.rs` | Left panel (Add Object), right panel (Properties incl. `Bool` checkboxes + colour swatches), object selection overlay, and the centred multi-line text-editing overlay (`render_text_overlay`). Every text field draws its caret through one shared helper, `draw_caret_line` (see "Text caret convention" below) |
| `src/editor/properties.rs` | `Editable` trait — one impl per object type holds its property list, setter, coordinate + geometry accessors; generic dispatch (`get_properties`, `set_property`, `common_properties` = the intersection of bulk-editable props across a selection, …) is type-agnostic. `PropertyKind::Bool` flags toggle in place (Space/Enter); `PropertyKind::Note` renders a non-editable free-form warning line (the whole `value`, no `name:`) — the mechanism for surfacing per-object warnings in the panel |
| `src/editor/preview.rs` | Canvas preview using Engine+Renderer |
| `src/editor/timeline.rs` | Frame bar (row 1) and mode/status line (row 2). The frame bar is always shown; while typing a `FrameJump`/`FrameSelectInput`, it live-highlights the slides the input resolves to and the typed field + instructions render on row 2. Frames under an auto-play `Animation` collapse into a single range cell (`[10-20]`); strictly-overlapping auto-play spans merge into one range (continuous auto-advance), adjacent-but-disjoint ones stay separate. When the bar overflows the row it abbreviates to the **first 3** segments, a 3-wide window around the current frame, and the **last 3** (with `...` for skipped gaps); the edge groups shrink 3→2→1 only when the row is too narrow (`abbreviated_indices`/`pick_indices`) |
| `src/editor/menubar.rs` | Context-sensitive menu bar |
| `src/editor/ui.rs` | Layout computation |

## Editor Mode FSM

```
Normal ──a──→ AddObject ──Enter──→ Normal (object added)
       ──s──→ MultiSelect{Select} ──Enter(1 obj)──→ SelectedObject ──e──→ EditProperties ──a──→ AnimateProperty
                                  ──Enter(2+ obj)─→ SelectAction (Copy / Converge / Delete / Edit Props)
       ──p──→ PresentationMenu ──Enter──→ (switch deck) │ ──o──→ OpenFile │ ──s/g/f──→ SaveAs/Settings/fullscreen
```

- **Normal**: frame navigation (←/→, and Shift+←/→ to jump ±10 frames clamped — `FRAMES_PER_JUMP`), `f` opens the frame sub-menu, `p` opens the **[p]resentations** hub, Ctrl-s save, q quit. **Save-as** (`Mode::SaveAs` → `state::save_as`, adopting the new path), **settings** (frame size) and **fullscreen** moved into the presentations hub and are no longer on Normal's menu bar — their global keys (`Shift+S`, `g`, `Shift+F`) still fire (capital `S`/`F` work on every terminal; `Ctrl-Shift-s` was undetectable without keyboard-enhancement). `s` enters **Select** (multi-select; copy & converge live in its action sub-menu — see below)
- **PresentationMenu** (`p`, the presentations hub): lists every open deck (active marked `●`, dirty marked `*` in the name) in the right panel; ↑/↓ move the cursor, **Enter** switches the active deck (`Action::SwitchDeck`). `o` → **OpenFile** (open another deck), `s` → **SaveAs**, `g` → **Settings**, `f` → fullscreen, Esc back. The deck list is read from the Editor-mirrored `state.workspace`.
- **OpenFile** (from the presentations hub via `o`): a path prompt (panel input, reuses `frame_text_key` + `draw_caret_line`). Enter → `Action::OpenDeck(path)`, which the Editor opens as a new deck (or focuses if already open); Esc returns to the hub.
- **FrameMenu**: frame operations — `a` add blank frame, `c` copy (duplicate) current frame, `o` overlay (paste) current frame's objects onto another existing frame, `j` jump to a frame by number (`FrameJump`), `s` select multiple frames (`FrameSelectInput` → `FrameSelected`), `t` auto-advance the current frame after a delay (`FrameAutoInput` → an `AutoAdvance` marker), `d` delete current frame (with confirm), `m` move current frame, `p` **paste frames** from the cross-deck frame clipboard (shown only when it holds frames → `FramePastePlace`), Esc back. Both input modes keep the frame bar (slide range indicator) on its own row and put the typed field + instructions on the mode/status row beneath it, live-highlighting the slides the input resolves to. **FrameJump** types a 1-based frame number (previewing the target slide); Enter jumps the deck there (clamped). **FrameSelectInput** types a list/range (`1, 2, 3` or `5-12`, mixable, `state::parse_frame_selection`); Enter → **FrameSelected**, which highlights the chosen frames in the timeline and offers `d` to delete them all (`state::delete_frames` removes highest-index-first and always keeps ≥1 frame). For a **contiguous** range it also offers `m` move and `c` copy → **FrameRangePlace** (see below), and `y` to **yank the block to the cross-deck frame clipboard** (`Action::CopyFrameBlock` → `state::copy_frame_block`) for pasting into another open deck. **FrameAutoInput** types the auto-advance delay in **seconds** (default 5, `0`/empty = off), seeded with the current frame's delay if it already auto-advances; Enter calls `state::set_frame_auto_advance`, which adds/replaces/removes a single-frame `AutoAdvance` marker (stored as `delay_ms`). The single-frame ops: `add` calls `state::insert_blank_frame` (the "make room" primitive — a new empty frame). `copy` calls `state::copy_frame`, which inserts a blank frame and then **deep-clones** every object on the source frame onto it, so the copy's objects are independent of the original (editing one never changes the other). Deck-wide/spanning objects stay shared (extended across the new frame) rather than cloned, so they remain a single continuous object
- **FrameOverlay**: paste the current (source) frame's objects *on top of* another existing frame, **without** inserting a new frame. ←/→ scroll the deck to a target frame; Enter calls `state::overlay_frame`, which **deep-clones** every object on the source frame onto the target (same positions/styles/z-order), appended after the target's existing objects so they render over it. Objects already visible on the target (e.g. a deck-wide background spanning both frames) are skipped rather than duplicated. Unlike copy/move, the deck's `frame_count` is unchanged
- **FrameMove → FrameMovePlace**: relocate the current slide. In FrameMove, ←/→ scroll the deck to a target slide; Enter opens FrameMovePlace, where Enter drops the moved slide *after* the target and `b` drops it *before* (`state::move_frame` remaps object ranges through the new frame ordering)
- **FrameRangePlace**: place a moved or copied **contiguous** frame block (reached from FrameSelected via `m`/`c`; the block must be contiguous — a scattered selection is rejected). ←/→ scroll the deck to a target slide; `Enter` drops the block *after* it, `b` *before* it (the `copy` flag picks the verb). **Move** calls `state::move_frames` (pure reorder; the target may not lie inside the moved block). **Copy** calls `state::copy_frames`, which inserts `count` new frames at the destination (`insert_blank_frames_at`) and deep-clones the block's content onto them — per-frame objects land on their copy frame, objects spanning within the block stay single spanning clones, and a deck-wide background the insert already stretches over the new frames is *not* re-cloned. The deck lands on the first frame of the result
- **FramePastePlace** (reached from the frame sub-menu's `p` paste-frames action, only when the cross-deck frame clipboard is non-empty): ←/→ scroll the deck to a target slide; `Enter` drops the pasted block *after* it, `b` *before* it (`Action::PasteFrameBlock` → `state::paste_frame_block`, which inserts the frames, shifts ranges/group-members into the destination, and assigns each cloned `Animation` a fresh id so it can't collide with the target deck's). The frame clipboard lives on the `Editor`, so it persists across deck switches and re-pastes
- **Settings**: edit the output frame size (width × height in cells); ↑↓/Tab switch field, Enter apply, Esc cancel
- **AddObject**: choose object type from the list (↑/↓ + Enter) or press its **quick-add shortcut** — one unique letter per type, shown as `[l] Label` and defined by `object_defaults::OBJECT_TYPE_KEYS` (`object_type_for_key` maps a keypress to the type). Either path runs the shared `commit_add_object`. After committing, most types land in `EditProperties` (browse); `Group`/`Art` enter their member/library pickers; `Morph` runs the art-library picker **twice** (pick the `from` piece, then the `to` piece) before landing in `EditProperties`; `Label` and `List` jump straight into the centred multi-line text overlay (empty buffer) so you can type content immediately — Esc keeps the default text, Enter commits
- **Select** (`s`, the single entry point): a **multi-select** reusing the `MultiSelect` toggle flow (`MultiSelectPurpose::Select`). `Space` toggles members (the cursor object is highlighted on the canvas; a `Group` expands to its members), `d` deletes the highlighted object (the old browse-and-delete), `Enter` **acts** on the chosen set (toggled members, or the highlighted object if none toggled): **1 object → `SelectedObject`** (its move/resize/edit/delete/copy menu), **2+ objects → `SelectAction`**. There is no longer a separate single-pick `SelectObject` mode.
- **SelectAction**: the action sub-menu shown after selecting 2+ objects (`SELECT_ACTIONS`, ↑/↓ + Enter). Currently **Copy** (`copy_to_clipboard`), **Converge** (`expand_selection` → `enter_converge`), **Delete** (confirm → `state::delete_objects`, removing the whole selected set at once), and **Edit Props** (bulk-edit the shared properties → `EditMultiProperties`). Copy & converge moved here from their old top-level `c`/`Shift+C` keys; delete is the multi-object counterpart to `SelectedObject`'s single `d`.
- **EditMultiProperties** (reached via **Select → SelectAction → Edit Props**): bulk-edit the properties **common** to every selected object. The panel lists only the props all members share by name *and* kind, restricted to the bulk-editable kinds (`properties::common_properties` — geometry/colour/flags/numbers/simple dropdowns; `Text`, group-member, table-column, read-only/note are excluded). Values shown are the **first member's** (the representative seed). Editing one value writes it to **every** member: `input::apply_multi_property` just calls the single-object `apply_property` per member, so group auto-range, animation re-locking, link propagation, and loop validation all behave exactly as for a single edit. The handlers (`handle_edit_multi_properties`/`_value`/`_dropdown`, `emp_*` constructors) are slim cousins of the `EditProperties` ones — no animate/table/group-member/multi-line-text path, since those kinds never enter the common set. `Esc` returns to `SelectAction` with the selection intact.
- **Copy/paste** (`v` paste, configurable; copy is reached via `SelectedObject`'s `c` for one object or the **Select → SelectAction → Copy** sub-menu for many): **copy** captures objects to `EditorState.clipboard` as self-contained deep clones — either one object (`c` in `SelectedObject`) or a `MultiSelect{Select}` toggle set (via the action sub-menu); a copied `Group` pulls in its members (`expand_selection`). **Paste** is not a standing top-level command: the `v` binding works in Normal and `SelectedObject`, but the **menu only surfaces `[v] paste` once the clipboard is non-empty** (so it appears right after a copy and stays visible while you navigate to the target frame, then disappears once consumed). **Paste** (`v`) enters `PastePlacing`: clones land on the current frame (re-anchored to it, animated coordinates flattened to `Fixed` at that frame via `state::flatten_coordinates` so the copy is static and arrow-nudgeable, then nudged off the source) as a movable **ghost** that rides the arrow keys; **Enter** drops the set and re-arms a fresh ghost (rubber-stamp loop — stamp N copies), **Esc** discards the un-dropped ghost and finishes. `l` toggles **Independent** vs **Linked**: a *linked* paste records one `links` family **per clipboard object** (its source + each stamp's clone of it), so editing a non-placement property of any member propagates to its siblings (`apply_property` → `SourcePresentation::link_siblings`; placement = `x/y/width/height/first_frame/last_frame/z_order` stays per-copy). Distinct objects copied together never cross-sync. The ghost clones live in `objects` (tail indices in `pending`), so the WYSIWYG preview shows them; Esc truncates that tail
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
- **SelectedObject**: move (arrows), `r` → resize mode, `e` → edit props, `d` delete; Shift+arrows also grow
- **ResizeObject**: arrow-key resize (←→ width, ↑↓ height) — a terminal-robust path since many terminals capture Shift+↑/↓ for scrollback; Enter/Esc exit
- **EditProperties**: edit typed properties; color fields show dropdown; text fields support multi-line (Alt-Enter = newline); property list scrolls vertically
- **AnimateProperty**: a role-based field list (`input::anim_roles`/`AnimRole`).
  Animating `x` or `y` on an object that has **both** becomes a *two-axis* session
  — fields `x from`/`x to`/`y from`/`y to` so x and y are set together; every other
  coordinate (width/height, or `y` on an `HLine`) stays single-axis with `from`/`to`.
  Then `start`/`end`, `add frames`/`auto play` (toggles, Space/Enter), `delay ms`,
  and `gap frames`. `enter_animate` seeds every value from the object's current
  coordinate(s), so an untouched axis is preserved on apply (a `Fixed` stays fixed,
  an `Animated` stays animated). `[s]` applies via `input::apply_animation`:
  it reuses the coordinate's existing `anim` id (or allocates a fresh one via
  `state::next_anim_id` for a brand-new animation), optionally inserts the spanned
  frames + shares the current frame's elements (`state::add_frames_and_share`),
  records the span + config on the `Animation` (`state::ensure_animation`), sets
  the `Coordinate::Animated { anim }` on each moving axis (`from != to`), and
  re-locks the object's own range to the union of its animations'
  spans (`scene_object_animation_span(obj, &anims)`). Re-applying never spawns a
  second animation (same id updated in place); `state::prune_orphan_animations`
  drops the animation if no axis actually moved.
  `gap frames` > 0 then strobes the element via `state::apply_gap`: `gap frames`
  is the count of *empty* frames between appearances, so the element shows every
  `gap + 1` frames of the span (single-frame samples at the interpolated position,
  `gap` blank frames between — a stop-motion look; `gap = 3` ⇒ frames `start`,
  `start + 4`, `start + 8`). It works on whatever frames the span covers,
  **independent of `add frames`** (inserted or pre-existing). Re-applying is
  idempotent: `apply_animation` first calls `state::clear_gap_clones` to remove
  the element's *own* prior strobe copies (matched by whole-object content, so an
  overlapping animation with the same motion is left intact), then re-strobes.
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
// threaded through Resolve via ResolveCtx { frame, canvas_width, anims }.

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
  `"magenta"`, `"cyan"`, `"white"` — the 8 `NamedColor`s only) or `{ "rgb": [r, g, b] }`
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
| `tests/label.rs` | `Label`: `framed` border (incl. at the canvas origin), `frame_style`, background fill + height pad, height clip, width wrap, `align` (left/center/right within `width`) and `valign` (top/center/bottom within `height`) |
| `tests/arrow.rs` | `Arrow`: horizontal/vertical/leftward body + auto head, diagonal L-routing, head-disabled, double-headed (`head_start` — outward heads at both ends, incl. custom-char rotation), zero-length point |
| `tests/hline.rs` | `HLine`: span (end-exclusive) and custom draw char |
| `tests/header.rs` | `Header`: glyph fill, custom fill char, inter-glyph spacing, canvas-width word wrap |
| `tests/rect.rs` | `Rect`: border + blank interior, title on the top edge |
| `tests/group.rs` | `Group`: members render independently / group emits nothing; auto range doesn't gate members; explicit range overrides members (narrows + widens) |
| `tests/looping.rs` | `Loop`: compiled `LoopRegion` sidecar (defaults + explicit fields) and `validate_loops` (disjoint OK; overlap/nesting/past-end/empty rejected). The auto-advance run-loop is TUI; the pure `loop_next` step fn is tested inline in `player/mod.rs` |
| `tests/animation.rs` | `Animation`: compiled `AnimationRegion` sidecar (defaults + explicit) and the loop/animation rules in `validate_loops` (animations may overlap; a loop must contain a whole animation or none of it — bisecting is rejected). The auto-advance/min-delay run-loop is TUI; the pure `auto_advance_delay` is tested inline in `player/mod.rs` |
| `tests/autoadvance.rs` | `AutoAdvance`: compiled `AutoAdvanceRegion` sidecar (default 5 s delay + explicit delay/range) and that the marker draws nothing into the static frames. The play-time auto-advance run-loop is TUI; the pure `frame_auto_advance_delay`/`effective_auto_delay` step fns are tested inline in `player/mod.rs` |
| `tests/circle.rs` | `Circle`: filled-circle rendering — full-width central rows, narrower round caps, horizontal + vertical symmetry, custom fill char, and hidden outside its frame range. The aspect helpers (`columns`/`rows_for_width`) are tested inline in `engine/objects/circle.rs` |
| `tests/morph.rs` | `Morph`: end-to-end blend — `from` on the first frame / `to` on the last, `wipe-right` half-done at the midpoint, smaller grid padded with transparent space, hidden outside its range. The per-cell threshold/progress fns are tested inline in `engine/objects/morph.rs` |
| `tests/engine.rs` | `Engine::compile`: one scene per frame, empty deck, object outside `frame_count` |
| `tests/renderer.rs` | Renderer + `grid_at`: equal-z-order source order, clamp past end, out-of-bounds diff skip |

Inline unit tests also live in `src/` (e.g. `editor/properties.rs`,
`engine/objects/wrap.rs`, `editor/textedit.rs`, `editor/object_defaults.rs`,
`editor/state.rs` — frame copy/blank-insert/move/delete + `add_frames_and_share`
+ `ensure_animation`/`remove_animation`/`prune_orphan_animations` (id-based) +
`scene_object_animation_span` + `set_frame_auto_advance`/`frame_auto_advance_delay`
(add/replace/remove + remap through insert/delete), `player/mod.rs` — `loop_next`
bounce/wrap stepping + `auto_advance_delay` min-over-overlap + `animation_cluster`
overlap-merging + `frame_auto_advance_delay`/`effective_auto_delay`
(range coverage, last-frame suppression, min over markers + animations);
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
narrow row)). The suite
totals 246 tests (114 integration
+ 132 inline); `TESTS.md` is the authoritative per-test list.

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
