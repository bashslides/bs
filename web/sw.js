// Service worker: makes the app installable and fully usable offline.
//
// Everything here is static, so the strategy is simply cache-first with a
// versioned cache. scripts/build-web.sh stamps BUILD with a hash of the built
// files, so every deploy installs a fresh cache and drops the old one — no
// manual version bumping, and no stale app after an update.

const BUILD = '__BUILD__';
const CACHE = `bs-${BUILD}`;

// Relative URLs so the worker works under a project subpath (…/bs/) as well as
// at a domain root.
const ASSETS = [
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
  'demo.json',
  'presentation.json',
  'presentation-format.md',
  'bs-deck-skill.md',
  'manifest.webmanifest',
  'icon-192.png',
  'icon-512.png',
  'icon-maskable-512.png',
];

self.addEventListener('install', (e) => {
  e.waitUntil((async () => {
    const cache = await caches.open(CACHE);
    // Individually, so one missing optional asset cannot fail the install.
    await Promise.all(ASSETS.map((u) => cache.add(u).catch(() => {})));
    self.skipWaiting();
  })());
});

self.addEventListener('activate', (e) => {
  e.waitUntil((async () => {
    const names = await caches.keys();
    await Promise.all(names.filter((n) => n !== CACHE).map((n) => caches.delete(n)));
    await self.clients.claim();
  })());
});

self.addEventListener('fetch', (e) => {
  const req = e.request;
  if (req.method !== 'GET') return;

  const url = new URL(req.url);
  if (url.origin !== location.origin) return;   // never touch third-party requests

  e.respondWith((async () => {
    const cached = await caches.match(req, { ignoreSearch: true });
    if (cached) return cached;
    try {
      const res = await fetch(req);
      // Cache same-origin successes so decks opened by URL work offline later.
      if (res.ok && res.type === 'basic') {
        const cache = await caches.open(CACHE);
        cache.put(req, res.clone());
      }
      return res;
    } catch (err) {
      // Offline and not cached: for a navigation, fall back to the shell.
      if (req.mode === 'navigate') {
        const shell = await caches.match('index.html', { ignoreSearch: true });
        if (shell) return shell;
      }
      throw err;
    }
  })());
});
