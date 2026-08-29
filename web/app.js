// The shell: one page, three views (instructions → compile → present), plus the
// deck library and the service-worker registration that makes it installable.
//
// Views are sections in a single document rather than separate pages, so the
// compiled WebAssembly engine, the loaded deck and the library are all shared
// state — compiling and then presenting never reloads anything.

'use strict';

import * as viewer from './viewer.js';
import * as compileView from './compile.js';
import * as instructionsView from './instructions.js';
import * as store from './store.js';

// `home` is the title page; the other three are the tools, in working order.
const VIEWS = ['home', 'instructions', 'compile', 'present'];
let current = null;
let showInstructions = null;
let compileApi = null;

const id = (n) => document.getElementById(n);

// ---------------------------------------------------------------- view routing

function show(view, { replace = false } = {}) {
  if (!VIEWS.includes(view)) view = 'home';
  if (view === current) return;

  if (current === 'present') viewer.suspend();
  current = view;

  for (const v of VIEWS) {
    id(`view-${v}`).hidden = v !== view;
    const tab = id(`tab-${v}`);
    tab.classList.toggle('active', v === view);
    if (tab.getAttribute('role') === 'tab') tab.setAttribute('aria-selected', String(v === view));
  }

  // library/open act on the present view, so they travel with it.
  id('bar-actions').hidden = view !== 'present';

  const hash = `#${view}`;
  if (location.hash !== hash) {
    if (replace) history.replaceState(null, '', hash);
    else history.pushState(null, '', hash);
  }

  if (view === 'instructions') showInstructions?.();
  if (view === 'present') viewer.resume();
}

// ------------------------------------------------------------------- library

function renderLibrary() {
  const wrap = id('library-list');
  const decks = store.summaries();
  id('library-count').textContent = decks.length ? `${decks.length} saved` : '';
  id('library-clear').disabled = decks.length === 0;

  if (!decks.length) {
    wrap.innerHTML = store.available()
      ? '<p class="dim empty">Nothing saved yet. Compiling a deck, or opening one here, adds it.</p>'
      : '<p class="bad empty">This browser will not let the page store decks '
        + '(private mode, or opened from a file:// path).</p>';
    return;
  }

  wrap.innerHTML = '';
  for (const d of decks) {
    const row = document.createElement('div');
    row.className = 'deck';

    const open = document.createElement('button');
    open.className = 'deck-open';
    open.type = 'button';
    open.innerHTML = `<b></b><span class="dim"></span>`;
    open.querySelector('b').textContent = d.name;
    open.querySelector('span').textContent =
      `${d.frames} frames · ${d.width}×${d.height} · ${store.fmtBytes(d.bytes)} · ${store.fmtAge(d.savedAt)}`;
    open.addEventListener('click', () => openFromLibrary(d.id));

    const del = document.createElement('button');
    del.className = 'deck-del';
    del.type = 'button';
    del.textContent = '✕';
    del.title = `delete “${d.name}”`;
    del.setAttribute('aria-label', `delete ${d.name}`);
    del.addEventListener('click', () => {
      store.remove(d.id);
      renderLibrary();
    });

    row.append(open, del);
    wrap.appendChild(row);
  }
}

function openFromLibrary(deckId) {
  const rec = store.get(deckId);
  if (!rec) { renderLibrary(); return; }
  try {
    viewer.load(JSON.parse(rec.json), rec.name);
    setLibraryOpen(false);
  } catch (err) {
    viewer.message([`  could not open “${viewer.esc(rec.name)}”`, '', `  ${viewer.esc(err.message)}`]);
  }
}

function setLibraryOpen(open) {
  id('library').hidden = !open;
  id('btn-library').classList.toggle('active', open);
  if (open) renderLibrary();
}

// ------------------------------------------------------------- opening a deck

/** Load a compiled deck from text, remember it, and switch to present. */
function present(json, name) {
  try {
    viewer.load(JSON.parse(json), name);
    store.save(name, json, 'compiled');
    show('present');
    setLibraryOpen(false);
  } catch (err) {
    show('present');
    viewer.message(['  that deck could not be opened', '', `  ${viewer.esc(err.message)}`]);
  }
}

