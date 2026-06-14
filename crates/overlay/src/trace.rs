//! Opt-in stderr tracing shared by the sceno apps, gated by `SCENO_DEBUG`.

use std::sync::OnceLock;

/// Whether `SCENO_DEBUG` is set in the environment (read once and cached).
pub fn debug_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("SCENO_DEBUG").is_some())
}

/// When `SCENO_DEBUG` is set, print `[tag] <args>` to stderr; otherwise a no-op.
/// Each app passes its own `tag` (e.g. `"player"`, `"tuner"`).
pub fn debug(tag: &str, args: std::fmt::Arguments) {
    if debug_enabled() {
        eprintln!("[{tag}] {args}");
    }
}
