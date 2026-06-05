let ws = null;
let lastText = '';

function connect() {
  ws = new WebSocket('ws://localhost:8765');
  ws.onopen = () => console.log('[lyrics-on-screen] connected');
  ws.onclose = () => setTimeout(connect, 3000);
  ws.onerror = () => ws.close();
}

function sendText(text) {
  if (text === lastText) return;
  lastText = text;
  if (ws && ws.readyState === WebSocket.OPEN) {
    ws.send(JSON.stringify({ text, source: 'youtube' }));
  }
}

function setupObserver() {
  const container = document.querySelector('.ytp-caption-window-container');
  if (!container) {
    setTimeout(setupObserver, 1000);
    return;
  }
  new MutationObserver(() => {
    const segments = container.querySelectorAll('.ytp-caption-segment');
    const text = Array.from(segments).map(s => s.textContent).join(' ').trim();
    sendText(text);
  }).observe(container, { childList: true, subtree: true, characterData: true });
}

connect();
setupObserver();