/** A compiled deck picked from disk in the present view. */
async function importDeck(file) {
  const text = await file.text().catch(() => null);
  if (text === null) { viewer.message([`  could not read ${viewer.esc(file.name)}`]); return; }
  const name = file.name.replace(/\.json$/i, '');
  try {
    const data = JSON.parse(text);
    viewer.load(data, name);
    store.save(name, text, 'imported');
    setLibraryOpen(false);
  } catch (err) {
    viewer.message([
      `  ${viewer.esc(file.name)} is not a compiled presentation`,
      '',
      `  ${viewer.esc(err.message)}`,
      '',
      '  Source decks go through <b>compile</b> first.',
    ]);
  }
}

// ------------------------------------------------------------------- start-up

function emptyState() {
  viewer.message([
    '  no deck loaded',
    '',
    '  <b>library</b> — open one you saved',
    '  <b>open</b> — a compiled .json from this device',
    '  <b>compile</b> — turn a source deck into one',
  ]);
}

function boot() {
  viewer.init({ onOpenRequest: () => setLibraryOpen(true) });
  compileApi = compileView.init({ onPresent: present });
  showInstructions = instructionsView.init();

  for (const v of VIEWS) id(`tab-${v}`).addEventListener('click', () => show(v));
  // In-paragraph links on the title page jump to the view they name.
  for (const b of document.querySelectorAll('#home [data-go]')) {
    b.addEventListener('click', () => show(b.dataset.go));
  }
  window.addEventListener('popstate', () => show(location.hash.slice(1) || 'home', { replace: true }));

  id('btn-library').addEventListener('click', () => setLibraryOpen(id('library').hidden));
  id('library-close').addEventListener('click', () => setLibraryOpen(false));
  id('library-clear').addEventListener('click', () => {
    if (!confirm('Delete every saved deck? This cannot be undone.')) return;
    store.clear();
    renderLibrary();
  });
  id('btn-open').addEventListener('click', () => id('p-file').click());
  id('p-file').addEventListener('change', (e) => {
    const f = e.target.files && e.target.files[0];
    if (f) importDeck(f);
    e.target.value = '';
  });
  store.onChange(() => { if (!id('library').hidden) renderLibrary(); });

  // Keys reach whichever view is showing; text fields keep their own.
  document.addEventListener('keydown', (e) => {
    const tag = e.target.tagName;
    if (tag === 'TEXTAREA' || tag === 'INPUT') return;
    if (current === 'present') {
      if (e.key === 'o' || e.key === 'O') { id('p-file').click(); e.preventDefault(); return; }
      viewer.handleKey(e);
    }
  });

  // Dropping a file anywhere: a compiled deck plays, a source deck compiles.
  let depth = 0;
  window.addEventListener('dragenter', (e) => { e.preventDefault(); depth++; document.body.classList.add('dragging'); });
  window.addEventListener('dragover', (e) => e.preventDefault());
  window.addEventListener('dragleave', () => { if (--depth <= 0) { depth = 0; document.body.classList.remove('dragging'); } });
  window.addEventListener('drop', async (e) => {
    e.preventDefault(); depth = 0; document.body.classList.remove('dragging');
    const f = e.dataTransfer?.files?.[0];
    if (!f) return;
    const text = await f.text().catch(() => '');
    // A playable deck has a `contract`; a source deck has `frame_count`.
    let looksCompiled = false;
    try { looksCompiled = !!JSON.parse(text)?.contract; } catch { /* let the view report it */ }
    if (looksCompiled) { show('present'); importDeck(f); }
    else { show('compile'); compileApi.openFile(f); }
  });

  emptyState();
  const start = location.hash.slice(1);
  current = null;
  show(VIEWS.includes(start) ? start : 'home', { replace: true });
  renderLibrary();
}

boot();

// ------------------------------------------------------------ installable app

// Registered last so a failure here can never stop the app from working.
if ('serviceWorker' in navigator) {
  window.addEventListener('load', () => {
    // A relative path scopes the worker to this directory, which is what makes
    // it work under a GitHub Pages project subpath (…/bs/) as well as at a root.
    navigator.serviceWorker.register('sw.js').catch(() => { /* offline is a bonus */ });
  });
}
