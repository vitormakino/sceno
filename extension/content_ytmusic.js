// YouTube Music shows timed lyrics line by line.
// The active line selector may vary — inspect the DOM if this stops working.
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
    ws.send(JSON.stringify({ text, source: 'ytmusic' }));
  }
}

// Poll for the currently highlighted lyrics line every 500ms.
setInterval(() => {
  const active =
    document.querySelector('.lyrics-player-lyrics-renderer [active]') ||
    document.querySelector('ytmusic-player-lyrics-panel [active]') ||
    document.querySelector('[data-is-current="true"]');
  if (active) {
    sendText(active.textContent.trim());
  }
}, 500);

connect();
