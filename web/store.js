// The deck library: compiled presentations kept in localStorage so they survive
// a reload, an app restart, and being offline.
//
// Storage is small (~5 MB) and a compiled deck is often >100 KB, so this is a
// *recents* list, not an archive: MAX_DECKS newest are kept and a write that
// runs out of room evicts the oldest and retries rather than failing.

'use strict';

const KEY = 'bs:decks';
const MAX_DECKS = 20;

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
 * stored record, or null if storage is unavailable even after evicting.
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

  // Same name replaces the older entry rather than piling up duplicates.
  let decks = list().filter((d) => d.name !== name);
  decks.unshift(deck);
  if (decks.length > MAX_DECKS) decks = decks.slice(0, MAX_DECKS);

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
  try {
    localStorage.removeItem(KEY);
    emit();
    return true;
  } catch {
    return false;
  }
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
