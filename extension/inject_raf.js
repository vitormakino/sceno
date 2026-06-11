// Runs in MAIN world at document_start — before any YouTube scripts load.
// Overrides document.hidden so Chrome won't throttle rAF in background tabs.
const _hiddenDesc = Object.getOwnPropertyDescriptor(Document.prototype, 'hidden');
const isReallyHidden = _hiddenDesc ? _hiddenDesc.get.bind(document) : () => false;

Object.defineProperty(document, 'hidden',          { get: () => false, configurable: true });
Object.defineProperty(document, 'visibilityState', { get: () => 'visible', configurable: true });

window.addEventListener('visibilitychange', e => e.stopImmediatePropagation(), true);
document.addEventListener('visibilitychange', e => e.stopImmediatePropagation(), true);

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
