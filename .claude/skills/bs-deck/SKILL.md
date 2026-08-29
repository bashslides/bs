---
name: bs-deck
description: Author, edit and debug presentation source JSON for `bs` (bashslides), a terminal-native presentation engine whose decks are a flat list of frame-gated objects on a fixed character grid. Use this skill whenever the user asks for a bs deck or bashslides deck, a terminal / ASCII / TUI slide deck, or wants to write, extend, restyle, review or fix a bs source JSON file. Trigger it even when they do not name the format: mentions of `bs compile`, `bs edit`, `bs play`, `source.json`, `playable.json`, frame ranges, or pasted JSON containing objects with `"type": "label"`, `"header"`, `"h_line"`, `"morph"`, `"auto_advance"` all mean this skill. Also use it when turning notes, a markdown outline, a README or another deck format into a bs deck, and when adding or reordering slides in an existing one.
---

# Authoring `bs` presentation source JSON

`bs` (bashslides) is a terminal-native presentation engine. A deck is a fixed
character grid (`width` x `height` terminal cells) plus a timeline of
`frame_count` frames. Every object paints some cells on a contiguous span of
frames. There is no slide object, no theme, no per-slide metadata: a "slide" is
just a frame index.

## What this skill does and does not do

Produce **source JSON**. That is the only deliverable.

```
source.json  --compile-->  playable.json  --play-->  terminal output
 (you write this)          (generated)               (user's terminal)
```

The `bs` binary is not available here. Never claim to have compiled, validated,
rendered or previewed a deck, and never guess at rendered output as if it were
authoritative. Correctness comes from the rules below plus the self-check in
section 10, and the user confirms layout with `bs edit source.json`, whose
preview runs the full engine.

Commands the user runs afterwards (mention only if useful, do not pad every
reply with them):

```bash
bs edit deck.json                          # live WYSIWYG review
bs compile deck.json deck.play.json        # source -> playable
bs play deck.play.json                     # present
bs migrate deck.json                       # upgrade an old source file in place
```

Never hand-edit a compiled `playable.json`. If the user pastes one, offer to
work on the source instead.

## 1. Document structure

```json
{
  "width": 80,
  "height": 24,
  "frame_count": 8,
  "objects": [ ]
}
```

| Field | Type | Required | Meaning |
|-------|------|----------|---------|
| `width` | int | yes | canvas width in terminal cells |
| `height` | int | yes | canvas height in terminal cells |
| `frame_count` | int | yes | number of frames in the deck |
| `objects` | array | yes | scene objects, may be `[]` |
| `links` | array of int arrays | no | editor-only linked-paste families. Omit when authoring; the engine ignores it. |

Default to `80 x 24` unless the user states a terminal size. Larger canvases
(for example `100 x 30`) are fine but only if the user says their terminal is
that big, since `bs` cannot reflow a deck that overflows the window.

## 2. Shared concepts

### Frame ranges

```json
"frames": { "start": 0, "end": 8 }
```

`start` inclusive, `end` **exclusive**, 0-indexed. `{ "start": 0, "end": 8 }`
covers all 8 frames of an 8-frame deck. A single-frame object is
`{ "start": 3, "end": 4 }`. Off-by-one on `end` is the most common authoring
bug, so check every range against `frame_count` before delivering.

### Coordinates

Fixed value:

```json
{ "fixed": 10 }
```

Animated value (linear interpolation):

```json
{ "animated": { "from": 2, "to": 20, "anim": 1 } }
```

`from` and `to` are the motion; `anim` is the `id` of an `animation` object that
owns the span. The value reaches `to` on the animation's last frame (`end - 1`),
holds `from` before the span and `to` after it.

Many numeric fields accept a **bare number** as shorthand for `{ "fixed": n }`:
`width`, `height`, `h_line` endpoints, `arrow` endpoints, `y`. Use the bare form
for those, it keeps files readable. `position.x` and `position.y` are always
written as coordinate objects.

Fixed coordinates are floats internally and floored when rendered, so
`{ "fixed": 5.9 }` draws at column 5. Negative values clamp to 0.

### Position

```json
"position": { "x": { "fixed": 10 }, "y": { "fixed": 2 } }
```

The object's **top-left corner**. Line and arrow objects use explicit endpoints
instead.

### Style and color

`style` is optional everywhere. Omit it for terminal defaults.

