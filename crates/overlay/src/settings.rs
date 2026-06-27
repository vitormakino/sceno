// ── FontSize ──────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum FontSize {
    Small,
    Medium,
    Large,
}

impl FontSize {
    pub fn px(self) -> f32 {
        match self {
            FontSize::Small => 22.0,
            FontSize::Medium => 32.0,
            FontSize::Large => 44.0,
        }
    }
    pub fn index(self) -> usize {
        match self {
            FontSize::Small => 0,
            FontSize::Medium => 1,
            FontSize::Large => 2,
        }
    }
    pub fn from_idx(i: usize) -> Self {
        match i {
            0 => FontSize::Small,
            2 => FontSize::Large,
            _ => FontSize::Medium,
        }
    }
}

// ── Config I/O ────────────────────────────────────────────────────────────────

pub fn config_path(app: &str) -> Option<std::path::PathBuf> {
    crate::paths::config_dir(app).map(|d| d.join("config.json"))
}

pub fn load_config<T: Default + serde::de::DeserializeOwned>(app: &str) -> T {
    config_path(app)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Like [`load_config`], but distinguishes "no usable config" from a valid one:
/// returns `None` when the file is missing **or malformed** instead of silently
/// substituting defaults. Config watching uses this so a half-typed external
/// edit (e.g. a JSON syntax error) is ignored rather than read as "the settings
/// changed to the defaults", which would wipe the user's saved values.
pub fn load_config_checked<T: serde::de::DeserializeOwned>(app: &str) -> Option<T> {
    config_path(app)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
}

/// Like [`load_config`], but writes the defaults to disk first if no config
/// file exists yet, then loads. This *materializes* a discoverable, editable
/// `config.json` on first run — the only configuration surface on macOS, where
/// there is no tray. Best-effort (a failed write just falls back to in-memory
/// defaults, exactly like [`load_config`]).
pub fn load_or_seed<T>(app: &str) -> T
where
    T: Default + serde::Serialize + serde::de::DeserializeOwned,
{
    let missing = config_path(app).map(|p| !p.exists()).unwrap_or(false);
    if missing {
        save(app, &T::default());
    }
    load_config(app)
}

/// Background stream that emits `on_change()` whenever `app`'s `config.json`
/// changes on disk (created, edited, or removed). Polls the file's mtime ~1 Hz
/// on its own thread — one `stat` per second, negligible cost — rather than
/// pulling in an OS file-watch dependency. Drive it via `Subscription::run` so
/// external edits to the JSON apply live without a restart (notably on macOS,
/// where the JSON is the only way to reconfigure).
///
/// Callers should treat the emitted message as "config *might* have changed"
/// and no-op when the reloaded config equals the running one, so a self-write
/// (e.g. a tray toggle persisting) doesn't bounce back as a reload.
pub fn watch_config_stream<M, F>(app: &str, on_change: F) -> futures::stream::BoxStream<'static, M>
where
    M: Send + 'static,
    F: Fn() -> M + Send + 'static,
{
    let (tx, rx) = futures::channel::mpsc::unbounded::<M>();
    let path = config_path(app);
    std::thread::spawn(move || {
        let Some(path) = path else { return };
        let mtime = |p: &std::path::Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
        let mut last = mtime(&path);
        loop {
            std::thread::sleep(std::time::Duration::from_millis(1000));
            let cur = mtime(&path);
            if cur != last {
                last = cur;
                if tx.unbounded_send(on_change()).is_err() {
                    break;
                }
            }
        }
    });
    Box::pin(rx)
}

pub fn save<T: serde::Serialize>(app: &str, cfg: &T) {
    if cfg!(test) {
        return;
    }
    let Some(path) = config_path(app) else { return };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string(cfg) {
        let _ = std::fs::write(path, json);
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fontsize_from_idx_roundtrips() {
        for (i, expected) in [
            (0, FontSize::Small),
            (1, FontSize::Medium),
            (2, FontSize::Large),
        ] {
            assert_eq!(FontSize::from_idx(i), expected);
            assert_eq!(expected.index(), i);
        }
    }

    #[test]
    fn fontsize_unknown_idx_defaults_to_medium() {
        assert_eq!(FontSize::from_idx(99), FontSize::Medium);
    }
}
