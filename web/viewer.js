// The present view: plays a compiled deck on a canvas.
//
// The playback rules are a port of the terminal player's *pure* logic —
// `PlayablePresentation::grid_at` → gridAt, `player::loop_next` → loopNext, and
// `Player::{auto_advance_delay, animation_cluster, frame_auto_advance_delay,
// effective_auto_delay}` → the same-named camelCase functions, and
// `Cell::printable`/`narrow_char` (src/types.rs) → printable/narrowChar. Change
// those in Rust and mirror them here; `cargo test` cannot see this file.
//
// The grid is drawn one `fillText` per cell at an exact cell origin, never as
// runs of text: a run only lines up if the device resolves a truly monospaced
// font that also covers every character used, and on mobile neither holds.

'use strict';

// bs web viewer — renders a compiled presentation (the output of
// `bs compile source.json out.json`) in the browser.
//
// It is a straight port of the terminal player's *pure* parts:
//   - PlayablePresentation::grid_at  → gridAt()   (replay full + diff frames)
//   - player::loop_next              → loopNext()
//   - player::loop_step              → loopTick() (pass counting)
//   - Player::loop_wait              → loopWait()
//   - Player::{auto_advance_delay, animation_cluster,
//              frame_auto_advance_delay, effective_auto_delay}
//   - Cell::printable / narrow_char (src/types.rs) → printable() / narrowChar()
// so navigation, loops, auto-play animations and auto-advance markers behave
// exactly as they do in the terminal.
//
// The one thing a browser cannot do is run `Command` objects. Their placeholder
// box is already baked into the static frames by the compiler, so the slide
// still renders; the viewer just names the command in the status bar instead of
// executing it.

const FRAMES_PER_JUMP = 10; // matches player::FRAMES_PER_JUMP

// Kept in sync with --mono in style.css. "Courier New" is the last real family
// before the generic keyword because it is present on essentially every mobile
// platform, so the stack lands on a genuine fixed-pitch face rather than the
// system UI font.
const FONT_STACK = 'ui-monospace, SFMono-Regular, "SF Mono", Menlo, Monaco, ' +
  'Consolas, "DejaVu Sans Mono", "Liberation Mono", "Courier New", monospace';
const DEFAULT_FG = '#d0d0d0';

// The 8 NamedColors, in a muted terminal palette.
const PALETTE = {
  black: '#1c1c1c', red: '#d75f5f', green: '#87af5f', yellow: '#d7af5f',
  blue: '#5f87d7', magenta: '#af87d7', cyan: '#5fafaf', white: '#e4e4e4',
};

const el = {};

/** Bind the view's elements once the shell has rendered them. */
function bindElements() {
  const id = (n) => document.getElementById(n);
  Object.assign(el, {
    screen: id('screen'),
    msg: id('msg'),
    stage: id('stage'),
    counter: id('counter'),
    note: id('note'),
    framebar: id('framebar'),
    controls: {
      prev: id('btn-prev'), next: id('btn-next'),
      first: id('btn-first'), last: id('btn-last'),
      full: id('btn-full'), exitFull: id('exit-full'),
    },
  });
}


/** @type {object|null} the loaded PlayablePresentation */
let deck = null;
let frame = 0;
/** Active loop playback: {region, forward, iterations, seenStart} — mirrors
 *  LoopPlay/LoopState (a pass counts only once the loop has shown its start). */
let loopPlay = null;
/** Span of a loop just broken out of / finished — mirrors `loop_exclude`. */
let loopExclude = null;
let timer = null;
/** Incremental grid cache so stepping forward doesn't replay from frame 0. */
let cache = { frame: -1, grid: null };

// ---------------------------------------------------------------------------
// Colour / style
// ---------------------------------------------------------------------------

function cssColor(c) {
  if (c == null) return null;
  if (typeof c === 'string') return PALETTE[c] || null;
  if (typeof c.r === 'number') return `rgb(${c.r},${c.g},${c.b})`;
  if (Array.isArray(c.rgb)) return `rgb(${c.rgb[0]},${c.rgb[1]},${c.rgb[2]})`;
  return null;
}

const ESC = { '&': '&amp;', '<': '&lt;', '>': '&gt;' };
export const esc = (s) => String(s).replace(/[&<>]/g, (c) => ESC[c]);

