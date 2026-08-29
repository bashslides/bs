// The compile view: source JSON → playable JSON via the real engine, compiled
// to WebAssembly (see wasm.js and wasm/src/lib.rs).
//
// The compiled JSON is never shown. It is not something anyone reads — it is
// input for the present view or a file to keep — so the view reports what came
// out (frames, size, regions) and offers the three things you can do with it.

'use strict';

import { compile, loadCompiler, wasmSupported } from './wasm.js';
import * as store from './store.js';

const el = {};
let compiled = null;      // { json, name }
let onPresent = null;     // handed in by the shell

export function init(opts) {
  const id = (n) => document.getElementById(n);
  Object.assign(el, {
    source: id('source'), status: id('c-status'), stats: id('c-stats'),
    compile: id('c-compile'), present: id('c-present'), download: id('c-download'),
    open: id('c-open'), sample: id('c-sample'), clear: id('c-clear'),
    file: id('c-file'), engine: id('c-engine'),
  });
  onPresent = opts.onPresent;

  el.compile.addEventListener('click', () => run(nameFor()));
  el.present.addEventListener('click', () => compiled && onPresent(compiled.json, compiled.name));
  el.download.addEventListener('click', download);
  el.open.addEventListener('click', () => el.file.click());
  el.clear.addEventListener('click', clear);
  el.sample.addEventListener('click', sample);

  el.file.addEventListener('change', () => {
    const f = el.file.files && el.file.files[0];
    if (f) openFile(f);
    el.file.value = '';     // so re-picking the same file fires change again
  });

  // Ctrl/Cmd+Enter compiles from inside the textarea.
  el.source.addEventListener('keydown', (e) => {
    if ((e.ctrlKey || e.metaKey) && e.key === 'Enter') { e.preventDefault(); run(nameFor()); }
  });

  bootEngine();
  return { openFile };
}

// --------------------------------------------------------------- engine boot

async function bootEngine() {
  if (!wasmSupported()) {
    setEngine('no WebAssembly', 'bad');
    disable('This browser cannot run WebAssembly, so compiling here is not possible.');
    return;
  }
  try {
    await loadCompiler();
    setEngine('engine ready', 'ok');
  } catch (err) {
    setEngine('engine failed', 'bad');
    disable(`Could not load the compiler: ${err.message}`);
  }
}

const setEngine = (text, cls) => {
  el.engine.textContent = text;
  el.engine.className = `ellipsis ${cls === 'ok' ? 'dim ok' : 'bad'}`;
};

function disable(reason) {
  el.compile.disabled = true;
  el.open.disabled = true;
  say(`${reason}\n\nYou can still compile locally:\n  bs compile source.json out.json`, 'bad');
}

function say(text, cls = 'dim') {
  el.status.textContent = text;
  el.status.className = cls;
}

// ------------------------------------------------------------------ compiling

let lastName = 'deck';
const nameFor = () => lastName;

/**
 * Compile whatever is in the textarea.
 *
 * On success the source is *not* kept on screen — there is nothing to read
 * there. On failure it stays (or is filled in, for an opened file) so the
 * problem can be found and fixed.
 */
async function run(name, sourceText = el.source.value) {
  const src = sourceText.trim();
  if (!src) { say('Nothing to compile — paste, or open a source deck.', 'bad'); return false; }

  el.compile.disabled = true;
  say('compiling…');
  try {
    const out = await compile(src);
    compiled = { json: out, name };
    lastName = name;
    el.present.disabled = false;
    el.download.disabled = false;

    const deck = JSON.parse(out);
    const extras = [
      deck.loops?.length && `${deck.loops.length} loop`,
      deck.animations?.length && `${deck.animations.length} animation`,
      deck.auto_advances?.length && `${deck.auto_advances.length} auto-advance`,
      deck.commands?.length && `${deck.commands.length} command`,
    ].filter(Boolean);
    el.stats.textContent = `${deck.frames.length} frames · ${deck.contract.width}×${deck.contract.height}`
      + ` · ${store.fmtBytes(out.length)}` + (extras.length ? ` · ${extras.join(', ')}` : '');

    const saved = store.save(name, out, 'compiled');
    say(saved ? `compiled — saved to your library as “${name}”`
              : 'compiled (could not save to the library: storage is unavailable)',
        saved ? 'ok' : 'dim');
    return true;
  } catch (err) {
    compiled = null;
    el.present.disabled = true;
    el.download.disabled = true;
    el.stats.textContent = '';
    // The compiler's own message, verbatim — the same text `bs compile` prints.
    say(err.message, 'bad');
    return false;
  } finally {
    el.compile.disabled = false;
  }
}

/** Opening a file compiles it straight away; only a failure needs the editor. */
async function openFile(file) {
  const text = await file.text().catch(() => null);
  if (text === null) { say(`Could not read ${file.name}`, 'bad'); return; }

  const name = file.name.replace(/\.json$/i, '');
  say(`compiling ${file.name}…`);
  const ok = await run(name, text);
  if (ok) {
    el.source.value = '';           // nothing worth showing
  } else {
    el.source.value = text;         // show it so the error can be fixed
    el.source.focus();
  }
}

function clear() {
  el.source.value = '';
  el.stats.textContent = '';
  compiled = null;
  el.present.disabled = el.download.disabled = true;
  say('cleared');
  el.source.focus();
}

async function sample() {
  say('loading sample…');
  try {
    const res = await fetch('demo.json', { cache: 'no-cache' });
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    el.source.value = await res.text();
    lastName = 'demo';
    say('sample loaded — press compile');
  } catch (err) {
    say(`Could not load the sample: ${err.message}`, 'bad');
  }
}

function download() {
  if (!compiled) return;
  const url = URL.createObjectURL(new Blob([compiled.json], { type: 'application/json' }));
  const a = document.createElement('a');
  a.href = url;
  a.download = `${compiled.name}.json`;
  document.body.appendChild(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}
