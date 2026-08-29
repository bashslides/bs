// The compile tool: source JSON → playable JSON, run through the real engine
// compiled to WebAssembly (see wasm.js and wasm/src/lib.rs).

'use strict';

import { compile, loadCompiler, wasmSupported } from './wasm.js';

const el = (id) => document.getElementById(id);
const ui = {
  source: el('source'), status: el('status'), stats: el('stats'),
  compile: el('compile'), download: el('download'), present: el('present'),
  sample: el('sample'), open: el('open'), clear: el('clear'),
  file: el('file'), engine: el('engine'),
};

/** Where present.html looks for a deck handed over from this page. */
const HANDOFF_KEY = 'bs:deck';

let compiled = null;

function say(text, cls = 'dim') {
  ui.status.textContent = text;
  ui.status.className = cls;
}

// --------------------------------------------------------------- engine boot

// Report the engine's state up front: a compile button that silently does
// nothing is worse than one that says why.
(async () => {
  if (!wasmSupported()) {
    ui.engine.textContent = 'no WebAssembly';
    ui.engine.className = 'bad ellipsis';
    fallback('This browser cannot run WebAssembly, so compiling here is not possible.');
    return;
  }
  try {
    await loadCompiler();
    ui.engine.textContent = 'engine ready';
    ui.engine.className = 'dim ellipsis ok';
  } catch (err) {
    ui.engine.textContent = 'engine failed';
    ui.engine.className = 'bad ellipsis';
    fallback(`Could not load the compiler: ${err.message}`);
  }
})();

/** Degrade to the CLI instructions when the wasm engine is unusable. */
function fallback(reason) {
  ui.compile.disabled = true;
  say(`${reason}\n\nYou can still compile locally:\n  bs compile source.json out.json`, 'bad');
}

// ------------------------------------------------------------------ compile

async function run() {
  const src = ui.source.value.trim();
  if (!src) { say('Nothing to compile — paste or open a source deck first.', 'bad'); return; }

  ui.compile.disabled = true;
  say('compiling…');
  try {
    const out = await compile(src);
    compiled = out;
    ui.download.disabled = false;
    ui.present.disabled = false;

    const deck = JSON.parse(out);
    const extras = [
      deck.loops?.length && `${deck.loops.length} loop`,
      deck.animations?.length && `${deck.animations.length} animation`,
      deck.auto_advances?.length && `${deck.auto_advances.length} auto-advance`,
      deck.commands?.length && `${deck.commands.length} command`,
    ].filter(Boolean);
    ui.stats.textContent =
      `${deck.frames.length} frames · ${deck.contract.width}×${deck.contract.height}` +
      ` · ${fmtBytes(out.length)}` + (extras.length ? ` · ${extras.join(', ')}` : '');
    say('compiled', 'ok');
  } catch (err) {
    compiled = null;
    ui.download.disabled = true;
    ui.present.disabled = true;
    ui.stats.textContent = '';
    // The compiler's own message, verbatim — the same text `bs compile` prints.
    say(err.message, 'bad');
  } finally {
    ui.compile.disabled = false;
  }
}

const fmtBytes = (n) => (n < 1024 ? `${n} B` : n < 1024 * 1024
  ? `${(n / 1024).toFixed(1)} KB` : `${(n / 1024 / 1024).toFixed(1)} MB`);

ui.compile.addEventListener('click', run);

// Ctrl/Cmd+Enter compiles from inside the textarea.
ui.source.addEventListener('keydown', (e) => {
  if ((e.ctrlKey || e.metaKey) && e.key === 'Enter') { e.preventDefault(); run(); }
});

// ------------------------------------------------------------- hand to present

/**
 * Stash the compiled deck where present.html can pick it up.
 *
 * Tries sessionStorage, then localStorage: which of the two is available varies
 * with browser privacy settings, and on a `file://` page both can throw. The
 * caller must be told when neither worked — a "present →" button that navigates
 * to an empty viewer is the worst outcome.
 */
function stash(text) {
  for (const store of [
    () => sessionStorage,
    () => localStorage,
  ]) {
    try {
      const s = store();
      s.setItem(HANDOFF_KEY, text);
      if (s.getItem(HANDOFF_KEY) === text) return true;   // confirm it stuck
    } catch { /* unavailable or over quota — try the next one */ }
  }
  return false;
}

ui.present.addEventListener('click', () => {
  if (!compiled) return;
  if (stash(compiled)) {
    location.href = 'present.html?deck=session';
  } else {
    say('This browser will not let the page store the deck (private mode, or the '
      + 'page was opened from a file:// path).\nUse [download], then open the file '
      + 'on the present page.', 'bad');
  }
});

ui.download.addEventListener('click', () => {
  if (!compiled) return;
  const url = URL.createObjectURL(new Blob([compiled], { type: 'application/json' }));
  const a = document.createElement('a');
  a.href = url;
  a.download = 'presentation.json';
  document.body.appendChild(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
});

// -------------------------------------------------------------- input sources

ui.clear.addEventListener('click', () => {
  ui.source.value = '';
  ui.stats.textContent = '';
  compiled = null;
  ui.download.disabled = ui.present.disabled = true;
  say('cleared');
  ui.source.focus();
});

ui.sample.addEventListener('click', async () => {
  say('loading sample…');
  try {
    const res = await fetch('demo.json', { cache: 'no-cache' });
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    ui.source.value = await res.text();
    say('sample loaded — press compile');
  } catch (err) {
    say(`Could not load the sample: ${err.message}`, 'bad');
  }
});

ui.open.addEventListener('click', () => ui.file.click());
ui.file.addEventListener('change', () => {
  const f = ui.file.files && ui.file.files[0];
  if (f) readFile(f);
  ui.file.value = '';       // so re-picking the same file fires change again
});

function readFile(file) {
  const r = new FileReader();
  r.onload = () => {
    ui.source.value = String(r.result);
    ui.stats.textContent = '';
    compiled = null;
    ui.download.disabled = ui.present.disabled = true;
    say(`loaded ${file.name} — press compile`);
  };
  r.onerror = () => say(`Could not read ${file.name}`, 'bad');
  r.readAsText(file);
}

let dragDepth = 0;
window.addEventListener('dragenter', (e) => {
  e.preventDefault(); dragDepth++; document.body.classList.add('dragging');
});
window.addEventListener('dragover', (e) => e.preventDefault());
window.addEventListener('dragleave', () => {
  if (--dragDepth <= 0) { dragDepth = 0; document.body.classList.remove('dragging'); }
});
window.addEventListener('drop', (e) => {
  e.preventDefault(); dragDepth = 0; document.body.classList.remove('dragging');
  const f = e.dataTransfer?.files?.[0];
  if (f) readFile(f);
});