```json
"style": { "fg": "red", "bg": { "r": 20, "g": 20, "b": 40 }, "bold": true, "dim": false }
```

| Field | Type | Default |
|-------|------|---------|
| `fg` | color | terminal default |
| `bg` | color | none (transparent) |
| `bold` | bool | `false` |
| `dim` | bool | `false` |

A color is either a named string or an RGB object.

- Named, and these 8 are the only valid names: `black`, `red`, `green`,
  `yellow`, `blue`, `magenta`, `cyan`, `white`.
- RGB: `{ "r": 20, "g": 20, "b": 40 }`, channels 0 to 255. The shape is three
  named keys. `{"rgb": [...]}` is rejected by the deserializer.

Background doubles as opacity. Text and art objects treat spaces as transparent
unless `bg` is set; setting `bg` fills the object's whole bounding box. That is
the main lever for layering.

### z_order

Optional integer on every drawable object, default `0`. Higher draws on top.
Ties break by position in `objects` (later wins).

## 3. Object catalog

Tagged union on `"type"`. Spellings are exact snake_case. `h_line` and
`auto_advance` are the two that get mistyped.

| `type` | Draws | Purpose |
|--------|-------|---------|
| `label` | text | multi-line text, optional box, alignment |
| `list` | text | ordered or unordered list |
| `header` | text | big ASCII block letters |
| `h_line` | line | horizontal rule |
| `rect` | box | border with optional title |
| `arrow` | line | arrow with auto or explicit heads, L-routing |
| `table` | grid | bordered or borderless table |
| `art` | art | inline multi-line ASCII art |
| `circle` | shape | parametric filled circle |
| `morph` | art | animated blend between two ASCII grids |
| `group` | nothing | logical container |
| `command` | box | runs a binary at play time, shows output |
| `loop` | nothing | play-time loop over a frame range |
| `animation` | nothing | owns an animation span and auto-play |
| `auto_advance` | nothing | timer-driven advance over a frame range |

Every drawable object takes `style` (optional), `frames` (required, except an
auto `group`) and `z_order` (optional).

## 4. Text objects

### `label`

The workhorse.

| Field | Type | Default | Notes |
|-------|------|---------|-------|
| `text` | string | required | `\n` separates lines |
| `position` | Position | required | top-left |
| `width` | coordinate | `0` | `0` = auto, no wrapping; `>0` wraps at this width |
| `height` | coordinate | `0` | `0` = auto; `>0` clips or pads to this many rows |
| `framed` | bool | `false` | box border around the text |
| `frame_style` | style | none | border-only style, defaults to `style` |
| `align` | `left`/`center`/`right` | `left` | no-op when `width == 0` |
| `valign` | `top`/`center`/`bottom` | `top` | no-op when `height == 0` |

```json
{
  "type": "label",
  "text": "Hello\nWorld",
  "position": { "x": { "fixed": 4 }, "y": { "fixed": 2 } },
  "width": 40,
  "align": "center",
  "style": { "fg": "white", "bold": true },
  "frames": { "start": 0, "end": 8 }
}
```

`framed` draws the border one cell outside the text bounding box so the text
position is preserved (at the canvas origin it shifts text in by 1 so the border
does not cover it). `style.bg` fills the whole `width x height` box.

### `list`

Each `\n`-separated line is one item; blank lines are dropped.

| Field | Type | Default | Notes |
|-------|------|---------|-------|
| `text` | string | required | one item per line |
| `position` | Position | required | |
| `width` | coordinate | `0` | `0` = no wrapping; `>0` wraps each item |
| `height` | coordinate | `0` | `0` = auto; `>0` clips or pads |
| `ordered` | bool | `false` | `true` gives `1. 2. 3.` |
| `bullet` | string | `"-"` | unordered marker, ignored when `ordered` |
| `spacing` | int | `1` | blank rows between items |

Wrapped continuation rows auto-indent under the item text.

### `header`

Big block letters from a built-in ASCII font. Text is uppercased and only
glyphs the font knows are drawn.

| Field | Type | Default | Notes |
|-------|------|---------|-------|
| `text` | string | required | wraps to canvas width on word boundaries |
| `position` | Position | required | top-left of the first glyph row |
| `ch` | char | `"█"` | fill character |

Glyphs are several rows tall and wrapped lines get a one-row gap. Reserve
generous vertical room below `position.y`, keep header text short (one word or
two on an 80-cell canvas), and say in the reply that the exact glyph height is
worth eyeballing in `bs edit`.

