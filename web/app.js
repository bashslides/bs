// bs web viewer — renders a compiled presentation (the output of
// `bs compile source.json out.json`) in the browser.
//
// It is a straight port of the terminal player's *pure* parts:
//   - PlayablePresentation::grid_at  → gridAt()   (replay full + diff frames)
//   - player::loop_next              → loopNext()
//   - Player::{auto_advance_delay, animation_cluster,
//              frame_auto_advance_delay, effective_auto_delay}
// so navigation, loops, auto-play animations and auto-advance markers behave
// exactly as they do in the terminal.
//
// The one thing a browser cannot do is run `Command` objects. Their placeholder
// box is already baked into the static frames by the compiler, so the slide
// still renders; the viewer just names the command in the status bar instead of
// executing it.

'use strict';

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

const el = {
  screen: document.getElementById('screen'),   // <canvas>
  msg: document.getElementById('msg'),         // empty/error state
  stage: document.getElementById('stage'),
  name: document.getElementById('deck-name'),
  counter: document.getElementById('counter'),
  note: document.getElementById('note'),
  framebar: document.getElementById('framebar'),
  file: document.getElementById('file'),
  controls: {
    prev: document.getElementById('btn-prev'),
    next: document.getElementById('btn-next'),
    first: document.getElementById('btn-first'),
    last: document.getElementById('btn-last'),
    open: document.getElementById('btn-open'),
    full: document.getElementById('btn-full'),
    exitFull: document.getElementById('exit-full'),
  },
};

/** @type {object|null} the loaded PlayablePresentation */
let deck = null;
let frame = 0;
/** Active loop playback: {region, forward, iterations} — mirrors LoopPlay. */
let loopPlay = null;
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
const esc = (s) => s.replace(/[&<>]/g, (c) => ESC[c]);

// ---------------------------------------------------------------------------
// Frame replay (port of PlayablePresentation::grid_at)
// ---------------------------------------------------------------------------

const blankCell = { ch: ' ' };

function blankGrid(w, h) {
  return Array.from({ length: h }, () => new Array(w).fill(blankCell));
}

