// The deck library: compiled presentations kept in IndexedDB so they survive
// a reload, an app restart, and being offline. A deck compiled here also keeps
// the source it came from, so it can be exported and edited elsewhere.
//
// A compiled deck is mostly the same cell records repeated frame after frame,
// so each one is stored gzip-compressed (CompressionStream; the demo deck goes
// from 125 KB to under 2 KB) and only inflated when it is opened. Metadata and
// deck bodies live in separate object stores, so listing the library never
// reads a body.
//
// IndexedDB's quota is a share of free disk space rather than localStorage's
// ~5 MB, but it is still per *origin*, and on GitHub Pages every project site
// of a user shares one origin. So the database is named after this
// deployment's path (two bs copies on one origin keep separate libraries), and
// every write keeps the stored (compressed) bodies under BUDGET bytes in total,
// evicting oldest first.
//
// Every function is async and never throws: storage that is unavailable
// (private mode, file://, a browser without IndexedDB) reads as an empty
// library and a save that cannot be kept resolves to null.

'use strict';

const SCOPE = (() => {
  try { return new URL('.', location.href).pathname; } catch { return '/'; }
})();
const DB_NAME = `bs:decks:${SCOPE}`;
/** Where builds before IndexedDB kept the library: per deployment, and before
 *  that one unscoped key. Adopted once, when this deployment's database is
 *  created, then freed. */
const LOCAL_KEY = `bs:decks:${SCOPE}`;
const LEGACY_KEY = 'bs:decks';
const MAX_DECKS = 100;
/** Most stored (compressed) bytes kept, summed over the decks. */
const BUDGET = 50 * 1024 * 1024;

const META = 'meta';   // id → { id, name, savedAt, frames, width, height, bytes, sourceBytes, stored, origin }
const DATA = 'data';   // id → { enc: 'gzip', body: ArrayBuffer } | { enc: 'none', body: string },
                       //      plus `src` (same shape) when the deck's source is kept

/** Fired after any change so open views can refresh. */
const listeners = new Set();
export const onChange = (fn) => { listeners.add(fn); return () => listeners.delete(fn); };
const emit = () => listeners.forEach((fn) => { try { fn(); } catch { /* keep going */ } });

// ------------------------------------------------------------ compression

const canCompress = typeof CompressionStream === 'function' && typeof DecompressionStream === 'function';

/** The body to store for `json`: gzip where the browser can, else the text. */
async function pack(json) {
  if (canCompress) {
    try {
      const stream = new Blob([json]).stream().pipeThrough(new CompressionStream('gzip'));
      const body = await new Response(stream).arrayBuffer();
      return { enc: 'gzip', body, size: body.byteLength };
    } catch { /* fall back to the plain text */ }
  }
  return { enc: 'none', body: json, size: json.length };
}

/** The deck text back out of a stored body. */
async function unpack(rec) {
  if (rec.enc === 'none') return rec.body;
  const stream = new Blob([rec.body]).stream().pipeThrough(new DecompressionStream('gzip'));
  return new Response(stream).text();
}

// --------------------------------------------------------------- database

/** Newest first. */
const newestFirst = (a, b) => b.savedAt - a.savedAt;

/** The ids to keep from `metas` (newest first): at most MAX_DECKS, oldest
 *  dropped until the stored bytes fit BUDGET. */
function keep(metas) {
  let kept = metas.slice(0, MAX_DECKS);
  let used = kept.reduce((n, m) => n + (m.stored ?? 0), 0);
  while (kept.length > 0 && used > BUDGET) used -= kept.pop().stored ?? 0;
  return kept;
}

/** A library saved to localStorage by an earlier build, or []. */
function localLibrary() {
  try {
    // A deployment's own key wins; the unscoped one is adopted only when this
    // deployment never had a library (an older bs copy may still use it).
    const own = localStorage.getItem(LOCAL_KEY);
    const text = own ?? localStorage.getItem(LEGACY_KEY);
    const decks = text ? JSON.parse(text) : [];
    return Array.isArray(decks) ? decks.filter((d) => d && typeof d.json === 'string' && d.id) : [];
  } catch {
    return [];
  }
}