## 5. Shapes and lines

### `h_line`

Horizontal rule at row `y` from `x_start` (inclusive) to `x_end` (exclusive).

| Field | Type | Default |
|-------|------|---------|
| `y` | coordinate | required |
| `x_start` | coordinate | required |
| `x_end` | coordinate | required |
| `ch` | char | `"─"` |

```json
{ "type": "h_line", "y": 5, "x_start": 2, "x_end": 40,
  "style": { "fg": "blue" }, "frames": { "start": 0, "end": 8 } }
```

### `rect`

Border only, blank interior, optional title on the top edge.

| Field | Type | Default | Notes |
|-------|------|---------|-------|
| `position` | Position | required | top-left |
| `width` | coordinate | required | border drawn at the edges |
| `height` | coordinate | required | |
| `title` | string | none | on the top edge, clipped to width |

For a solid panel, layer a `label` with a `bg` behind or over it.

### `arrow`

Straight or L-routed orthogonal arrow.

| Field | Type | Default | Notes |
|-------|------|---------|-------|
| `x1,y1,x2,y2` | coordinate | required | endpoints, bare numbers fine |
| `head` | bool | `true` | head at `(x2,y2)` |
| `head_start` | bool | `false` | outward head at the start too |
| `head_ch` | char or omit | auto | omit or `null` for direction-aware default |
| `body_ch` | char or omit | auto | omit or `null` for `─`/`│` |

Routing is automatic: mostly-horizontal arrows go horizontal first then turn,
mostly-vertical go vertical first. Head chars auto-rotate; the `▶◀▼▲`, `><v^`
and `→←↓↑` families are recognized for `head_ch`.

### `circle`

| Field | Type | Default | Notes |
|-------|------|---------|-------|
| `position` | Position | required | top-left of the bounding box |
| `diameter` | int | `10` | height in rows; width derived as roughly 2x |
| `ch` | char | `"@"` | fill character |

Terminal cells are about 2:1, so column extent is doubled to look round. Budget
`2 * diameter` columns when placing it.

## 6. Art objects

### `art`

Multi-line ASCII art rendered verbatim. Spaces transparent unless `bg` is set.

| Field | Type | Default | Notes |
|-------|------|---------|-------|
| `position` | Position | required | top-left |
| `art` | string | required | each char at its row/col offset |
| `name` | string | `""` | display-only label |

Art is inline and self-contained; the file never references an external art
library. Write art with explicit `\n` escapes in the JSON string, and count the
longest line to know the bounding width.

### `morph`

Blends grid `from` into grid `to` across the object's frame range. Fully baked
into static frames.

| Field | Type | Default | Notes |
|-------|------|---------|-------|
| `position` | Position | required | grids overlay corner-to-corner |
| `from` | string | required | shown at the first frame of the range |
| `to` | string | required | reached on the last frame |
| `name` | string | `""` | display-only |
| `mode` | enum | `"dissolve"` | see below |

`mode` is one of `dissolve`, `wipe-right`, `wipe-left`, `wipe-down`, `wipe-up`.
Note the kebab-case. A single-frame range stays at `from`. Give the two grids
the same dimensions unless a ragged blend is intentional.

## 7. `table`

| Field | Type | Default | Notes |
|-------|------|---------|-------|
| `position` | Position | required | top-left |
| `width` | coordinate | `30` | total table width in cells |
| `height` | coordinate | `0` | `0` = auto-size to content; `>0` pads, never clips shorter |
| `col_widths` | array of floats | required | fractions in `[0,1]` summing to about 1.0; length = column count |
| `rows` | int | required | number of rows |
| `cells` | `cells[row][col]` | `[]` | auto-extended to `rows x col_count`, missing cells blank |
| `header_bold` | bool | `false` | first row bold |
| `borders` | bool | `true` | box-drawing borders around every cell |

A cell is `{ "content": "text", "style": { } }`; `content` defaults to `""` and
`style` is an optional per-cell override.

```json
{
  "type": "table",
  "position": { "x": { "fixed": 2 }, "y": { "fixed": 2 } },
  "width": 40,
  "col_widths": [0.4, 0.3, 0.3],
  "rows": 2,
  "cells": [
    [ {"content": "Name"}, {"content": "Role"}, {"content": "Loc"} ],
    [ {"content": "Ada"}, {"content": "Eng"}, {"content": "UK", "style": {"fg": "green"}} ]
  ],
  "header_bold": true,
  "frames": { "start": 0, "end": 8 }
}
```

