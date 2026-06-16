# `bs` Presentation Source Format — Reference for Authoring & Editing

This document describes the **source JSON format** consumed by `bs`, a
terminal-native presentation engine. It is written so that an AI assistant (or a
human) can author and edit `bs` presentation files correctly without reading the
Rust source. Drop it into any repo where you write `bs` decks.

> Every field name, default, and enum spelling below is taken directly from the
> engine's serde definitions. Where the engine accepts shorthand or has
> non-obvious behavior, it is called out explicitly.

---

## 1. The pipeline & how you use this file

```
source.json  ──compile──▶  playable.json  ──play──▶  terminal output
   (this file's format)      (generated; do not hand-edit)
```

You author the **source** file. The other two stages are generated.

```bash
bs compile source.json out.json   # source → playable
bs edit    source.json [more…]    # interactive editor (live WYSIWYG preview)
bs play    out.json               # play a compiled presentation
bs migrate source.json            # upgrade an old source file in place (.bak backup)
```

Typical loop: an assistant writes/edits `source.json` → the human opens it with
`bs edit source.json` to review visually → then `bs compile` + `bs play`.

**The editor runs the full engine live**, so what `bs edit` shows is exactly what
will compile and play. Only four object types have play-time-only behavior that
the editor shows as a placeholder (see §8).

---

## 2. Top-level document structure

```json
{
  "width": 80,
  "height": 24,
  "frame_count": 8,
  "objects": [ /* … SceneObjects … */ ]
}
```

| Field | Type | Required | Meaning |
|-------|------|----------|---------|
| `width` | integer | **yes** | Canvas width in terminal cells |
| `height` | integer | **yes** | Canvas height in terminal cells |
| `frame_count` | integer | **yes** | Number of frames (slides) in the deck |
| `objects` | array | **yes** | The scene objects (may be empty `[]`) |
| `links` | array of arrays of ints | no | Editor-only "linked paste" families; omit when authoring by hand. The engine ignores it. |

There is **no** top-level title, theme, or per-slide metadata. A "slide" is just
a frame index; an object decides which frames it appears on via its `frames`
range. The whole deck is one flat list of objects, each gated to a frame range.

> **Mental model:** think of the deck as a fixed-size grid of cells (`width × height`)
> and a timeline of `frame_count` frames. Each object paints some cells on some
> contiguous span of frames.

---

## 3. Core shared concepts

### 3.1 Frame ranges (`frames`)

Almost every object has a `frames` field:

```json
"frames": { "start": 0, "end": 8 }
```

- `start` is **inclusive**, `end` is **exclusive**. `{ "start": 0, "end": 8 }`
  means frames 0,1,2,3,4,5,6,7 — i.e. all 8 frames of an 8-frame deck.
- A single-frame object: `{ "start": 3, "end": 4 }` (visible only on frame 3).
- Frames are 0-indexed.

### 3.2 Coordinates (`Coordinate`)

Positions and sizes are **coordinates**, which can be either fixed or animated.

**Fixed** (a static value):
```json
{ "fixed": 10 }
```

**Animated** (linear interpolation across an animation span):
```json
{ "animated": { "from": 2, "to": 20, "anim": 1 } }
```
- `from` / `to` are the start and end values (the *motion*).
- `anim` references an [`Animation` object](#810-animation) by its `id` — that
  animation owns the *span* (which frames the motion plays over). The motion
  reaches `to` on the animation's **last** frame (`end - 1`) and holds `from`
  before the span and `to` after it.

**Shorthand:** many numeric fields accept a **bare number** instead of a
coordinate object. The engine treats `10` as `{ "fixed": 10 }`. This works for
`width`, `height`, `hline` endpoints, arrow endpoints, etc. So both of these are
valid and identical:

```json
"width": 20
"width": { "fixed": 20 }
```

Positions (`position.x` / `position.y`) are always written as coordinate objects
(`{ "fixed": … }` or `{ "animated": … }`), not bare numbers.

Fixed coordinates are stored as floats internally and **floored** when rendered,
so `{ "fixed": 5.9 }` draws at column 5. Negative values clamp to 0.

### 3.3 Position

Objects with a single anchor point carry:
```json
"position": { "x": { "fixed": 10 }, "y": { "fixed": 2 } }
```
This is the object's **top-left corner** (for line/arrow objects the geometry is
expressed as explicit endpoints instead — see those types).

