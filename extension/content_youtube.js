// ── Native Messaging ──────────────────────────────────────────────────────────
const bgPort = chrome.runtime.connect({ name: 'lyrics' });
let lastText = '';
let trackActive = false; // true once a textTrack delivers at least one cue

function sendText(text) {
  if (text === lastText) return;
  lastText = text;
  bgPort.postMessage({ text, source: 'youtube' });
}

// ── Timeline helpers ──────────────────────────────────────────────────────────

function makeSyncPayload(video) {
  return {
    currentTime: video.currentTime,
    wallTime: Date.now(),
    paused: video.paused,
    playbackRate: video.playbackRate,
  };
}

// Key that identifies a cue window by its boundaries and size.
// Avoids resending the same batch on every cuechange when the window hasn't moved.
let lastCueWindowKey = '';

function sendCueBatch(video, track) {
  if (!track.cues?.length) return;
  const seen = new Set();
  const cues = Array.from(track.cues)
    .map(c => ({
      start: c.startTime,
      end: c.endTime,
      text: c.text.replace(/<[^>]*>/g, '').replace(/\n/g, ' ').trim(),
    }))
    .filter(c => c.text && !seen.has(`${c.start}:${c.text}`) && seen.add(`${c.start}:${c.text}`));
  if (!cues.length) return;

  const key = `${cues[0].start}:${cues[cues.length - 1].end}:${cues.length}`;
  if (key === lastCueWindowKey) return;
  lastCueWindowKey = key;

  bgPort.postMessage({ type: 'cues', source: 'youtube', cues, ...makeSyncPayload(video) });
}

// YouTube may load cues lazily — poll until the window is populated, then send.
function waitForCues(video, track, tries = 20) {
  if (track.cues?.length) { sendCueBatch(video, track); return; }
  if (tries > 0) setTimeout(() => waitForCues(video, track, tries - 1), 300);
}

// ── textTracks — primary (works in background, feeds both modes) ───────────────
const seenTracks = new WeakSet();

function attachTrack(video, track) {
  if (seenTracks.has(track)) return;
  seenTracks.add(track);
  track.mode = 'hidden';

  // Send any cue that is already active right now (covers the case where the
  // app starts or the overlay is re-enabled mid-playback — cuechange won't
  // fire again for a cue that was already active before we attached).
  if (track.activeCues?.length) {
    const seen = new Set();
    const parts = Array.from(track.activeCues)
      .map(c => c.text.replace(/<[^>]*>/g, '').replace(/\n/g, ' ').trim())
      .filter(t => t && !seen.has(t) && seen.add(t));
    if (parts.length) { trackActive = true; sendText(parts.join(' ')); }
  }

  // Timeline mode: send the initial cue window, then refresh on every cuechange.
  // cuechange also fires when YouTube slides a rolling window, so this keeps
  // the Rust-side cue list up to date without a separate polling loop.
  waitForCues(video, track);

  track.addEventListener('cuechange', () => {
    const cues = track.activeCues;

    // Live mode: send the currently visible cue text.
    if (cues?.length) {
      const seen = new Set();
      const parts = Array.from(cues)
        .map(c => c.text.replace(/<[^>]*>/g, '').replace(/\n/g, ' ').trim())
        .filter(t => t && !seen.has(t) && seen.add(t));
      if (parts.length) {
        trackActive = true;
        sendText(parts.join(' '));
      }
    }

    // Timeline mode: refresh the cue window only if it has actually changed.
    sendCueBatch(video, track);
  });
}

function watchTracks(video) {
  const scan = () => {
    for (const t of video.textTracks)
      if ((t.kind === 'subtitles' || t.kind === 'captions') && t.mode !== 'disabled')
        attachTrack(video, t);
  };
  scan();
  video.textTracks.addEventListener('change', scan);
  setupSync(video);
}

// ── Sync heartbeat — keeps Timeline mode accurate between cue refreshes ────────
function setupSync(video) {
  const sendSync = () =>
    bgPort.postMessage({ type: 'sync', source: 'youtube', ...makeSyncPayload(video) });

  for (const ev of ['play', 'pause', 'seeked', 'ratechange'])
    video.addEventListener(ev, sendSync);
  setInterval(sendSync, 2000);
}

// ── MutationObserver — fallback when textTracks produces nothing ───────────────
function setupObserver() {
  const container = document.querySelector('.ytp-caption-window-container');
  if (!container) { setTimeout(setupObserver, 1000); return; }
  new MutationObserver(() => {
    if (trackActive) return; // textTracks path is working — don't double-send
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
  watchTracks(video);
  setupObserver();
}

setup();