Column count comes from `col_widths.length`, the last column absorbs rounding
slack, and cell text wraps to the column's content width. Keep cell text short:
`0.3 * 40` is 12 cells minus border and padding.

## 8. Containers and runtime behaviors

These five draw nothing into static frames (or only a placeholder box).

### `group`

Bundles other objects **by index** into the top-level `objects` array.

```json
{ "type": "group", "members": [1, 2, 3], "z_order": 0 }
```

- Auto form (`frames` omitted or `null`): members keep their own ranges. This is
  the normal case.
- Explicit form (`frames` set): the group's range overrides every member's,
  widening or narrowing it.

Indices are fragile under insertion and reordering. Prefer not to emit groups at
all when hand-authoring; when the user asks for one, re-verify every index after
any later edit to `objects`.

### `command`

Runs a binary at play time and paints its stdout and stderr into a box. It is
never run while editing or compiling.

| Field | Type | Default | Notes |
|-------|------|---------|-------|
| `position`, `width`, `height` | | required | the output box |
| `command` | string | required | looked up on `PATH` |
| `args` | array of strings | `[]` | |
| `cwd` | string | none | defaults to the player's cwd |
| `timeout_secs` | int | none | omit for no timeout |
| `border` | bool | `true` | border around the output region |

The player pipes stdio, paints the tail of the output into the box interior, and
marks the top edge with a green check on exit 0 or a red cross on failure or
timeout. Navigation never branches on exit status.

Only emit a `command` object when the user asks for a live command. Give it a
`timeout_secs` so a hanging binary does not stall the talk, and keep the box tall
enough for the expected output.

### `loop`

```json
{ "type": "loop", "frames": { "start": 4, "end": 8 },
  "delay_ms": 500, "count": 0, "bounce": true }
```

| Field | Type | Default | Notes |
|-------|------|---------|-------|
| `frames` | FrameRange | required | end exclusive |
| `delay_ms` | int | `500` | delay between auto-advanced frames |
| `count` | int | `0` | plays before moving on; `0` = forever |
| `bounce` | bool | `true` | ping-pong vs restart |

Validated at compile time: loops must be non-empty, fit the deck, must not
overlap or nest, and must not bisect an `animation` (a loop contains an
animation span wholly or not at all). A bad loop fails compilation. The
presenter breaks out with the arrow keys.

### `animation`

Owns a span and its auto-play config. It draws nothing; the motion lives in
other objects' animated coordinates, joined by `id`.

```json
{ "type": "animation", "id": 1, "frames": { "start": 0, "end": 5 },
  "auto_play": true, "delay_ms": 500 }
```

| Field | Type | Default | Notes |
|-------|------|---------|-------|
| `id` | int | required | unique; referenced by `{"animated": {"anim": id}}` |
| `frames` | FrameRange | required | the interpolation span |
| `auto_play` | bool | `true` | auto-advance across the span at play time |
| `delay_ms` | int | `500` | delay between auto-advanced frames |
| `gap_frames` | int | `0` | editor metadata, ignored at runtime |

Both halves are required: the `animation` object and the animated coordinate on
each moving object. A dangling `anim` reference renders static at `from`.
Animations may overlap (unlike loops); where several auto-play animations cover
the same boundary the effective delay is the minimum of their `delay_ms`. One
`id` can drive several objects and both axes.

### `auto_advance`

```json
{ "type": "auto_advance", "frames": { "start": 2, "end": 5 }, "delay_ms": 5000 }
```

Suppressed on the last frame and while a `loop` drives playback. Where an
auto-play `animation` also covers a frame, the effective delay is the minimum of
the two. Manual navigation always works.

## 9. Composing a deck

Work from content to geometry.

1. **Count the beats.** One beat per thing the presenter says. `frame_count` is
   the beat count, not the section count. Progressive reveals cost frames: a
   4-bullet build is 4 frames.
2. **Fix the furniture.** Objects that persist (title bar, rule, footer) get
   `{ "start": 0, "end": frame_count }` and low `z_order`.
3. **Gate the content.** Each beat's objects get their own range. To keep a
   bullet on screen once revealed, run its range to the end of the section
   rather than a single frame.