// ---------------------------------------------------------------------------
// Frame replay (port of PlayablePresentation::grid_at)
// ---------------------------------------------------------------------------

const blankCell = { ch: ' ' };

function blankGrid(w, h) {
  return Array.from({ length: h }, () => new Array(w).fill(blankCell));
}

/** Apply one frame to `grid`, keeping its shape (port of `Frame::apply_to`):
 *  a Full frame yields a fresh copy, missing cells blank; a diff writes in place,
 *  skipping changes outside the grid. */
function applyFrame(grid, f) {
  if (!f) return grid;
  if (f.type === 'full') {
    return grid.map((row, y) => row.map((_, x) => (f.cells && f.cells[y] && f.cells[y][x]) || blankCell));
  }
  for (const c of f.changes || []) {
    const row = grid[c.y];
    if (row && c.x < row.length) grid[c.y][c.x] = c.cell;
  }
  return grid;
}

function gridAt(p, n) {
  const grid = blankGrid(p.contract.width, p.contract.height);
  if (!p.frames.length) return grid;
  const last = Math.min(n, p.frames.length - 1);
  let g = grid;
  for (let i = 0; i <= last; i++) g = applyFrame(g, p.frames[i]);
  return g;
}

/** `gridAt`, but reusing the cached grid when stepping forward. */
function gridFor(n) {
  n = clamp(n);
  if (cache.grid && n >= cache.frame) {
    let g = cache.grid;
    for (let i = cache.frame + 1; i <= n; i++) g = applyFrame(g, deck.frames[i]);
    cache = { frame: n, grid: g };
    return g;
  }
  const g = gridAt(deck, n);
  cache = { frame: n, grid: g };
  return g;
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

// The deck is a fixed character grid, so it is drawn on a canvas with one
// `fillText` per cell at an exact cell origin — never as a run of text.
//
// Why: a run of text only lines up if the font is truly monospaced AND has a
// glyph for every character used. On mobile, neither holds reliably — the
// platform's "monospace" can resolve to a proportional face, and box-drawing
// characters (─ │ ┌ █ …) routinely come from a *fallback* font with different
// metrics. Either one shears the grid apart, most visibly as spaces that are
// narrower than everything else. Positioning each cell ourselves makes the
// layout independent of whatever font the device actually picks.

/** Advance width and line height per 1px of font-size, for the resolved font. */
let ADV = 0.6;
let LINE = 1.2;

/** Cell geometry in CSS pixels, recomputed by layout(). */
let cellW = 0;
let cellH = 0;
let fontPx = 0;

let ctx = null;   // set by bindElements' caller, once #screen exists

/** Width of one glyph at the current font, cached (used to stretch box art). */
const glyphWidth = new Map();

// Port of `narrow_char` / `Cell::printable` (src/types.rs): the char a cell
// shows, exactly one column wide like in `bs play` and the editor — a control
// or known zero-width char (combining mark, joiner, variation selector) as a
// blank, and a char not known to be single-width (CJK, emoji) as `?`. Keep the
// tables in step with src/types.rs.
const ZERO_WIDTH = [[0x0300, 0x036F], [0x0483, 0x0489], [0x1AB0, 0x1AFF], [0x1DC0, 0x1DFF],
  [0x200B, 0x200F], [0x202A, 0x202E], [0x2060, 0x206F], [0x20D0, 0x20FF], [0xFE00, 0xFE0F],
  [0xFE20, 0xFE2F], [0xFEFF, 0xFEFF], [0x1F3FB, 0x1F3FF], [0xE0000, 0xE007F], [0xE0100, 0xE01EF]];
const WIDE_SYMBOLS = [[0x2614, 0x2615], [0x2630, 0x2637], [0x268A, 0x268F], [0x2648, 0x2653],
  [0x267F, 0x267F], [0x2693, 0x2693], [0x26A1, 0x26A1], [0x26AA, 0x26AB], [0x26BD, 0x26BE],
  [0x26C4, 0x26C5], [0x26CE, 0x26CE], [0x26D4, 0x26D4], [0x26EA, 0x26EA], [0x26F2, 0x26F3],
  [0x26F5, 0x26F5], [0x26FA, 0x26FA], [0x26FD, 0x26FD], [0x2705, 0x2705], [0x270A, 0x270B],
  [0x2728, 0x2728], [0x274C, 0x274C], [0x274E, 0x274E], [0x2753, 0x2755], [0x2757, 0x2757],
  [0x2795, 0x2797], [0x27B0, 0x27B0], [0x27BF, 0x27BF]];
const NARROW = [[0x20, 0x7E], [0xA0, 0x2FF], [0x370, 0x377], [0x37A, 0x37F], [0x384, 0x38A],
  [0x38C, 0x38C], [0x38E, 0x3A1], [0x3A3, 0x482], [0x48A, 0x52F], [0x1E00, 0x1EFF],
  [0x2010, 0x2027], [0x2030, 0x205E], [0x20A0, 0x20BF], [0x2100, 0x214F], [0x2190, 0x22FF],
  [0x2300, 0x2300], [0x2500, 0x25FC], [0x2600, 0x27BF], [0x2800, 0x28FF]];
const inRanges = (c, ranges) => ranges.some(([lo, hi]) => c >= lo && c <= hi);

/** `narrow_char`: the one-column char for code point `c`, or null if zero-width. */
function narrowChar(c) {
  if (inRanges(c, ZERO_WIDTH)) return null;
  if (inRanges(c, WIDE_SYMBOLS)) return '?';
  return inRanges(c, NARROW) ? String.fromCodePoint(c) : '?';
}

/** `Cell::printable`: what a cell with char `ch` paints. */
export function printable(ch) {
  const c = ch ? ch.codePointAt(0) : 0x20;
  if (c <= 0x1F || (c >= 0x7F && c <= 0x9F)) return ' ';   // Rust `char::is_control`
  return narrowChar(c) ?? ' ';
}

/** Box-drawing and block elements — the characters that must tile seamlessly. */
const isBoxArt = (ch) => {
  const c = ch.codePointAt(0);
  return c >= 0x2500 && c <= 0x259f;
};

/** Measure the resolved monospace font once, in font-size-relative units. */
function measure() {
  if (!ctx) return;
  ctx.font = `100px ${FONT_STACK}`;
  ADV = ctx.measureText('M').width / 100 || 0.6;
  LINE = 1.2;
  glyphWidth.clear();
}

/** Size the canvas so the whole grid fits the stage, crisp on any DPI. */
function layout() {
  if (!deck) return;
  const cols = deck.contract.width;
  const rows = deck.contract.height;
  const cs = getComputedStyle(el.stage);
  const availW = el.stage.clientWidth - parseFloat(cs.paddingLeft) - parseFloat(cs.paddingRight);
  const availH = el.stage.clientHeight - parseFloat(cs.paddingTop) - parseFloat(cs.paddingBottom);
  if (availW <= 0 || availH <= 0) return;

  // The fit always wins: a legibility floor would make a large deck's canvas
  // wider/taller than the stage, whose overflow is hidden, silently cutting
  // off edge columns or rows. Guard only against 0/NaN (an empty contract).
  fontPx = Math.min(availW / (cols * ADV), availH / (rows * LINE));
  if (!(fontPx > 0 && Number.isFinite(fontPx))) return;
  cellW = fontPx * ADV;
  cellH = fontPx * LINE;

  const dpr = window.devicePixelRatio || 1;
  el.screen.style.width = `${cols * cellW}px`;
  el.screen.style.height = `${rows * cellH}px`;
  el.screen.width = Math.round(cols * cellW * dpr);
  el.screen.height = Math.round(rows * cellH * dpr);
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  glyphWidth.clear();
  if (deck) {
    paint(gridFor(frame));
    renderFrameBar();
  }
}

/** Draw one cell grid onto the canvas. */
function paint(grid) {
  const cols = deck.contract.width;
  const rows = deck.contract.height;

  ctx.clearRect(0, 0, cols * cellW, rows * cellH);
  ctx.textBaseline = 'middle';
  ctx.textAlign = 'center';

  for (let y = 0; y < rows && y < grid.length; y++) {
    const row = grid[y];
    const py = y * cellH;
    for (let x = 0; x < cols && x < row.length; x++) {
      const cell = row[x] || blankCell;
      const st = cell.style;
      const px = x * cellW;

      if (st) {
        const bg = cssColor(st.bg);
        if (bg) {
          ctx.fillStyle = bg;
          // Overdraw by a hair so neighbouring fills never leave a seam.
          ctx.fillRect(px, py, cellW + 0.5, cellH + 0.5);
        }
      }

      const ch = printable(cell.ch);
      if (ch === ' ') continue;

      const bold = !!(st && st.bold);
      ctx.font = `${bold ? 'bold ' : ''}${fontPx}px ${FONT_STACK}`;
      ctx.fillStyle = (st && cssColor(st.fg)) || DEFAULT_FG;
      ctx.globalAlpha = st && st.dim ? 0.55 : 1;

      // A glyph pulled from a fallback font can be narrower than the cell,
      // which would break long runs of ─ or █ into dashes. Stretch just those
      // to the full cell width; ordinary text is left at its natural width.
      if (isBoxArt(ch)) {
        const key = `${ch}|${bold}`;
        let w = glyphWidth.get(key);
        if (w === undefined) {
          w = ctx.measureText(ch).width;
          glyphWidth.set(key, w);
        }
        if (w > 0.1 && Math.abs(w - cellW) > 0.5) {
          ctx.save();
          ctx.translate(px + cellW / 2, py + cellH / 2);
          ctx.scale(cellW / w, 1);
          ctx.fillText(ch, 0, 0);
          ctx.restore();
          ctx.globalAlpha = 1;
          continue;
        }
      }

      ctx.fillText(ch, px + cellW / 2, py + cellH / 2);
      ctx.globalAlpha = 1;
    }
  }
}

function render() {
  showCanvas();
  paint(gridFor(frame));
  const total = deck.frames.length;
  el.counter.textContent = `${String(frame + 1).padStart(2, '0')}/${String(total).padStart(2, '0')}`;
  el.note.textContent = statusNote();
  renderFrameBar();
}

/** A short dim note about play-time behaviour on the current frame. */
function statusNote() {
  const bits = [];
  // Every command box on the slide, not only the first (mirrors `regions_for`).
  const cmds = (deck.commands || []).filter((c) => covers(c, frame)).map((c) => c.command);
  if (cmds.length) bits.push(`${cmds.length > 1 ? 'commands' : 'command'}: ${cmds.join(', ')} (not run in the browser)`);
  if (loopPlay) bits.push(`loop ${loopPlay.region.start_frame}-${loopPlay.region.end_frame - 1}`);
  else if (loopRegionFor(frame)) bits.push('loop');
  else if (animationCluster(frame)) bits.push('animating');
  else if (frameAutoAdvanceDelay(frame) != null) bits.push('auto-advance');
  return bits.join('  ·  ');
}

// The frame bar shows one tick per frame only while that actually fits. A tick
// cannot render below ~1px, so a long deck's ticks would otherwise overflow the
// bar, widen the page, and — because the horizontal scrollbar steals width from
// the stage — shrink the deck itself. Past the limit, each tick stands for a
// contiguous *range* of frames instead, so the bar always fits exactly.
const TICK_MIN = 2;   // px, smallest tick that still reads as a mark
const TICK_GAP = 1;   // px, must match the `gap` on #framebar in style.css

/** First frame of each tick. Length is the tick count. */
let tickStarts = [];
let barTotal = -1;

/** Frame index each tick starts at, for a bar `total` frames long. */
function frameBarBuckets(total) {
  const w = el.framebar.clientWidth;
  // Before the first layout clientWidth is 0; one tick per frame is the right
  // guess then, and the next render (after layout) corrects it.
  const fits = w > 0 ? Math.max(1, Math.floor((w + TICK_GAP) / (TICK_MIN + TICK_GAP))) : total;
  const ticks = Math.max(1, Math.min(total, fits));
  const starts = new Array(ticks);
  for (let i = 0; i < ticks; i++) starts[i] = Math.floor((i * total) / ticks);
  return starts;
}

/** The half-open frame range tick `i` stands for. */
const tickRange = (i, total) => [tickStarts[i], i + 1 < tickStarts.length ? tickStarts[i + 1] : total];

function renderFrameBar() {
  const total = deck.frames.length;
  const starts = frameBarBuckets(total);

  if (starts.length !== tickStarts.length || total !== barTotal) {
    tickStarts = starts;
    barTotal = total;
    el.framebar.innerHTML = '';
    for (let i = 0; i < tickStarts.length; i++) {
      const tick = document.createElement('i');
      tick.addEventListener('click', () => {
        const left = leaveLoop();
        show(tickRange(i, barTotal)[0]);
        armLoop(left);
        reschedule();
      });
      el.framebar.appendChild(tick);
    }
  }

  const kids = el.framebar.children;
  const overlaps = (rs, lo, hi) => rs.some((r) => r.start_frame < hi && lo < r.end_frame);
  for (let i = 0; i < tickStarts.length; i++) {
    const [lo, hi] = tickRange(i, total);
    const inRegion =
      overlaps(deck.loops || [], lo, hi) ||
      overlaps(deck.animations || [], lo, hi) ||
      overlaps(deck.auto_advances || [], lo, hi);
    kids[i].className = frame >= lo && frame < hi ? 'current' : inRegion ? 'region' : '';
  }
}

// ---------------------------------------------------------------------------
// Play-time regions (ports of the Player's pure helpers)
// ---------------------------------------------------------------------------

const covers = (r, f) => r.start_frame <= f && f < r.end_frame;
const clamp = (n) => Math.max(0, Math.min(n, deck.frames.length - 1));
const min = (xs) => (xs.length ? Math.min(...xs) : null);

function loopRegionFor(f) {
  return (deck.loops || []).find((l) => covers(l, f)) || null;
}

/** Min delay_ms over auto-play animations covering the `from`→ boundary. */
function autoAdvanceDelay(from, forward) {
  const lo = forward ? from : from - 1;
  if (lo < 0) return null;
  return min(
    (deck.animations || [])
      .filter((a) => a.auto_play && a.start_frame <= lo && lo + 1 < a.end_frame)
      .map((a) => a.delay_ms),
  );
}

/** Merged span [lo, hi) of auto-play animations connected by overlap to `f`. */
function animationCluster(f) {
  const auto = (deck.animations || []).filter((a) => a.auto_play);
  let lo = Infinity;
  let hi = 0;
  for (const a of auto) {
    if (covers(a, f)) {
      lo = Math.min(lo, a.start_frame);
      hi = Math.max(hi, a.end_frame);
    }
  }
  if (lo === Infinity) return null;
  for (let grew = true; grew; ) {
    grew = false;
    for (const a of auto) {
      if (a.start_frame < hi && lo < a.end_frame) {
        if (a.start_frame < lo) { lo = a.start_frame; grew = true; }
        if (a.end_frame > hi) { hi = a.end_frame; grew = true; }
      }
    }
  }
  return [lo, hi];
}

/** Min delay_ms over AutoAdvance markers covering `f`; never on the last frame. */
function frameAutoAdvanceDelay(f) {
  if (f + 1 >= deck.frames.length) return null;
  return min((deck.auto_advances || []).filter((a) => covers(a, f)).map((a) => a.delay_ms));
}

function effectiveAutoDelay(f) {
  return min([autoAdvanceDelay(f, true), frameAutoAdvanceDelay(f)].filter((d) => d != null));
}

/** Port of player::loop_next → [next, nextForward, completedPass]. */
function loopNext(start, end, current, forward, bounce) {
  const last = Math.max(0, end - 1);
  let next;
  let nextForward;
  if (!bounce) {
    [next, nextForward] = current < last ? [current + 1, true] : [start, true];
  } else if (last === start) {
    [next, nextForward] = [start, true];
  } else if (forward) {
    [next, nextForward] = current < last ? [current + 1, true] : [last - 1, false];
  } else if (current > start) {
    [next, nextForward] = [current - 1, false];
  } else {
    [next, nextForward] = [start + 1, true];
  }
  return [next, nextForward, next === start];
}

/** Port of Player::loop_wait: the auto-play delay of the boundary the loop's
 *  next step from `f` actually crosses (back the way it came at a bounce
 *  turnaround), else the loop's own delay (a wrap, a one-frame loop, a gap). */
function loopWait(r, f, forward) {
  const [after] = loopNext(r.start_frame, r.end_frame, f, forward, r.bounce);
  let d = null;
  if (after === f + 1) d = autoAdvanceDelay(f, true);
  else if (r.bounce && after + 1 === f) d = autoAdvanceDelay(f, false);
  return d ?? r.delay_ms;
}

// ---------------------------------------------------------------------------
// Playback
// ---------------------------------------------------------------------------

function show(n) {
  frame = clamp(n);
  render();
}

function stopLoop() {
  loopPlay = null;
}

/** Tear down the running loop and return its span, for `armLoop` — mirrors
 *  `leave_loop`, so a jump landing back inside the loop just left keeps it off. */
function leaveLoop() {
  const r = loopPlay && loopPlay.region;
  loopPlay = null;
  return r ? [r.start_frame, r.end_frame] : null;
}

/** Begin the loop on the current frame. `exclude` (after a break-out or a
 *  finished count) is remembered until the frame leaves that span, so the loop
 *  just left cannot restart while playback stays inside it — mirrors `arm_loop`. */
function armLoop(exclude) {
  if (exclude) loopExclude = exclude;
  if (loopExclude && (frame < loopExclude[0] || frame >= loopExclude[1])) loopExclude = null;
  if (loopPlay) return;
  const region = loopRegionFor(frame);
  if (!region) return;
  if (loopExclude && loopExclude[0] === region.start_frame && loopExclude[1] === region.end_frame) return;
  loopPlay = { region, forward: true, iterations: 0, seenStart: frame === region.start_frame };
}

// setTimeout converts its delay with ToInt32, so a delay of 2^31 ms or more
// wraps (often to 0) and would fire at once. Such a delay (~24.8 days) means
// "wait for the presenter", like the terminal player's full Duration wait.
const MAX_TIMEOUT_MS = 2147483647;

/** setTimeout for a deck delay (at least 1 ms); null when it is too long to arm. */
function armTimer(fn, d) {
  // A NaN delay would make setTimeout fire at once; treat it as "no timer".
  if (!Number.isFinite(d) || d > MAX_TIMEOUT_MS) return null;
  return setTimeout(fn, Math.max(1, d));
}

/** (Re)arm the single timer: a loop drives playback if one is active. */
function reschedule() {
  if (timer) { clearTimeout(timer); timer = null; }
  if (!deck) return;
  if (loopPlay) {
    timer = armTimer(loopTick, loopWait(loopPlay.region, frame, loopPlay.forward));
  } else {
    const d = effectiveAutoDelay(frame);
    if (d != null) timer = armTimer(autoTick, d);
  }
}

function autoTick() {
  if (frame < deck.frames.length - 1) {
    show(frame + 1);
    armLoop(null);
  }
  reschedule();
}

function loopTick() {
  if (!loopPlay) return;
  const r = loopPlay.region;
  const [next, nextForward, completed] = loopNext(
    r.start_frame, r.end_frame, frame, loopPlay.forward, r.bounce,
  );
  // Port of loop_step: a return to the start is a full pass only if the loop
  // has been on its start since it was entered (not when entered mid-loop).
  const iterations = loopPlay.iterations + (completed && loopPlay.seenStart ? 1 : 0);
  // A finite loop that has played its full count continues just past the loop.
  if (r.count && iterations >= r.count) {
    stopLoop();
    show(clamp(r.end_frame));
    armLoop([r.start_frame, r.end_frame]);
    reschedule();
    return;
  }
  loopPlay.forward = nextForward;
  loopPlay.iterations = iterations;
  loopPlay.seenStart = loopPlay.seenStart || completed;
  show(next);
  reschedule();
}

// ---------------------------------------------------------------------------
// Navigation (mirrors the player's key handling)
// ---------------------------------------------------------------------------

function forward() {
  if (loopPlay) {
    const r = loopPlay.region;
    stopLoop();
    show(clamp(r.end_frame));
    armLoop([r.start_frame, r.end_frame]);
  } else {
    const cluster = animationCluster(frame);
    // Land on the animation's OWN last frame — its finished state — rather than
    // stepping past it. `end_frame` is exclusive, hence -1. Once already there,
    // fall through to a normal step so the animation cannot trap the deck.
    const endOfAnim = cluster ? clamp(cluster[1] - 1) : -1;
    if (cluster && frame < endOfAnim) show(endOfAnim);
    else show(frame + 1);
    armLoop(null);
  }
  reschedule();
}

function back() {
  if (loopPlay) {
    const r = loopPlay.region;
    stopLoop();
    show(Math.max(0, r.start_frame - 1));
    armLoop([r.start_frame, r.end_frame]);
  } else {
    const cluster = animationCluster(frame);
    if (cluster) show(Math.max(0, cluster[0] - 1));
    else show(frame - 1);
    armLoop(null);
  }
  reschedule();
}

function jumpTo(n) {
  const left = leaveLoop();
  show(n);
  armLoop(left);
  reschedule();
}

function setBare(on) {
  document.body.classList.toggle('bare', on);
  layout();
}

/**
 * Toggle the no-bars view, and ask for real browser fullscreen alongside it.
 *
 * The two are deliberately separate: iOS Safari refuses `requestFullscreen` on
 * a non-video element, so on iPhone only the bars-hidden half happens — which
 * is the part that matters. Leaving is what needs care there: with the bars
 * gone and no keyboard, `#exit-full` is the only way back, so it is shown
 * whenever `body.bare` is set.
 */
function toggleFullscreen(on = !document.body.classList.contains('bare')) {
  setBare(on);
  if (on) document.documentElement.requestFullscreen?.().catch(() => {});
  else if (document.fullscreenElement) document.exitFullscreen?.().catch(() => {});
}

/** Handle a key while the present view is active. Returns true if consumed. */
export function handleKey(e) {
  if (e.metaKey || e.ctrlKey || e.altKey) return false;
  const k = e.key;
  if (!deck) return false;
  const last = deck.frames.length - 1;
  switch (k) {
    case 'ArrowRight':
      // Shift+→ is a coarse ±10-frame scrub, like the terminal player.
      if (e.shiftKey) jumpTo(frame + FRAMES_PER_JUMP);
      else forward();
      break;
    case ' ': case 'Enter': case 'PageDown':
      forward();
      break;
    case 'ArrowLeft':
      if (e.shiftKey) jumpTo(frame - FRAMES_PER_JUMP);
      else back();
      break;
    case 'PageUp':
      back();
      break;
    case 'Home': jumpTo(0); break;
    case 'End': jumpTo(last); break;
    case 'f': case 'F':
      toggleFullscreen();
      break;
    case 'Escape':
      if (!document.body.classList.contains('bare')) return false;
      setBare(false);
      return true;
    default:
      return false;
  }
  e.preventDefault();
  return true;
}



// Tap/click the canvas to step (left third goes back, the rest forward).

// ---------------------------------------------------------------------------
// Public API — the shell drives the view through these
// ---------------------------------------------------------------------------

/** Wire up the view. Called once, after the shell's markup exists. */
export function init({ onOpenRequest }) {
  bindElements();
  ctx = el.screen.getContext('2d');
  measure();

  const c = el.controls;
  // Every key also has a tap target: a phone has no keyboard.
  c.prev.addEventListener('click', () => deck && back());
  c.next.addEventListener('click', () => deck && forward());
  c.first.addEventListener('click', () => deck && jumpTo(0));
  c.last.addEventListener('click', () => deck && jumpTo(deck.frames.length - 1));
  c.full.addEventListener('click', () => toggleFullscreen());
  c.exitFull.addEventListener('click', () => toggleFullscreen(false));

  // Tapping the canvas steps: left third back, the rest forward. Only taps on
  // the deck surface count — the library panel lives inside #stage too, and its
  // clicks bubble here (its delete button may already be detached by then).
  el.stage.addEventListener('click', (e) => {
    if (e.target !== el.stage && e.target !== el.screen && !el.msg.contains(e.target)) return;
    if (!deck) { onOpenRequest?.(); return; }
    const box = el.stage.getBoundingClientRect();
    if (e.clientX < box.left + box.width / 3) back();
    else forward();
  });

  document.addEventListener('fullscreenchange', () => {
    if (!document.fullscreenElement) setBare(false);
  });

  window.addEventListener('resize', layout);
  // Re-fit whenever the stage itself changes size, not only the window: the
  // footer or the bars can grow (wrapping controls, a panel), and a canvas
  // fitted to the old stage would then be clipped.
  if (typeof ResizeObserver === 'function') new ResizeObserver(() => layout()).observe(el.stage);
  window.addEventListener('orientationchange', () => setTimeout(layout, 200));
    if (document.fonts && document.fonts.ready) {
    document.fonts.ready.then(() => { measure(); layout(); });
  }
}

/** Reject a playable whose fields the viewer relies on are missing or the
 *  wrong type — the contract, every frame and cell, and the command, loop,
 *  animation and auto-advance sidecars, checked as serde requires them in
 *  src/types.rs (so `bs play` refuses such a file too) before the deck is
 *  shown or saved to the library. A bad cell would otherwise freeze navigation
 *  and a missing delay_ms race the deck to its end. Fields the viewer ignores
 *  (markers, style contents, command options) are not checked. */
function validatePlayable(d) {
  const isObj = (v) => v !== null && typeof v === 'object' && !Array.isArray(v);
  if (!isObj(d) || !isObj(d.contract) || !Array.isArray(d.frames) || !d.frames.length) {
    throw new Error('not a compiled presentation (expected the output of `bs compile`)');
  }
  const fail = (what) => { throw new Error(`malformed compiled presentation: ${what}`); };
  const uint = (v, max = Infinity) => Number.isInteger(v) && v >= 0 && v <= max;
  const u16 = (v) => uint(v, 65535);
  // `ch` is a Rust `char`: exactly one code point (one or two UTF-16 units).
  const oneChar = (s) => typeof s === 'string'
    && (s.length === 1 || (s.length === 2 && s.codePointAt(0) > 0xffff));
  const isCell = (c) => isObj(c) && oneChar(c.ch) && (c.style === undefined || isObj(c.style));
  if (!u16(d.contract.width) || !u16(d.contract.height)) fail('contract width/height');
  d.frames.forEach((f, i) => {
    const ok = isObj(f) && (
      (f.type === 'full' && Array.isArray(f.cells)
        && f.cells.every((r) => Array.isArray(r) && r.every(isCell)))
      || (f.type === 'diff' && Array.isArray(f.changes)
        && f.changes.every((c) => isObj(c) && u16(c.x) && u16(c.y) && isCell(c.cell))));
    if (!ok) fail(`frame ${i + 1}`);
  });
  const regions = (key, check) => {
    const list = d[key];
    if (list === undefined) return;
    if (!Array.isArray(list)) fail(key);
    list.forEach((r, i) => {
      if (!isObj(r) || !uint(r.start_frame) || !uint(r.end_frame) || !check(r)) fail(`${key}[${i}]`);
    });
  };
  regions('commands', (r) => typeof r.command === 'string'
    && ['x', 'y', 'w', 'h', 'status_x', 'status_y'].every((k) => u16(r[k])));
  regions('loops', (r) => uint(r.delay_ms) && (r.count === undefined || uint(r.count))
    && typeof r.bounce === 'boolean');
  regions('animations', (r) => uint(r.delay_ms) && typeof r.auto_play === 'boolean');
  regions('auto_advances', (r) => uint(r.delay_ms));
}

/** Show a compiled deck. `data` is the parsed PlayablePresentation. */
export function load(data, name) {
  validatePlayable(data);
  // Mirrors MAX_CANVAS_CELLS (src/types.rs): every frame replays onto a full grid.
  if (data.contract.width * data.contract.height > 1000000) {
    throw new Error(`canvas ${data.contract.width}×${data.contract.height} is too large (limit 1000000 cells)`);
  }
  deck = data;
  frame = 0;
  loopPlay = null;
  loopExclude = null;
  cache = { frame: -1, grid: null };
  tickStarts = [];
  barTotal = -1;
  // No on-screen deck title — the user asked for that bar gone. The document
  // title still carries it, which is what a browser tab and an installed app's
  // switcher entry show.
  document.title = name ? `${name} — bs` : 'bs';
  showCanvas();
  layout();
  render();
  armLoop(null);
  reschedule();
}

/** Replace the deck with a message (empty state, errors). Tears playback
 *  down, so no timer, tap, key or `resume()` can bring the old deck back. */
export function message(lines) {
  suspend();
  deck = null;
  loopPlay = null;
  loopExclude = null;
  cache = { frame: -1, grid: null };
  document.title = 'bs';
  el.screen.hidden = true;
  el.msg.hidden = false;
  el.msg.innerHTML = lines.join('\n');
  el.counter.textContent = '--/--';
  el.note.textContent = '';
  el.framebar.innerHTML = '';
  tickStarts = [];
  barTotal = -1;
}

/** True once a deck is showing. */
export const hasDeck = () => !!deck;

/** Stop timers — used when leaving the view; the deck stays for `resume()`. */
export function suspend() {
  if (timer) { clearTimeout(timer); timer = null; }
}

/** Re-arm playback and re-fit after becoming visible again. */
export function resume() {
  if (!deck) return;
  layout();
  render();
  reschedule();
}

function showCanvas() {
  el.screen.hidden = false;
  el.msg.hidden = true;
}
