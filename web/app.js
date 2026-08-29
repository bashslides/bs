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

// The 8 NamedColors, in a muted terminal palette.
const PALETTE = {
  black: '#1c1c1c', red: '#d75f5f', green: '#87af5f', yellow: '#d7af5f',
  blue: '#5f87d7', magenta: '#af87d7', cyan: '#5fafaf', white: '#e4e4e4',
};

const el = {
  screen: document.getElementById('screen'),
  probe: document.getElementById('probe'),
  stage: document.getElementById('stage'),
  name: document.getElementById('deck-name'),
  counter: document.getElementById('counter'),
  note: document.getElementById('note'),
  framebar: document.getElementById('framebar'),
  file: document.getElementById('file'),
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

const styleCache = new WeakMap();

/** A Style → inline CSS. Empty string for the default style. */
function styleCss(s) {
  if (!s) return '';
  const hit = styleCache.get(s);
  if (hit !== undefined) return hit;
  let out = '';
  const fg = cssColor(s.fg);
  const bg = cssColor(s.bg);
  if (fg) out += `color:${fg};`;
  if (bg) out += `background:${bg};`;
  if (s.bold) out += 'font-weight:700;';
  if (s.dim) out += 'opacity:.55;';
  styleCache.set(s, out);
  return out;
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

/** Rasterize a cell grid to HTML, coalescing runs that share a style. */
function gridHtml(grid) {
  let html = '';
  for (const row of grid) {
    let i = 0;
    while (i < row.length) {
      const css = styleCss(row[i].style);
      let buf = '';
      let j = i;
      while (j < row.length && styleCss(row[j].style) === css) {
        buf += row[j].ch;
        j++;
      }
      html += css ? `<span style="${css}">${esc(buf)}</span>` : esc(buf);
      i = j;
    }
    html += '\n';
  }
  return html;
}

function render() {
  el.screen.innerHTML = gridHtml(gridFor(frame));
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

function renderFrameBar() {
  const total = deck.frames.length;
  if (el.framebar.childElementCount !== total) {
    el.framebar.innerHTML = '';
    for (let i = 0; i < total; i++) {
      const tick = document.createElement('i');
      tick.addEventListener('click', () => {
        stopLoop();
        show(i);
        armLoop(null);
        reschedule();
      });
      el.framebar.appendChild(tick);
    }
  }
  const kids = el.framebar.children;
  for (let i = 0; i < total; i++) {
    const inRegion =
      (deck.loops || []).some((r) => covers(r, i)) ||
      (deck.animations || []).some((r) => covers(r, i)) ||
      (deck.auto_advances || []).some((r) => covers(r, i));
    kids[i].className = i === frame ? 'current' : inRegion ? 'region' : '';
  }
}

/** Scale the canvas so the whole grid fits the stage. */
let CW = 0; // char width at font-size 1px
let LH = 0; // line height at font-size 1px

function measure() {
  el.probe.textContent = 'M'.repeat(10) + '\n' + 'M'.repeat(10);
  const r = el.probe.getBoundingClientRect();
  CW = r.width / 10 / 100;
  LH = r.height / 2 / 100;
}

function fit() {
  if (!deck || !CW || !LH) return;
  const cols = deck.contract.width;
  const rows = deck.contract.height;
  const cs = getComputedStyle(el.stage);
  const availW = el.stage.clientWidth - parseFloat(cs.paddingLeft) - parseFloat(cs.paddingRight);
  const availH = el.stage.clientHeight - parseFloat(cs.paddingTop) - parseFloat(cs.paddingBottom);
  const size = Math.max(4, Math.min(availW / (cols * CW), availH / (rows * LH)));
  el.screen.style.fontSize = size + 'px';
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
  fit();
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
      setBare(!document.body.classList.contains('bare'));
      if (document.body.classList.contains('bare')) {
        document.documentElement.requestFullscreen?.().catch(() => {});
      } else if (document.fullscreenElement) {
        document.exitFullscreen?.().catch(() => {});
      }
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

// Tap/click the canvas to step (left third goes back, the rest forward).
el.screen.addEventListener('click', (e) => {
  if (!deck) return;
  const box = el.screen.getBoundingClientRect();
  if (e.clientX < box.left + box.width / 3) back();
  else forward();
});

// ---------------------------------------------------------------------------
// Loading
// ---------------------------------------------------------------------------

function message(lines) {
  el.screen.style.fontSize = '';
  el.screen.innerHTML = `<span class="msg">${lines.join('\n')}</span>`;
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
  el.name.textContent = name;
  document.title = `${name} — bs`;
  render();
  fit();
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
document.getElementById('open').addEventListener('click', () => el.file.click());

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

window.addEventListener('resize', fit);
// Metrics can shift once webfonts settle; re-measure when they do.
if (document.fonts && document.fonts.ready) {
  document.fonts.ready.then(() => { measure(); fit(); });
}
if (window.ResizeObserver) new ResizeObserver(fit).observe(el.stage);

async function boot() {
  measure();
  const src = new URLSearchParams(location.search).get('deck') || 'presentation.json';

  // `?deck=session` is the hand-off from the compile page, which stashes the
  // freshly compiled deck in sessionStorage rather than round-tripping a file.
  if (src === 'session') {
    let stashed = null;
    try {
      stashed = sessionStorage.getItem('bs:deck');
    } catch {
      // storage blocked (private mode) — fall through to the empty state
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
