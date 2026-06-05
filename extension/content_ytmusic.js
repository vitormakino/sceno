let ws = null;
let lastText = '';

function connect() {
  ws = new WebSocket('ws://127.0.0.1:8765');
  ws.onopen = () => console.log('[lyrics-on-screen] ytmusic connected');
  ws.onclose = () => setTimeout(connect, 3000);
  ws.onerror = () => ws.close();
}

function sendText(text) {
  if (text === lastText) return;
  lastText = text;
  if (ws?.readyState === WebSocket.OPEN)
    ws.send(JSON.stringify({ text, source: 'ytmusic' }));
}

// Seletores para a linha de letra atualmente ativa.
// O YouTube Music muda esses atributos/classes entre versões — se parar de
// funcionar, abra o DevTools no YouTube Music com a letra aberta e olhe o
// console para ver o que foi encontrado (veja debugLyricsPanel abaixo).
const ACTIVE_SELECTORS = [
  'ytmusic-player-lyrics-panel [active]',
  'ytmusic-player-lyrics-panel [is-current]',
  'ytmusic-player-lyrics-panel [current-line]',
  'ytmusic-player-lyrics-panel [aria-current="true"]',
  '.lyrics-player-lyrics-renderer [active]',
  '.ytmusic-player-lyrics-panel-content [active]',
  '[data-is-current="true"]',
  '[class*="lyric"][active]',
  '[class*="lyric"][current]',
];

function findActiveLyric() {
  for (const sel of ACTIVE_SELECTORS) {
    const el = document.querySelector(sel);
    if (el) return el.textContent.trim();
  }
  return null;
}

// ── Debug: loga o conteúdo do painel de letras quando encontrado ──────────────
// Útil para encontrar os seletores corretos quando os acima não funcionarem.
// Veja o console do DevTools (F12) na aba do YouTube Music com letras abertas.
let _debugDone = false;
function debugLyricsPanel() {
  if (_debugDone) return;
  const panel =
    document.querySelector('ytmusic-player-lyrics-panel') ||
    document.querySelector('[class*="lyrics-panel"]');
  if (!panel) return;
  _debugDone = true;
  console.log('[lyrics-on-screen] painel de letras encontrado:', panel.tagName);
  const children = [...panel.querySelectorAll('*')].slice(0, 20);
  children.forEach(el => {
    const attrs = [...el.attributes].map(a => `${a.name}="${a.value}"`).join(' ');
    const text = el.textContent?.trim().slice(0, 60);
    if (text) console.log(`  <${el.tagName.toLowerCase()} ${attrs}> ${text}`);
  });
}

function checkLyrics() {
  debugLyricsPanel();
  const text = findActiveLyric();
  if (text) sendText(text);
}

new MutationObserver(checkLyrics).observe(document.body, {
  subtree: true,
  childList: true,
  attributes: true,
  attributeFilter: ['active', 'is-current', 'current-line', 'aria-current', 'data-is-current'],
});

setInterval(checkLyrics, 1000);
connect();
