// The compile tool: source JSON → playable JSON, run through the real engine
// compiled to WebAssembly (see wasm.js and wasm/src/lib.rs).

'use strict';

import { compile, loadCompiler, wasmSupported } from './wasm.js';

const el = (id) => document.getElementById(id);
const ui = {
  source: el('source'), output: el('output'), status: el('status'), stats: el('stats'),
  compile: el('compile'), download: el('download'), present: el('present'),
  sample: el('sample'), open: el('open'), clear: el('clear'),
  file: el('file'), engine: el('engine'),
};

/** Where present.html looks for a deck handed over from this page. */
const HANDOFF_KEY = 'bs:deck';

let compiled = null;

// --------------------------------------------------------------- engine boot

// Report the engine's state up front: a compile button that silently does
// nothing is worse than one that says why.
(async () => {
  if (!wasmSupported()) {
    ui.engine.textContent = 'WebAssembly unavailable';
    ui.engine.className = 'bad';
    fallback('This browser cannot run WebAssembly, so compiling here is not possible.');
    return;
  }
  try {
    await loadCompiler();
    ui.engine.textContent = 'engine ready';
    ui.engine.className = 'dim ok';
  } catch (err) {
    ui.engine.textContent = 'engine failed to load';
    ui.engine.className = 'bad';
    fallback(`Could not load the compiler: ${err.message}`);
  }
})();

/** Degrade to the CLI instructions when the wasm engine is unusable. */
function fallback(reason) {
  ui.compile.disabled = true;
  say(`${reason}\n\nYou can still compile locally:\n  bs compile source.json out.json`, 'bad');
}

// ------------------------------------------------------------------- actions

function say(text, cls = 'dim') {
  ui.status.textContent = text;
  ui.status.className = cls;
}

async function run() {
  const src = ui.source.value.trim();
  if (!src) { say('Nothing to compile — paste a source deck first.', 'bad'); return; }

  ui.compile.disabled = true;
  say('compiling…');
  try {
    const out = await compile(src);
    compiled = out;
    ui.output.textContent = out;
    ui.download.disabled = false;
    ui.present.disabled = false;

    const deck = JSON.parse(out);
    const { width, height } = deck.contract;
    const extras = [
      deck.loops?.length && `${deck.loops.length} loop`,
      deck.animations?.length && `${deck.animations.length} animation`,
      deck.auto_advances?.length && `${deck.auto_advances.length} auto-advance`,
      deck.commands?.length && `${deck.commands.length} command`,
    ].filter(Boolean);
    ui.stats.textContent =
      `${deck.frames.length} frames · ${width}×${height} · ${fmtBytes(out.length)}` +
      (extras.length ? ` · ${extras.join(', ')}` : '');
    say('compiled', 'ok');
  } catch (err) {
    compiled = null;
    ui.output.textContent = '';
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

ui.download.addEventListener('click', () => {
  if (!compiled) return;
  const url = URL.createObjectURL(new Blob([compiled], { type: 'application/json' }));
  const a = document.createElement('a');
  a.href = url;
  a.download = 'presentation.json';
  a.click();
  URL.revokeObjectURL(url);
});

ui.present.addEventListener('click', () => {
  if (!compiled) return;
  try {
    sessionStorage.setItem(HANDOFF_KEY, compiled);
    location.href = 'present.html?deck=session';
  } catch {
    // Private-mode storage limits: fall back to a download.
    say('Could not hand the deck over (storage blocked) — use [download] instead.', 'bad');
  }
});

ui.clear.addEventListener('click', () => {
  ui.source.value = '';
  ui.output.textContent = '';
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
  ui.file.value = '';
});

function readFile(file) {
  const r = new FileReader();
  r.onload = () => { ui.source.value = String(r.result); say(`loaded ${file.name}`); };
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
