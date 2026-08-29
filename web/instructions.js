// The instructions page: serve PRESENTATION_FORMAT.md verbatim, with one button
// that puts the whole thing on the clipboard.
//
// Raw markdown is deliberate — it is exactly what you want to paste into an
// assistant, and it means this page can never drift from the repo's reference:
// scripts/build-web.sh copies the file straight out of the repo root at build
// time rather than keeping a second copy here.

'use strict';

const DOC_URL = 'presentation-format.md';
const el = (id) => document.getElementById(id);

let text = '';

(async () => {
  try {
    const res = await fetch(DOC_URL, { cache: 'no-cache' });
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    text = await res.text();
    el('doc').textContent = text;
    const lines = text.split('\n').length;
    const kb = (new TextEncoder().encode(text).length / 1024).toFixed(1);
    el('meta').textContent = `${lines} lines · ${kb} KB`;
  } catch (err) {
    el('doc').textContent =
      `could not load ${DOC_URL}: ${err.message}\n\n` +
      'The reference lives at PRESENTATION_FORMAT.md in the repository.';
    el('copy').disabled = true;
    el('download').disabled = true;
  }
})();

el('copy').addEventListener('click', async () => {
  if (!text) return;
  const note = el('copy-note');
  try {
    // navigator.clipboard needs a secure context; file:// and plain http on a
    // non-localhost origin do not qualify, hence the textarea fallback.
    if (navigator.clipboard && window.isSecureContext) {
      await navigator.clipboard.writeText(text);
    } else {
      legacyCopy(text);
    }
    note.textContent = '✓ copied — now paste it into your assistant';
    note.className = 'ok';
  } catch (err) {
    note.textContent = `could not copy automatically (${err.message}) — select the text below instead`;
    note.className = 'bad';
  }
  setTimeout(() => { note.textContent = ''; note.className = 'dim'; }, 6000);
});

function legacyCopy(value) {
  const ta = document.createElement('textarea');
  ta.value = value;
  ta.setAttribute('readonly', '');
  ta.style.cssText = 'position:fixed;top:-1000px;opacity:0';
  document.body.appendChild(ta);
  ta.select();
  const ok = document.execCommand('copy');
  document.body.removeChild(ta);
  if (!ok) throw new Error('execCommand failed');
}

el('download').addEventListener('click', () => {
  if (!text) return;
  const url = URL.createObjectURL(new Blob([text], { type: 'text/markdown' }));
  const a = document.createElement('a');
  a.href = url;
  a.download = 'bs-presentation-format.md';
  a.click();
  URL.revokeObjectURL(url);
});