/** Apply one frame to `grid` in place (a Full frame replaces it). */
function applyFrame(grid, f) {
  if (!f) return grid;
  if (f.type === 'full') return f.cells.map((row) => row.slice());
  for (const c of f.changes || []) {
    if (c.y < grid.length && grid.length && c.x < grid[0].length) grid[c.y][c.x] = c.cell;
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

const ctx = el.screen.getContext('2d');

/** Width of one glyph at the current font, cached (used to stretch box art). */
const glyphWidth = new Map();

/** Box-drawing and block elements — the characters that must tile seamlessly. */
const isBoxArt = (ch) => {
  const c = ch.codePointAt(0);
  return c >= 0x2500 && c <= 0x259f;
};

/** Measure the resolved monospace font once, in font-size-relative units. */
function measure() {
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

  fontPx = Math.max(4, Math.min(availW / (cols * ADV), availH / (rows * LINE)));
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
      const cell = row[x];
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

      const ch = cell.ch;
      if (!ch || ch === ' ') continue;

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
  const cmd = (deck.commands || []).find((c) => covers(c, frame));
  if (cmd) bits.push(`command: ${cmd.command} (not run in the browser)`);
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
        stopLoop();
        show(tickRange(i, barTotal)[0]);
        armLoop(null);
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

/** Begin the loop on the current frame, unless `exclude` is that same span. */
function armLoop(exclude) {
  if (loopPlay) return;
  const region = loopRegionFor(frame);
  if (!region) return;
  if (exclude && exclude[0] === region.start_frame && exclude[1] === region.end_frame) return;
  loopPlay = { region, forward: true, iterations: 0 };
}

/** (Re)arm the single timer: a loop drives playback if one is active. */
function reschedule() {
  if (timer) { clearTimeout(timer); timer = null; }
  if (!deck) return;
  if (loopPlay) {
    const d = autoAdvanceDelay(frame, loopPlay.forward) ?? loopPlay.region.delay_ms;
    timer = setTimeout(loopTick, Math.max(1, d));
  } else {
    const d = effectiveAutoDelay(frame);
    if (d != null) timer = setTimeout(autoTick, Math.max(1, d));
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
  const iterations = loopPlay.iterations + (completed ? 1 : 0);
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
    if (cluster) show(clamp(cluster[1]));
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
  stopLoop();
  show(n);
  armLoop(null);
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

document.addEventListener('keydown', (e) => {
  if (e.metaKey || e.ctrlKey || e.altKey) return;
  const k = e.key;
  if (k === 'o' || k === 'O') { el.file.click(); e.preventDefault(); return; }
  if (!deck) return;
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
      if (document.body.classList.contains('bare')) setBare(false);
      return; // let the browser handle its own fullscreen exit too
    default:
      return;
  }
  e.preventDefault();
});

document.addEventListener('fullscreenchange', () => {
  if (!document.fullscreenElement) setBare(false);
});

// Bottom-bar controls. Each mirrors a key, so a phone can reach everything the
// keyboard can. They sit in #foot, outside #stage, so pressing one never also
// triggers the tap-to-step handler on the canvas.
const c = el.controls;
c.prev.addEventListener('click', () => deck && back());
c.next.addEventListener('click', () => deck && forward());
c.first.addEventListener('click', () => deck && jumpTo(0));
c.last.addEventListener('click', () => deck && jumpTo(deck.frames.length - 1));
c.open.addEventListener('click', () => el.file.click());
c.full.addEventListener('click', () => toggleFullscreen());
c.exitFull.addEventListener('click', () => toggleFullscreen(false));

// Tap/click the canvas to step (left third goes back, the rest forward).
el.stage.addEventListener('click', (e) => {
  if (!deck) return;
  const box = el.stage.getBoundingClientRect();
  if (e.clientX < box.left + box.width / 3) back();
  else forward();
});

// ---------------------------------------------------------------------------
// Loading
// ---------------------------------------------------------------------------

function showCanvas() {
  el.screen.hidden = false;
  el.msg.hidden = true;
}

function message(lines) {
  el.screen.hidden = true;
  el.msg.hidden = false;
  el.msg.innerHTML = lines.join('\n');
  el.counter.textContent = '--/--';
  el.note.textContent = '';
  el.framebar.innerHTML = '';
}

function load(data, name) {
  if (!data || !data.contract || !Array.isArray(data.frames) || !data.frames.length) {
    message([
      '  not a compiled bs presentation',
      '',
      '  expected the output of:  <b>bs compile source.json out.json</b>',
    ]);
    return;
  }
  deck = data;
  frame = 0;
  loopPlay = null;
  cache = { frame: -1, grid: null };
  tickStarts = [];
  barTotal = -1;
  el.name.textContent = name;
  document.title = `${name} — bs`;
  showCanvas();
  layout();
  render();
  armLoop(null);
  reschedule();
}

function loadFile(file) {
  const reader = new FileReader();
  reader.onload = () => {
    try {
      load(JSON.parse(String(reader.result)), file.name);
    } catch (err) {
      message([`  could not parse ${esc(file.name)}`, '', `  ${esc(String(err.message))}`]);
    }
  };
  reader.readAsText(file);
}

el.file.addEventListener('change', () => {
  const f = el.file.files && el.file.files[0];
  if (f) loadFile(f);
  el.file.value = '';
});

let dragDepth = 0;
window.addEventListener('dragenter', (e) => {
  e.preventDefault();
  dragDepth++;
  document.body.classList.add('dragging');
});
window.addEventListener('dragover', (e) => e.preventDefault());
window.addEventListener('dragleave', () => {
  if (--dragDepth <= 0) { dragDepth = 0; document.body.classList.remove('dragging'); }
});
window.addEventListener('drop', (e) => {
  e.preventDefault();
  dragDepth = 0;
  document.body.classList.remove('dragging');
  const f = e.dataTransfer && e.dataTransfer.files && e.dataTransfer.files[0];
  if (f) loadFile(f);
});

window.addEventListener('resize', layout);
window.addEventListener('orientationchange', () => setTimeout(layout, 200));
// Metrics can shift once webfonts settle; re-measure when they do.
if (document.fonts && document.fonts.ready) {
  document.fonts.ready.then(() => { measure(); layout(); });
}
if (window.ResizeObserver) new ResizeObserver(layout).observe(el.stage);

async function boot() {
  measure();
  const src = new URLSearchParams(location.search).get('deck') || 'presentation.json';

  // `?deck=session` is the hand-off from the compile page, which stashes the
  // freshly compiled deck in sessionStorage rather than round-tripping a file.
  if (src === 'session') {
    // The compile page writes to whichever store it could use, so read both.
    let stashed = null;
    for (const get of [() => sessionStorage, () => localStorage]) {
      try {
        stashed = get().getItem('bs:deck');
        if (stashed) break;
      } catch {
        // storage blocked (private mode, file://) — try the next one
      }
    }
    if (stashed) {
      try {
        load(JSON.parse(stashed), 'compiled deck');
        return;
      } catch (err) {
        message([`  the handed-over deck could not be parsed`, '', `  ${esc(String(err.message))}`]);
        return;
      }
    }
    message([
      '  nothing handed over',
      '',
      '  compile a deck first, then press <b>present →</b>',
      '  (or drop a compiled presentation here / press <b>o</b>)',
    ]);
    return;
  }

  message([`  loading ${esc(src)} …`]);
  try {
    const res = await fetch(src, { cache: 'no-cache' });
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    load(await res.json(), src.split('/').pop());
  } catch (err) {
    message([
      '  no deck loaded',
      '',
      '  drop a compiled presentation here, or press <b>o</b> to open one',
      '  (or pass one by URL:  <b>?deck=path/to/out.json</b>)',
      '',
      `  <b>bs compile source.json out.json</b>`,
      '',
      `  ${esc(String(err.message))}`,
    ]);
  }
}

boot();
