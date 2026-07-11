# AI Maintainability Guide

This is the compact context file for AI agents. It summarizes the stable
architecture, invariants, and high-risk edit paths without repeating the full
detail in `CLAUDE.md`.

## Mental Model

`bs` is a terminal-native presentation engine:

```text
SourcePresentation JSON
  -> Engine::compile()      Vec<ResolvedScene> with DrawOps per frame
  -> Renderer::render()     PlayablePresentation with full/diff frames
  -> Player::play()         terminal playback
```

The editor uses the same engine and renderer for live preview. If preview and
compiled playback disagree, look first for a runtime sidecar object or duplicated
frame-replay logic.

## Stable Boundaries

| Area | Files | Rule of thumb |
| --- | --- | --- |
| Source schema | `src/engine/source.rs`, object structs | Own serde shape, validation, sidecar collection, frame ranges |
| Object rendering | `src/engine/objects/` | Convert one object into `DrawOp`s for one frame; no terminal IO |
| Raster/diff | `src/renderer/mod.rs`, `src/types.rs` | Convert draw ops into frames; `PlayablePresentation::grid_at` is canonical replay |
| Playback | `src/player/mod.rs` | Own terminal IO, keyboard navigation, command execution, auto-advance timers |
| Editor state | `src/editor/state.rs` | Pure-ish deck mutations, frame/object remapping, clipboard logic |
| Editor input | `src/editor/input.rs` | Key handling and mode transitions |
| Editor panels | `src/editor/panel.rs`, `menubar.rs`, `timeline.rs`, `ui.rs` | Rendering of TUI controls and status |
| Format docs | `PRESENTATION_FORMAT.md` | Contract for hand-authored source JSON |
| Tests | `tests/`, inline `#[cfg(test)]` modules | Prefer behavior tests through compile + render grids |
| Doc sync | `tests/docs.rs` | Machine-checks docs against the code; fix the doc it names, don't weaken the check |

## Core Invariants

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
  frames override member ranges.
- There must always be at least one frame after frame deletion.

## High-Risk Changes

### Adding an object type

Use the checklist in `src/engine/objects/mod.rs`. It is more complete than what
the compiler catches. Also update:

- `PRESENTATION_FORMAT.md` for JSON authoring.
- `README.md` if the user-facing catalog changes.
- `TESTS.md` after adding tests.

### Changing source JSON

Update serde structs, validation, examples, `PRESENTATION_FORMAT.md`, and tests.
If existing saved decks should continue to work, update `src/migrate.rs` or add
backward-compatible deserialization.

### Changing timeline or frame operations

Start in `src/editor/state.rs`. Verify object ranges, links, groups, animations,
auto-advance markers, and clipboard behavior. Favor pure helper tests in
`state.rs` plus end-to-end integration tests where rendering changes.

### Changing playback behavior

Keep the run loop thin and move calculations into pure helpers that can be
unit-tested. Existing examples include loop stepping, animation cluster merging,
and effective auto-delay selection.

### Changing editor UI

Expect coordinated edits across `state.rs`, `input.rs`, `panel.rs`,
`menubar.rs`, and possibly `timeline.rs`. Avoid adding more large inline
rendering branches when a small helper can make mode-specific rendering clearer.

## Test Strategy

- Rendering behavior: integration test in `tests/<object>.rs` using JSON source.
- Pure geometry/layout helpers: inline unit tests near the helper.
- Player timing/navigation logic: unit-test pure functions in `player/mod.rs`.
- TUI-only interactions: isolate the state transition or calculation into a
  helper, then test that helper.
- Docs-only changes: run `cargo test --test docs` — `tests/docs.rs` checks the
  mechanical doc↔code sync rules (TESTS.md rows/totals, referenced paths,
  object-type lists, JSON examples, build-command copies) and its failure
  messages say which doc line to fix. Prose accuracy still needs a human/agent
  read; do a quick repo search for stale references.

## Maintainability Guardrails

- Keep ownership metadata at the boundary that owns it. In particular,
  `SceneObject::type_name()` (in `src/engine/source.rs`) is the one source of
  editor-facing object names; do not recreate that match in editor modules.
- The Add Object menu uses `object_defaults::OBJECT_TYPES`, a descriptor list
  (`AddableObjectType { name, shortcut }`) that keeps each addable type's name
  and shortcut together. Do not split it into parallel arrays. Its unit tests
  verify that every descriptor constructs the named object and that shortcuts
  stay unique, and `object_type_registry_is_complete` cross-checks the list
  against the `SceneObject` enum (and the documented sub-menu-only set) so a new
  variant can't silently go missing from the menu.
- Exhaustive `match` expressions over `SceneObject` are deliberate guardrails:
  a new enum variant makes the compiler show every behavior that still needs a
  decision. Prefer an exhaustive match to a wildcard in schema, rendering, and
  editor-dispatch code (the `every_variant` fixture behind the registry test
  leans on exactly this to force new types through the check).
- The object-type checklist is a change checklist, not a substitute for review.
  After an object addition, search for `SceneObject::` and review every relevant
  match that the compiler did not force you to touch (especially sidecars and
  special editor flows).

## Known Maintainability Debt

- The editor mode FSM is large and keeps growing as object behavior grows.
- `panel.rs::render_right_panel` is still a large multi-mode renderer.
- Several `Editable` impls repeat common x/y/width/height/style/frame handling.

Prefer incremental improvements that reduce duplication in touched areas without
turning behavior changes into broad refactors.