### 3.4 Style & color

`style` is **optional everywhere** — omit it for defaults (terminal default
foreground, no background, not bold).

```json
"style": { "fg": "red", "bg": { "rgb": [20, 20, 40] }, "bold": true, "dim": false }
```

| Field | Type | Default |
|-------|------|---------|
| `fg` | color | terminal default |
| `bg` | color | none (transparent) |
| `bold` | bool | `false` |
| `dim` | bool | `false` |

**Color** is either a **named color string** or an **RGB object**:

- Named (the only 8 valid names): `"black"`, `"red"`, `"green"`, `"yellow"`,
  `"blue"`, `"magenta"`, `"cyan"`, `"white"`.
- RGB: `{ "rgb": [r, g, b] }` with each channel 0–255.

> **Background = opacity.** Many text/art objects treat spaces as *transparent*
> unless a `bg` is set. Setting a `bg` fills the object's whole bounding box with
> that background (a solid block); leaving it unset lets underlying objects show
> through the gaps. This is the main lever for layering.

### 3.5 `z_order`

Every drawable object has an optional `z_order` (integer, default `0`). Higher
draws on top. Ties break by object order in the `objects` array (later wins).

---

## 4. Object catalog overview

Objects are a tagged union: each has a `"type"` field. **The tag spellings are
exact** (snake_case) — note `h_line` and `auto_advance` in particular:

| `type` | Draws | Purpose |
|--------|-------|---------|
| `label` | text | Multi-line text, optional box, alignment |
| `list` | text | Ordered/unordered list |
| `header` | text | Big ASCII-art block letters |
| `h_line` | line | Horizontal rule |
| `rect` | box | Rectangle/border with optional title |
| `arrow` | line | Arrow with auto/explicit head(s), L-routing |
| `table` | grid | Bordered/borderless table |
| `art` | art | Inline multi-line ASCII art |
| `circle` | shape | Parametric filled circle |
| `morph` | art | Animated blend between two ASCII grids |
| `group` | nothing | Logical container of other objects |
| `command` | box* | Runs a binary at play time, shows output |
| `loop` | nothing* | Play-time loop over a frame range |
| `animation` | nothing* | Owns an animation span + auto-play |
| `auto_advance` | nothing* | Auto-advance a frame range on a timer |

`*` = play-time behavior; see §8.

Fields common to all **drawable** objects: `style` (optional), `frames`
(required, except auto `group`), `z_order` (optional, default 0).

---

## 5. Text objects

### 5.1 `label`

Multi-line text. The workhorse object.

```json
{
  "type": "label",
  "text": "Hello\nWorld",
  "position": { "x": { "fixed": 4 }, "y": { "fixed": 2 } },
  "width": 0,
  "height": 0,
  "framed": false,
  "frame_style": { "fg": "cyan" },
  "align": "left",
  "valign": "top",
  "style": { "fg": "white", "bold": true },
  "frames": { "start": 0, "end": 8 },
  "z_order": 0
}
```

| Field | Type | Default | Notes |
|-------|------|---------|-------|
| `text` | string | **required** | `\n` separates lines |
| `position` | Position | **required** | top-left |
| `width` | coordinate | `0` | `0` = auto (no wrapping); `>0` wraps at this width |
| `height` | coordinate | `0` | `0` = auto; `>0` clips/pads to this many rows |
| `framed` | bool | `false` | draw a box border around the text |
| `frame_style` | style | none | border-only style (defaults to `style`) |
| `align` | `"left"`/`"center"`/`"right"` | `"left"` | horizontal align within `width` (no-op if `width==0`) |
| `valign` | `"top"`/`"center"`/`"bottom"` | `"top"` | vertical align within `height` (no-op if `height==0`) |
| `style`, `frames`, `z_order` | | | common fields |

Notes:
- With `width == 0`, text is placed verbatim (each `\n` is a new row); `align`
  does nothing because there's no box to align in.