4. **Reveal patterns.** For a build, emit one `list` per step with growing
   `text` and disjoint ranges, or one `label` per bullet with staggered starts.
   Per-bullet labels are easier to restyle and dim; the growing-list form is
   more compact. Prefer per-bullet labels past three bullets.
5. **Place by budget, not by eye.** Left margin `x = 2` to `4`, title at
   `y = 1`, rule under it, body from `y = 4`. Keep everything inside
   `0 .. width-1` and `0 .. height-1`, and leave the bottom row or two free.
6. **Style sparingly.** One accent color plus default foreground reads better in
   a terminal than five colors. `dim` is the cleanest way to de-emphasize.

Text width is the one thing to compute rather than guess: count characters. For
a `label` with `width: 0`, the bounding width is the longest line of `text`, so
`position.x + longest_line` must stay under `width`. For `width: n`, wrapping
happens at `n` and rows grow accordingly. `list` items also carry the bullet or
number prefix.

Do not hand-derive precise centering or complex column math and present it as
exact. Give the layout sane margins and tell the user to confirm in `bs edit`,
whose preview is the same engine that compiles and plays.

## 10. Self-check before delivering

Re-read the JSON against this list. These are the failure modes that either
break compilation or waste the user's review pass.

1. Valid JSON. No trailing commas, no comments, `\n` escaped inside strings.
2. `width`, `height`, `frame_count`, `objects` all present at the top level.
3. Every `type` tag spelled exactly: `label`, `list`, `header`, `h_line`,
   `rect`, `arrow`, `table`, `art`, `circle`, `morph`, `group`, `command`,
   `loop`, `animation`, `auto_advance`.
4. Every drawable object has `frames`, and every `frames.end` is at most
   `frame_count`, with `start < end`.
5. Some object is visible on every frame from 0 to `frame_count - 1`. A blank
   frame in the middle is almost always an off-by-one.
6. Every `{"animated": {"anim": N}}` has a matching `animation` with `id: N`,
   and that animation's span is covered by the moving object's own `frames`.
7. Loops: non-empty, inside the deck, no overlap, no nesting, no bisected
   animation span.
8. `group.members` indices point at the objects intended after any reordering.
9. Colors are one of the 8 names or `{"r","g","b"}` with channels 0 to 255.
10. Tables: `col_widths` sums to about 1.0, its length matches every row's cell
    count, and `rows` matches the `cells` array.
11. Geometry inside the canvas: `position` plus the object's extent stays within
    `width` and `height`, including derived extents (`circle` is about
    `2 * diameter` wide, `framed` labels add a cell on each side).
12. `position.x` and `position.y` are coordinate objects, not bare numbers.

## 11. Output

Write the deck to a `.json` file and present it, so the user can download it and
run `bs edit` on it directly. Name it after the topic, for example
`intro-deck.json`. If file creation is not available in the current surface,
emit the whole deck in a single fenced JSON code block instead, with no
commentary inside the block.

Alongside the file, give a short frame-by-frame outline (one line per frame) so
the user can check the narrative without reading JSON, plus any assumptions
made about canvas size and any layout worth eyeballing in the editor. Keep that
prose short; the deck is the deliverable.

## 12. Minimal complete example

A 3-frame deck: a persistent title, a numbered list on frames 1 and 2, and a
star that slides across on the same frames.

```json
{
  "width": 60,
  "height": 20,
  "frame_count": 3,
  "objects": [
    {
      "type": "header",
      "text": "BS",
      "position": { "x": { "fixed": 2 }, "y": { "fixed": 1 } },
      "style": { "fg": "cyan" },
      "frames": { "start": 0, "end": 3 }
    },
    {
      "type": "list",
      "text": "Author JSON\nReview in bs edit\nCompile and play",
      "position": { "x": { "fixed": 4 }, "y": { "fixed": 9 } },
      "width": 40,
      "ordered": true,
      "style": { "fg": "white" },
      "frames": { "start": 1, "end": 3 }
    },
    {
      "type": "label",
      "text": "★",
      "position": {
        "x": { "animated": { "from": 0, "to": 50, "anim": 1 } },
        "y": { "fixed": 17 }
      },
      "style": { "fg": "yellow", "bold": true },
      "frames": { "start": 1, "end": 3 }
    },
    {
      "type": "animation",
      "id": 1,
      "frames": { "start": 1, "end": 3 },
      "auto_play": false
    }
  ]
}
```