function freeLocalLibrary() {
  try {
    if (localStorage.getItem(LOCAL_KEY) !== null) localStorage.removeItem(LOCAL_KEY);
    else localStorage.removeItem(LEGACY_KEY);
  } catch { /* nothing to free */ }
}

let opening = null;

/** The open database (created, and the old localStorage library adopted, on
 *  first use). Rejects when the browser will not store anything. */
function db() {
  if (opening) return opening;
  opening = new Promise((resolve, reject) => {
    let req;
    try { req = indexedDB.open(DB_NAME, 1); } catch (err) { reject(err); return; }
    let created = false;
    req.onupgradeneeded = () => {
      const d = req.result;
      const meta = d.createObjectStore(META, { keyPath: 'id' });
      const data = d.createObjectStore(DATA);
      // Runs exactly once, when the database is created, inside the same
      // transaction: the old decks are adopted as plain text (compressing is
      // async and would end the transaction), and their key is freed only
      // once that transaction has committed.
      const old = localLibrary().map(({ json, ...m }) => ({ m: { ...m, stored: json.length }, json }));
      const kept = new Set(keep(old.map((o) => o.m).sort(newestFirst)).map((m) => m.id));
      for (const { m, json } of old) {
        if (!kept.has(m.id)) continue;
        meta.put(m);
        data.put({ enc: 'none', body: json }, m.id);
      }
      created = true;
    };
    req.onsuccess = () => {
      if (created) freeLocalLibrary();
      const d = req.result;
      // Another tab upgrading the schema later must not be blocked by this one.
      d.onversionchange = () => { d.close(); opening = null; };
      resolve(d);
    };
    req.onerror = () => reject(req.error);
  });
  // A failed open is not cached: the next call tries again.
  const attempt = opening;
  attempt.catch(() => { if (opening === attempt) opening = null; });
  return attempt;
}

/** Run `body(stores)` in one transaction; resolves to its result once committed. */
async function tx(mode, body) {
  const d = await db();
  return new Promise((resolve, reject) => {
    const t = d.transaction([META, DATA], mode);
    let result;
    t.oncomplete = () => resolve(result);
    // A failed request (e.g. a put over quota) fires `error` before the
    // transaction aborts, while `t.error` is still null; remember the
    // request's own error and reject once the abort has happened.
    let failure = null;
    t.onerror = (e) => { failure ??= e.target?.error ?? null; };
    t.onabort = () => reject(t.error || failure || new Error('transaction aborted'));
    try {
      result = body({ meta: t.objectStore(META), data: t.objectStore(DATA) }, (v) => { result = v; });
    } catch (err) {
      try { t.abort(); } catch { /* already finished */ }
      reject(err);
    }
  });
}

/** An IDB request's result, set into the transaction's result via `set`. */
const into = (request, set, map = (v) => v) => { request.onsuccess = () => set(map(request.result)); };

// -------------------------------------------------------------------- API

/** Metadata of every saved deck, newest first — enough for the library list. */
export async function summaries() {
  try {
    return await tx('readonly', ({ meta }, set) => into(meta.getAll(), set, (all) => all.sort(newestFirst)));
  } catch {
    return [];
  }
}

/** A saved deck's metadata and stored body, or null. */
function read(id) {
  return tx('readonly', ({ meta, data }, set) => {
    const m = meta.get(id);
    const b = data.get(id);
    b.onsuccess = () => set(m.result && b.result ? { m: m.result, b: b.result } : null);
  });
}

/** A saved deck with its text (`json`), or null. */
export async function get(id) {
  try {
    const found = await read(id);
    if (!found) return null;
    return { ...found.m, json: await unpack(found.b) };
  } catch {
    return null;
  }
}

/**
 * What exporting a saved deck writes: its source when the library kept it
 * (`editable`, for `bs edit` or the compile view), else the compiled deck
 * (only `bs play` and the present view can use it). Null if it is gone.
 */