- `framed` draws the border one cell **outside** the text bounding box, so the
  text position is preserved (at the canvas origin it shifts text in by 1 so the
  border doesn't cover it).
- Setting `style.bg` fills the whole `width × height` box with the background.

### 5.2 `list`

Ordered or unordered list. Each `\n`-separated line of `text` is one item.

```json
{
  "type": "list",
  "text": "First point\nSecond point\nThird point",
  "position": { "x": { "fixed": 4 }, "y": { "fixed": 3 } },
  "width": 40,
  "height": 0,
  "ordered": false,
  "bullet": "-",
  "spacing": 1,
  "style": { "fg": "white" },
  "frames": { "start": 1, "end": 8 },
  "z_order": 0
}
```

| Field | Type | Default | Notes |
|-------|------|---------|-------|
| `text` | string | **required** | one item per line; blank lines are dropped |
| `position` | Position | **required** | |
| `width` | coordinate | `0` | `0` = no wrapping; `>0` wraps each item |
| `height` | coordinate | `0` | `0` = auto; `>0` clips/pads |
| `ordered` | bool | `false` | `true` → numbered `1. 2. 3.`; `false` → bulleted |
| `bullet` | string | `"-"` | marker for unordered items (ignored when `ordered`) |
| `spacing` | integer | `1` | blank rows between items |
| `style`, `frames`, `z_order` | | | common fields |

Wrapped continuation rows are auto-indented to line up under the item text.

### 5.3 `header`

Large block-letter text rendered from a built-in ASCII font (think a banner /
title). Letters are uppercased; only glyphs the font knows are drawn.

```json
{
  "type": "header",
  "text": "INTRO",
  "position": { "x": { "fixed": 2 }, "y": { "fixed": 1 } },
  "ch": "█",
  "style": { "fg": "green" },
  "frames": { "start": 0, "end": 1 },
  "z_order": 0
}
```

| Field | Type | Default | Notes |
|-------|------|---------|-------|
| `text` | string | **required** | wrapped to canvas width on word boundaries |
| `position` | Position | **required** | top-left of the first glyph row |
| `ch` | char | `"█"` | fill character for the big glyphs |
| `style`, `frames`, `z_order` | | | common fields |

Glyphs are several rows tall; the header auto-wraps to the canvas width with a
one-row gap between wrapped lines. Leave vertical room below `position.y`.

---

## 6. Shape & line objects

### 6.1 `h_line`  (note the underscore in the type tag)

A horizontal rule from `x_start` to `x_end` (exclusive) at row `y`.

```json
{
  "type": "h_line",
  "y": 5,
  "x_start": 2,
  "x_end": 40,
  "ch": "─",
  "style": { "fg": "blue" },
  "frames": { "start": 0, "end": 8 },
  "z_order": 0
}
```

| Field | Type | Default | Notes |
|-------|------|---------|-------|
| `y` | coordinate | **required** | row |
| `x_start` | coordinate | **required** | inclusive |
| `x_end` | coordinate | **required** | exclusive |
| `ch` | char | `"─"` | line character |

(`y`, `x_start`, `x_end` accept bare numbers.)

### 6.2 `rect`

A rectangle border with a blank interior and an optional title on the top edge.

```json
{
  "type": "rect",
  "position": { "x": { "fixed": 1 }, "y": { "fixed": 1 } },
  "width": 30,
  "height": 8,
  "title": "Notes",
  "style": { "fg": "yellow" },
  "frames": { "start": 2, "end": 6 },
  "z_order": 0
}
```

| Field | Type | Default | Notes |
|-------|------|---------|-------|
| `position` | Position | **required** | top-left |
| `width` | coordinate | **required** | box-drawing border drawn at the edges |
| `height` | coordinate | **required** | |
| `title` | string | none | drawn on the top edge, clipped to the width |
| `style`, `frames`, `z_order` | | | common fields |

The interior is **not** filled (it's a border only). Put a `label` with a `bg`
behind/over it if you want a solid panel.

### 6.3 `arrow`

A straight or L-routed (orthogonal) arrow from `(x1,y1)` to `(x2,y2)`.

```json
{
  "type": "arrow",
  "x1": 4, "y1": 4,
  "x2": 30, "y2": 10,
  "head": true,
  "head_start": false,
  "head_ch": null,
  "body_ch": null,
  "style": { "fg": "red" },
  "frames": { "start": 3, "end": 8 },
  "z_order": 0
}
```

| Field | Type | Default | Notes |
|-------|------|---------|-------|
| `x1,y1,x2,y2` | coordinate | **required** | endpoints (accept bare numbers) |
| `head` | bool | `true` | draw an arrowhead at the end `(x2,y2)` |
| `head_start` | bool | `false` | also draw an outward head at the start (double-headed) |
| `head_ch` | char or omit | auto | custom head char; omit/`null` → auto by direction |
| `body_ch` | char or omit | auto | custom body char; omit/`null` → auto (`─`/`│`) |
| `style`, `frames`, `z_order` | | | common fields |

Routing is automatic: mostly-horizontal arrows go horizontal-first then turn;
mostly-vertical go vertical-first. The head char auto-rotates to point the right
way (`▶◀▼▲`, `><v^`, `→←↓↑` families are recognized for `head_ch`).

### 6.4 `circle`

A parametric filled circle drawn with one repeated character. Because terminal
cells are ~2:1 (tall), the column extent is derived as ~2× the row diameter so
the circle looks round.

```json
{
  "type": "circle",
  "position": { "x": { "fixed": 4 }, "y": { "fixed": 2 } },
  "diameter": 10,
  "ch": "@",
  "style": { "fg": "cyan" },
  "frames": { "start": 0, "end": 1 },
  "z_order": 0
}
```

| Field | Type | Default | Notes |
|-------|------|---------|-------|
| `position` | Position | **required** | top-left of the bounding box |
| `diameter` | integer | `10` | height in **rows**; width is derived (~2×) |
| `ch` | char | `"@"` | fill character |
| `style`, `frames`, `z_order` | | | common fields |

---

## 7. Art objects

### 7.1 `art`

Inline multi-line ASCII art, rendered verbatim. Spaces are transparent unless a
`bg` is set.

```json
{
  "type": "art",
  "position": { "x": { "fixed": 10 }, "y": { "fixed": 3 } },
  "art": "  /\\_/\\\n ( o.o )\n  > ^ <",
  "name": "cat",
  "style": { "fg": "white" },
  "frames": { "start": 0, "end": 8 },
  "z_order": 0
}
```

| Field | Type | Default | Notes |
|-------|------|---------|-------|
| `position` | Position | **required** | top-left |
| `art` | string | **required** | multi-line; each char placed at its row/col offset |
| `name` | string | `""` | display-only label (e.g. the source piece name) |
| `style`, `frames`, `z_order` | | | common fields |

`art` is self-contained — the file never depends on the art library.

### 7.2 `morph`

Blends one ASCII grid (`from`) into another (`to`) across its frame range. Fully
baked into static frames, so the editor preview shows it.

```json
{
  "type": "morph",
  "position": { "x": { "fixed": 10 }, "y": { "fixed": 3 } },
  "from": "  ***\n *****\n  ***",
  "to":   "  +++\n +++++\n  +++",
  "name": "ball→square",
  "mode": "dissolve",
  "style": { "fg": "magenta" },
  "frames": { "start": 2, "end": 8 },
  "z_order": 0
}
```

| Field | Type | Default | Notes |
|-------|------|---------|-------|
| `position` | Position | **required** | the two grids overlay corner-to-corner |
| `from` | string | **required** | shown at the first frame of the range |
| `to` | string | **required** | reached on the last frame of the range |
| `name` | string | `""` | display-only |
| `mode` | enum | `"dissolve"` | transition style (below) |
| `style`, `frames`, `z_order` | | | common fields |

`mode` is one of: `"dissolve"` (default), `"wipe-right"`, `"wipe-left"`,
`"wipe-down"`, `"wipe-up"` (note kebab-case). A single-frame range stays at
`from`. Spaces are transparent unless a `bg` is set.

---

## 8. Tables

### `table`

```json
{
  "type": "table",
  "position": { "x": { "fixed": 2 }, "y": { "fixed": 2 } },
  "width": 40,
  "height": 0,
  "col_widths": [0.4, 0.3, 0.3],
  "rows": 3,
  "cells": [
    [ {"content": "Name"}, {"content": "Role"}, {"content": "Loc"} ],
    [ {"content": "Ada"},  {"content": "Eng"},  {"content": "UK"}  ],
    [ {"content": "Bo"},   {"content": "PM"},   {"content": "US", "style": {"fg": "green"}} ]
  ],
  "header_bold": true,
  "borders": true,
  "style": { "fg": "white" },
  "frames": { "start": 0, "end": 8 },
  "z_order": 0
}
```

| Field | Type | Default | Notes |
|-------|------|---------|-------|
| `position` | Position | **required** | top-left |
| `width` | coordinate | `30` | total table width in cells |
| `height` | coordinate | `0` | `0` = auto-size to content; `>0` pads (never clips shorter) |
| `col_widths` | array of floats | **required** | fractional widths in `[0,1]`, summing to ~1.0; count = number of columns |
| `rows` | integer | **required** | number of rows |
| `cells` | `cells[row][col]` | `[]` | auto-extended to `rows × col_count`; missing cells are blank |
| `header_bold` | bool | `false` | render the first row bold |
| `borders` | bool | `true` | box-drawing borders around every cell |
| `style`, `frames`, `z_order` | | | common fields |

A **cell** is `{ "content": "text", "style": { … } }`. `content` defaults to
`""`; `style` is optional (per-cell override). Column count is determined by
`col_widths.length`; the last column absorbs rounding slack. Cell text wraps to
the column's content width.

---

## 9. Containers & runtime behaviors

These four play-time types draw **nothing** into the static frames (or only a
placeholder). The editor shows them as selectable, range-editable markers.

### 9.1 `group`

A logical container of other objects by **index** into the top-level `objects`
array. Members render themselves; the group just bundles them (and can override
their frame range).

```json
{ "type": "group", "members": [1, 2, 3], "frames": null, "z_order": 0 }
```

| Field | Type | Default | Notes |
|-------|------|---------|-------|
| `members` | array of ints | **required** | indices into `objects` |
| `frames` | FrameRange or omit | omit (*auto*) | see below |
| `z_order` | int | `0` | |

- **Auto** (`frames` omitted/`null`): the group has no range of its own; members
  render on their own ranges. The group's effective span is the union of its
  members'. This is the normal case.
- **Explicit** (`frames` set): the range **overrides every member** — each member
  is gated on the group's range instead of its own (can widen or narrow it).

> `members` are array **indices**, so they are fragile if you reorder/insert
> objects. When hand-authoring, prefer the explicit-range form only when you need
> it, and double-check indices after edits. (The editor maintains them for you.)

### 9.2 `command`

Runs a binary at **play time** and paints its stdout/stderr into a box. The
binary is **never** run while editing or compiling — the editor shows only a
bordered placeholder box.

```json
{
  "type": "command",
  "position": { "x": { "fixed": 2 }, "y": { "fixed": 2 } },
  "width": 50,
  "height": 12,
  "command": "kubectl",
  "args": ["get", "pods"],
  "cwd": "/home/me/project",
  "timeout_secs": 10,
  "border": true,
  "style": { "fg": "white" },
  "frames": { "start": 4, "end": 5 },
  "z_order": 0
}
```

| Field | Type | Default | Notes |
|-------|------|---------|-------|
| `position`, `width`, `height` | | **required** | the output box |
| `command` | string | **required** | program (looked up on `PATH`) |
| `args` | array of strings | `[]` | arguments |
| `cwd` | string | none | working directory (defaults to player's cwd) |
| `timeout_secs` | integer | none | kill after N seconds; omit ⇒ no timeout |
| `border` | bool | `true` | draw a border around the output region |
| `style`, `frames`, `z_order` | | | common fields |

At play time the player runs it with piped stdio (it can't touch the terminal),
paints the tail of the output into the box interior, and marks the top edge with
a green `✓` (exit 0) or red `✗` (non-zero/timeout/spawn failure). Navigation
never branches on exit status — arrow keys always work.

### 9.3 `loop`

A play-time loop: when navigation lands inside its range, the player
auto-advances across it on a timer.

```json
{ "type": "loop", "frames": { "start": 4, "end": 8 },
  "delay_ms": 500, "count": 0, "bounce": true }
```

| Field | Type | Default | Notes |
|-------|------|---------|-------|
| `frames` | FrameRange | **required** | the loop range (end exclusive) |
| `delay_ms` | integer | `500` | delay between auto-advanced frames |
| `count` | integer | `0` | times to play before moving on; `0` = forever |
| `bounce` | bool | `true` | ping-pong (`5,6,7,8,7,6,…`) vs restart (`5,6,7,8,5,…`) |

**Rules (validated at compile time and live in the editor):** loops must be
non-empty, fit within the deck, and may **not overlap or nest**, and may **not
bisect an `animation`** (a loop must contain each animation span wholly or not at
all). The presenter breaks out with the arrow keys.

### 9.4 `animation`

Owns an **animation span** and its auto-play config. It draws nothing; the motion
itself lives in objects' `Coordinate::Animated` fields, which reference this
animation by `id`. The animation is the single source of truth for *when* the
motion plays.

```json
{ "type": "animation", "id": 1, "frames": { "start": 0, "end": 5 },
  "auto_play": true, "delay_ms": 500 }
```

| Field | Type | Default | Notes |
|-------|------|---------|-------|
| `id` | integer | **required** | unique; referenced by `{"animated":{…,"anim":<id>}}` |
| `frames` | FrameRange | **required** | the span the motion interpolates over |
| `auto_play` | bool | `true` | auto-advance across the span at play time |
| `delay_ms` | integer | `500` | delay between auto-advanced frames |
| `gap_frames` | integer | `0` | editor metadata (strobe); ignored at runtime |

**How to author an animation (two halves):**
1. Add an `animation` object with a unique `id` and a `frames` span.
2. On each moving object, set the animated axis to
   `{ "animated": { "from": <start>, "to": <end>, "anim": <id> } }`.

Animations **may overlap** (unlike loops). When several auto-play animations
cover the same boundary, the effective advance delay is the **minimum** of their
`delay_ms`. A dangling `anim` reference (no matching animation) renders as static
at `from`.

> **Worked example — slide a label from column 2 to column 30 over frames 0–4:**
> ```json
> {
>   "objects": [
>     { "type": "label", "text": "→",
>       "position": { "x": { "animated": { "from": 2, "to": 30, "anim": 7 } },
>                     "y": { "fixed": 5 } },
>       "frames": { "start": 0, "end": 5 } },
>     { "type": "animation", "id": 7, "frames": { "start": 0, "end": 5 } }
>   ]
> }
> ```
> The label's `frames` should cover the animation span so it's visible the whole
> time. X and Y can share one `anim` id (reference the same id from both axes).

### 9.5 `auto_advance`  (note the underscore in the type tag)

Makes a range of frames advance to the next on their own after a delay.

```json
{ "type": "auto_advance", "frames": { "start": 2, "end": 5 }, "delay_ms": 5000 }
```

| Field | Type | Default | Notes |
|-------|------|---------|-------|
| `frames` | FrameRange | **required** | frames that auto-advance (end exclusive) |
| `delay_ms` | integer | `5000` | delay before advancing (default 5 s) |

Suppressed on the last frame and while a `loop` is driving playback. Where an
auto-play `animation` also covers a frame, the effective delay is the **minimum**
of the two. The presenter can still navigate manually at any time.

---

## 10. Authoring checklist & gotchas

- **`frames.end` is exclusive.** To cover all of an N-frame deck, use
  `{ "start": 0, "end": N }`.
- **Type tags are exact snake_case.** The two easy-to-miss ones are `h_line`
  (not `hline`) and `auto_advance`. Others: `label`, `list`, `header`, `rect`,
  `arrow`, `table`, `art`, `circle`, `morph`, `group`, `command`, `loop`,
  `animation`.
- **Only 8 named colors** exist; anything else must be `{ "rgb": [r,g,b] }`.
- **Spaces are transparent** in `art`/`morph`/`label` (no `bg`). Set a `bg` to
  make an object an opaque block for layering.
- **`group.members` and `links` are array indices** — fragile under reordering.
  Hand-edit with care; the editor manages them automatically.
- **Animations need both halves**: the `animation` object *and* the
  `Coordinate::Animated` on the moving objects, joined by a shared `id`.
- **`loop` constraints are validated**: no overlap/nesting, must not bisect an
  animation, must fit the deck. A bad loop fails compilation.
- **Don't hand-edit the compiled `out.json`** — it's regenerated from source.
- **Optional fields can be omitted** entirely; the engine fills defaults. Keep
  hand-written files terse by omitting `style`, `z_order: 0`, default flags, etc.
- **Review visually before trusting layout.** Open with `bs edit source.json`;
  the preview is the same engine that compiles and plays, so geometry you eyeball
  there is authoritative. Hand-deriving exact column math is error-prone.

---

## 11. Minimal complete example

A 3-frame deck: a title, a bulleted list that appears on frame 1, and a label
that slides in on frames 1–2.

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

Compile and play:

```bash
bs compile deck.json deck.play.json && bs play deck.play.json
# or just review live:
bs edit deck.json
```
