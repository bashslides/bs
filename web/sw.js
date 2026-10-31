// Service worker: makes the app installable and fully usable offline.
//
// Everything here is static, so the strategy is simply cache-first with a
// versioned cache. scripts/build-web.sh stamps BUILD with a hash of the built
// files, so every deploy installs a fresh cache and drops its own old one — no
// manual version bumping, and once the new build is active no stale file.
//
// A new build never takes over a running page: no skipWaiting(), so it waits
// until every tab of the old build is closed. A page keeps fetching files
// lazily (the sample deck, the format reference, bs.wasm after a trap), and
// they must come from the build whose engine and glue it already loaded.

const BUILD = '__BUILD__';
// Cache Storage is shared by the whole origin (e.g. every project site on
// <user>.github.io), so the name carries this deployment's scope and only
// caches with this exact prefix are ever deleted. Spaces separate the parts: a
// serialised URL never contains one, so no other scope's names share it.
const PREFIX = `bs ${self.registration.scope} `;
const CACHE = `${PREFIX}${BUILD}`;
// The name builds before this scheme used (`bs-` + 12-hex build hash).
const LEGACY = /^bs-[0-9a-f]{12}$/;

// Relative URLs so the worker works under a project subpath (…/bs/) as well as
// at a domain root. The app cannot start or compile without REQUIRED, so any
// of them failing fails the install; OPTIONAL ones may be missing.
const REQUIRED = [
  '.',
  'index.html',
  'style.css',
  'app.js',
  'viewer.js',
  'compile.js',
  'instructions.js',
  'store.js',
  'wasm.js',
  'bs.wasm',
  'manifest.webmanifest',
];
const OPTIONAL = [
  'demo.json',
  'presentation-format.md',
  'bs-deck-skill.md',
  'icon-192.png',
  'icon-512.png',
  'icon-maskable-512.png',
];

self.addEventListener('install', (e) => {
  e.waitUntil((async () => {
    // `reload` bypasses the HTTP cache, so this build's cache never holds a
    // previous build's files.
    const get = (u) => new Request(u, { cache: 'reload' });
    const fresh = !(await caches.has(CACHE));
    const cache = await caches.open(CACHE);
    try {
      await cache.addAll(REQUIRED.map(get));
    } catch (err) {
      // A failed install keeps the previous worker and its complete cache;
      // drop the partial one (unless it predates this install).
      if (fresh) await caches.delete(CACHE);
      throw err;
    }
    await Promise.all(OPTIONAL.map((u) => cache.add(get(u)).catch(() => {})));
  })());
});

self.addEventListener('activate', (e) => {
  e.waitUntil((async () => {
    const names = await caches.keys();
    // Only this deployment's previous builds; other apps' caches are not ours.
    // A legacy-named cache is ours only if it holds this scope's engine — on a
    // shared origin it may be another bs deployment's live cache.
    const wasm = new URL('bs.wasm', self.registration.scope).href;
    const stale = [];
    for (const n of names) {
      const ours = n.startsWith(PREFIX)
        ? n !== CACHE
        : LEGACY.test(n) && !!(await (await caches.open(n)).match(wasm));
      if (ours) stale.push(n);
    }
    await Promise.all(stale.map((n) => caches.delete(n)));
    // Without skipWaiting this runs only once no page uses the old worker, so it
    // claims just pages with no worker (a first visit, or a Shift+reload, which
    // fetched its files from the network and so gets the newest ones anyway).
    await self.clients.claim();
  })());
});

self.addEventListener('fetch', (e) => {
  const req = e.request;
  if (req.method !== 'GET') return;

  const url = new URL(req.url);
  if (url.origin !== location.origin) return;   // never touch third-party requests

  e.respondWith((async () => {
    // Read only this deployment's cache: other apps' caches on the origin persist.
    const cache = await caches.open(CACHE);
    const cached = await cache.match(req, { ignoreSearch: true });
    if (cached) return cached;
    try {
      const res = await fetch(req);
      // Cache same-origin successes so decks opened by URL work offline later.
      if (res.ok && res.type === 'basic') {
        cache.put(req, res.clone());
      }
      return res;
    } catch (err) {
      // Offline and not cached: for a navigation, fall back to the shell.
      if (req.mode === 'navigate') {
        const shell = await cache.match('index.html', { ignoreSearch: true });
        if (shell) return shell;
      }
      throw err;
    }
  })());
});
