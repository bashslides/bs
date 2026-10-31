// The deck library: compiled presentations kept in localStorage so they survive
// a reload, an app restart, and being offline.
//
// Storage is small (~5 MB) and a compiled deck is often >100 KB, so this is a
// *recents* list, not an archive: MAX_DECKS newest are kept and a write that
// runs out of room evicts the oldest and retries rather than failing.
//
// localStorage is per *origin*, and on GitHub Pages every project site of a
// user shares one origin and its quota. So the key is scoped to this
// deployment's path (two bs copies on one origin keep separate libraries), and
// every write keeps the decks' compiled JSON under BUDGET characters in total,
// evicting oldest first, so the library never takes the whole quota from
// sibling sites. (Stored, each deck's JSON is an escaped string, typically ~1.4x longer.)

'use strict';

const SCOPE = (() => {
  try { return new URL('.', location.href).pathname; } catch { return '/'; }
})();
const KEY = `bs:decks:${SCOPE}`;
/** The unscoped key used before; adopted (and freed) by a deployment's empty library. */
const LEGACY_KEY = 'bs:decks';
const MAX_DECKS = 20;
/**
 * Most characters of compiled deck JSON kept, summed over the decks (the size
 * the library lists). Stored escaped that is typically ~2.1M characters, well
 * under the ~5 MB quota an origin's sites share.
 */
const BUDGET = 1_500_000;

/** Total compiled JSON of `decks`, the quantity BUDGET bounds. */
const used = (decks) => decks.reduce((n, d) => n + (d.json?.length ?? 0), 0);

/** Newest first, at most MAX_DECKS, oldest dropped until within BUDGET. */
function trim(decks) {
  let kept = decks.slice(0, MAX_DECKS);
  while (kept.length > 0 && used(kept) > BUDGET) kept = kept.slice(0, -1);
  return kept;
}

/** Fired after any change so open views can refresh. */
const listeners = new Set();
export const onChange = (fn) => { listeners.add(fn); return () => listeners.delete(fn); };
const emit = () => listeners.forEach((fn) => { try { fn(); } catch { /* keep going */ } });

/** localStorage can throw outright (private mode, file://) — never let it break a view. */
function raw() {
  try {
    return localStorage.getItem(KEY);
  } catch {
    return null;
  }
}

/** All saved decks, newest first. Never throws; a corrupt store reads as empty. */
export function list() {
  const text = raw();
  if (!text) return [];
  try {
    const decks = JSON.parse(text);
    return Array.isArray(decks) ? decks : [];
  } catch {
    return [];
  }
}

function write(decks) {
  try {
    localStorage.setItem(KEY, JSON.stringify(decks));
    return true;
  } catch {
    return false;
  }
}

// Migration: a deployment with no library yet adopts one saved under the old
// unscoped key, trimmed to the budget. The old key is freed first — that old
// library may fill the whole origin quota, so the copy could not be written
// beside it — and the copy then drops its oldest decks until it fits. Once
// this deployment has its own key the old one is never touched again (an
// older bs copy on the origin may still be using it).
try {
  const legacy = localStorage.getItem(LEGACY_KEY);
  if (legacy !== null && localStorage.getItem(KEY) === null) {
    let decks = [];
    try { decks = JSON.parse(legacy); } catch { /* corrupt: start empty */ }
    localStorage.removeItem(LEGACY_KEY);
    decks = Array.isArray(decks) ? trim(decks) : [];
    while (!write(decks) && decks.length > 0) decks = decks.slice(0, -1);
  }
} catch { /* storage unavailable: list() reads as empty */ }

/** Metadata only — enough for the library list, without the deck body. */
export function summaries() {
  return list().map(({ id, name, savedAt, frames, width, height, bytes, origin }) =>
    ({ id, name, savedAt, frames, width, height, bytes, origin }));
}

export function get(id) {
  return list().find((d) => d.id === id) || null;
}

/**
 * Save a compiled deck, newest first, de-duplicated by name.
 *
 * `json` is the compiled text exactly as the compiler produced it. Returns the
 * stored record, or null if storage is unavailable even after evicting, or the
 * deck's compiled JSON alone is over the library's budget.
 */
export function save(name, json, origin = 'compiled') {
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
      origin,
      json,
    };
  } catch {
    return null;
  }

  // A deck over the budget on its own is not kept (it still plays).
  if (json.length > BUDGET) return null;
  // Same name replaces the older entry rather than piling up duplicates.
  const decks = trim([deck, ...list().filter((d) => d.name !== name)]);

  // Out of quota: drop the oldest and try again, down to just this deck.
  while (decks.length > 0) {
    if (write(decks)) {
      emit();
      return deck;
    }
    if (decks.length === 1) break;
    decks.pop();
  }
  return null;
}

export function remove(id) {
  const kept = list().filter((d) => d.id !== id);
  const ok = write(kept);
  if (ok) emit();
  return ok;
}

export function clear() {
  // An empty list, not a removed key: a missing key would re-copy the old
  // unscoped library on the next load.
  const ok = write([]);
  if (ok) emit();
  return ok;
}

/** True when the browser lets this page store anything at all. */
export function available() {
  try {
    const probe = '__bs_probe__';
    localStorage.setItem(probe, '1');
    localStorage.removeItem(probe);
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