export async function exported(id) {
  try {
    const found = await read(id);
    if (!found) return null;
    const editable = !!found.b.src;
    const text = await unpack(editable ? found.b.src : found.b);
    return { name: found.m.name, text, editable };
  } catch {
    return null;
  }
}

/**
 * Save a compiled deck, newest first, de-duplicated by name.
 *
 * `json` is the compiled text exactly as the compiler produced it; `source`,
 * when given, the source text it was compiled from, kept for export. Resolves to
 * the stored record's metadata, or null if storage is unavailable even after
 * evicting, or the deck alone is over the library's budget.
 */
export async function save(name, json, origin = 'compiled', source = null) {
  let deck;
  try {
    const parsed = JSON.parse(json);
    deck = {
      id: `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 7)}`,
      name,
      savedAt: Date.now(),
      frames: parsed.frames?.length ?? 0,
      width: parsed.contract?.width ?? 0,
      height: parsed.contract?.height ?? 0,
      bytes: json.length,
      sourceBytes: source == null ? 0 : source.length,
      origin,
    };
  } catch {
    return null;
  }

  // Compress before the transaction: awaiting anything but IndexedDB inside
  // one would let it commit early.
  const packed = await pack(json);
  const src = source == null ? null : await pack(source);
  deck.stored = packed.size + (src?.size ?? 0);
  // A deck over the budget on its own is not kept (it still plays).
  if (deck.stored > BUDGET) return null;

  // Out of quota: keep fewer of the older decks and try again, down to just
  // this one. `limit` caps how many older decks stay.
  for (let limit = MAX_DECKS - 1; limit >= 0; ) {
    try {
      await tx('readwrite', ({ meta, data }) => {
        meta.getAll().onsuccess = (e) => {
          // Same name replaces the older entry rather than piling up duplicates.
          const others = e.target.result.filter((m) => m.name !== name).sort(newestFirst);
          const kept = keep([deck, ...others.slice(0, limit)]);
          const ids = new Set(kept.map((m) => m.id));
          for (const m of e.target.result) {
            if (ids.has(m.id)) continue;
            meta.delete(m.id);
            data.delete(m.id);
          }
          meta.put(deck);
          data.put({ enc: packed.enc, body: packed.body,
            ...(src && { src: { enc: src.enc, body: src.body } }) }, deck.id);
        };
      });
      emit();
      return deck;
    } catch (err) {
      if (err?.name !== 'QuotaExceededError' || limit === 0) return null;
      // Halve what is kept, so a nearly full origin needs a few tries, not 100.
      const have = (await summaries()).filter((m) => m.name !== name).length;
      limit = Math.min(limit, have) >> 1;
    }
  }
  return null;
}

export async function remove(id) {
  try {
    await tx('readwrite', ({ meta, data }) => { meta.delete(id); data.delete(id); });
    emit();
    return true;
  } catch {
    return false;
  }
}

export async function clear() {
  try {
    await tx('readwrite', ({ meta, data }) => { meta.clear(); data.clear(); });
    emit();
    return true;
  } catch {
    return false;
  }
}

/** True when the browser lets this page store anything at all. */
export async function available() {
  try {
    await db();
    return true;
  } catch {
    return false;
  }
}

/** "3.2 KB" / "1.4 MB" */
export const fmtBytes = (n) => (n < 1024 ? `${n} B`
  : n < 1024 * 1024 ? `${(n / 1024).toFixed(1)} KB`
  : `${(n / 1024 / 1024).toFixed(1)} MB`);

/** "just now" / "12 min ago" / "3 days ago" */
export function fmtAge(ts) {
  const s = Math.max(0, (Date.now() - ts) / 1000);
  if (s < 60) return 'just now';
  if (s < 3600) return `${Math.floor(s / 60)} min ago`;
  if (s < 86400) return `${Math.floor(s / 3600)} h ago`;
  const d = Math.floor(s / 86400);
  return d === 1 ? 'yesterday' : `${d} days ago`;
}
