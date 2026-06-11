const APP_NAME = 'com.vitormakino.lyrics_on_screen';
let nativePort = null;
let pendingMessages = [];
const MAX_PENDING = 10;

function connectNative() {
  nativePort = chrome.runtime.connectNative(APP_NAME);
  nativePort.onDisconnect.addListener(() => {
    nativePort = null;
  });
  // Flush messages queued while the port was down.
  while (pendingMessages.length && nativePort) {
    try {
      nativePort.postMessage(pendingMessages.shift());
    } catch (_) {
      nativePort = null;
      break;
    }
  }
}

// Start the native app immediately when the service worker initialises.
// This avoids the cold-start delay on the first subtitle message.
connectNative();

chrome.runtime.onConnect.addListener((contentPort) => {
  if (!nativePort) connectNative();

  contentPort.onMessage.addListener((msg) => {
    if (!nativePort) connectNative();
    try {
      nativePort.postMessage(msg);
    } catch (_) {
      nativePort = null;
      // Queue the message so it's replayed after the next successful reconnect.
      if (pendingMessages.length < MAX_PENDING) pendingMessages.push(msg);
    }
  });
});
