// The instructions view: the source-format reference, verbatim, with one button
// that puts all of it on the clipboard — plus the packaged Claude skill.
//
// Raw markdown is deliberate: it is exactly what you paste into an assistant,
// and serving the repo's own file (copied in by scripts/build-web.sh) means this
// view can never drift from the reference.

'use strict';

const DOC_URL = 'presentation-format.md';

let text = '';
let loaded = false;

export function init() {
  const id = (n) => document.getElementById(n);
  const doc = id('doc'), meta = id('i-meta'), note = id('i-note');

  id('i-copy').addEventListener('click', async () => {
    if (!text) return;
    try {
      // navigator.clipboard needs a secure context; file:// and plain http on a
      // non-localhost origin do not qualify, hence the textarea fallback.
      if (navigator.clipboard && window.isSecureContext) await navigator.clipboard.writeText(text);
      else legacyCopy(text);
      note.textContent = '✓ copied — paste it into your assistant';
      note.className = 'ok';
    } catch (err) {
      note.textContent = `could not copy (${err.message}) — select the text below instead`;
      note.className = 'bad';
    }
    setTimeout(() => { note.textContent = ''; note.className = 'dim'; }, 6000);
  });

  id('i-download').addEventListener('click', () => {
    if (!text) return;
    const url = URL.createObjectURL(new Blob([text], { type: 'text/markdown' }));
    const a = document.createElement('a');
    a.href = url;
    a.download = 'bs-presentation-format.md';
    document.body.appendChild(a);
    a.click();
    a.remove();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  });

  // Fetched on first view rather than at startup: it is the largest asset here
  // and most sessions go straight to compile or present.
  return async function show() {
    if (loaded) return;
    loaded = true;
    try {
      const res = await fetch(DOC_URL, { cache: 'no-cache' });
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      text = await res.text();
      doc.textContent = text;
      meta.textContent = `${text.split('\n').length} lines · ${(new TextEncoder().encode(text).length / 1024).toFixed(1)} KB`;
    } catch (err) {
      loaded = false;
      doc.textContent = `could not load ${DOC_URL}: ${err.message}\n\n`
        + 'The reference lives at PRESENTATION_FORMAT.md in the repository.';
    }
  };
}

function legacyCopy(value) {
  const ta = document.createElement('textarea');
  ta.value = value;
  ta.setAttribute('readonly', '');
  ta.style.cssText = 'position:fixed;top:-1000px;opacity:0';
  document.body.appendChild(ta);
  ta.select();
  const ok = document.execCommand('copy');
  ta.remove();
  if (!ok) throw new Error('execCommand failed');
}
