// ── 1. rAF injection ─────────────────────────────────────────────────────────
// Faz o YouTube acreditar que a aba está sempre visível e substitui
// requestAnimationFrame por setTimeout no background (10fps).
// Executado no contexto da página (main world) via tag <script>.
const injectCode = `
  const _hiddenDesc = Object.getOwnPropertyDescriptor(Document.prototype, 'hidden');
  const isReallyHidden = _hiddenDesc ? _hiddenDesc.get.bind(document) : () => false;

  Object.defineProperty(document, 'hidden',          { get: () => false, configurable: true });
  Object.defineProperty(document, 'visibilityState', { get: () => 'visible', configurable: true });

  const _raf = window.requestAnimationFrame.bind(window);
  const _caf = window.cancelAnimationFrame.bind(window);
  const _timers = new Set();

  window.requestAnimationFrame = cb => {
    if (isReallyHidden()) {
      const id = setTimeout(() => { _timers.delete(id); cb(performance.now()); }, 100);
      _timers.add(id);
      return id;
    }
    return _raf(cb);
  };
  window.cancelAnimationFrame = id => {
    if (_timers.has(id)) { _timers.delete(id); clearTimeout(id); }
    else _caf(id);
  };
`;
const _s = document.createElement('script');
_s.textContent = injectCode;
document.documentElement.appendChild(_s);
_s.remove();

// ── 2. WebSocket ──────────────────────────────────────────────────────────────
let ws = null;
let lastText = '';

function connect() {
  ws = new WebSocket('ws://127.0.0.1:8765');
  ws.onopen = () => console.log('[lyrics-on-screen] connected');
  ws.onclose = () => setTimeout(connect, 3000);
  ws.onerror = () => ws.close();
}

function sendText(text) {
  if (text === lastText) return;
  lastText = text;
  if (ws?.readyState === WebSocket.OPEN)
    ws.send(JSON.stringify({ text, source: 'youtube' }));
}

// ── 3. video.textTracks — abordagem primária para background ──────────────────
// cuechange é disparado pela timeline da mídia, não pelo renderer.
// Funciona mesmo com a aba em background ou em outro workspace.
const seenTracks = new WeakSet();

function attachTrack(track) {
  if (seenTracks.has(track)) return;
  seenTracks.add(track);
  // 'hidden' dispara cuechange sem mostrar o overlay nativo do browser
  track.mode = 'hidden';
  track.addEventListener('cuechange', () => {
    const cues = track.activeCues;
    if (!cues?.length) { sendText(''); return; }
    sendText(
      Array.from(cues)
        .map(c => c.text.replace(/<[^>]*>/g, '').replace(/\n/g, ' '))
        .join(' ')
        .trim()
    );
  });
}

function watchTracks(video) {
  const scan = () => {
    for (const t of video.textTracks)
      if ((t.kind === 'subtitles' || t.kind === 'captions') && t.mode !== 'disabled')
        attachTrack(t);
  };
  scan();
  video.textTracks.addEventListener('change', scan);
}

// ── 4. MutationObserver — fallback se textTracks estiver vazio ────────────────
function setupObserver() {
  const container = document.querySelector('.ytp-caption-window-container');
  if (!container) { setTimeout(setupObserver, 1000); return; }
  new MutationObserver(() => {
    // Tenta os dois seletores — YouTube muda nomes de classe entre versões
    const els = container.querySelectorAll('.ytp-caption-segment, .caption-visual-line');
    const text = els.length
      ? Array.from(els).map(e => e.textContent).join(' ').trim()
      : container.textContent.trim();
    sendText(text);
  }).observe(container, { childList: true, subtree: true, characterData: true });
}

// ── Bootstrap ─────────────────────────────────────────────────────────────────
function setup() {
  const video = document.querySelector('video');
  if (!video) { setTimeout(setup, 1000); return; }
  watchTracks(video);  // primário: funciona em background
  setupObserver();     // secundário: cobre versões do YouTube sem textTracks
}

connect();
setup();
