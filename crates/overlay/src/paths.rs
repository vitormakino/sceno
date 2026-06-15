//! XDG base-directory resolution for sceno apps.
//!
//! Config lives under `$XDG_CONFIG_HOME/sceno/<app>` (fallback
//! `~/.config/sceno/<app>`); cache under `$XDG_CACHE_HOME/sceno/<app>`
//! (fallback `~/.cache/sceno/<app>`); persistent data under
//! `$XDG_DATA_HOME/sceno/<app>` (fallback `~/.local/share/sceno/<app>`).

use std::path::PathBuf;

/// Pure XDG base resolution (env values injected for testability): use `xdg`
/// when it is `Some` and non-empty, otherwise `$HOME/<fallback>`; then append
/// `sceno/<app>`. Returns `None` only when neither source is available.
fn resolve(xdg: Option<&str>, home: Option<&str>, fallback: &str, app: &str) -> Option<PathBuf> {
    // Reject app names that would escape the sceno/ subtree.
    if app.is_empty() || app.starts_with('.') || app.contains('/') || app.contains('\\') {
        return None;
    }
    let root = match xdg {
        Some(x) if !x.is_empty() => PathBuf::from(x),
        _ => PathBuf::from(home?).join(fallback),
    };
    Some(root.join("sceno").join(app))
}

/// `$XDG_CONFIG_HOME/sceno/<app>` (fallback `~/.config/sceno/<app>`).
pub fn config_dir(app: &str) -> Option<PathBuf> {
    let xdg = std::env::var("XDG_CONFIG_HOME").ok();
    let home = std::env::var("HOME").ok();
    resolve(xdg.as_deref(), home.as_deref(), ".config", app)
}

/// `$XDG_CACHE_HOME/sceno/<app>` (fallback `~/.cache/sceno/<app>`).
pub fn cache_dir(app: &str) -> Option<PathBuf> {
    let xdg = std::env::var("XDG_CACHE_HOME").ok();
    let home = std::env::var("HOME").ok();
    resolve(xdg.as_deref(), home.as_deref(), ".cache", app)
}

/// Per-app data directory: `$XDG_DATA_HOME/sceno/<app>` (fallback
/// `~/.local/share/sceno/<app>`). Persistent, user-browsable files each app
/// owns — UltraStar `.txt` for `karaoke`, downloaded `.lrc` for `lyrics` — kept
/// per-app (not one mixed folder) so the two file kinds don't intermingle.
pub fn data_dir(app: &str) -> Option<PathBuf> {
    let xdg = std::env::var("XDG_DATA_HOME").ok();
    let home = std::env::var("HOME").ok();
    resolve(xdg.as_deref(), home.as_deref(), ".local/share", app)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xdg_var_takes_precedence() {
        let p = resolve(Some("/run/cfg"), Some("/home/u"), ".config", "lyrics").unwrap();
        assert_eq!(p, PathBuf::from("/run/cfg/sceno/lyrics"));
    }

    #[test]
    fn empty_xdg_falls_back_to_home() {
        let p = resolve(Some(""), Some("/home/u"), ".config", "lyrics").unwrap();
        assert_eq!(p, PathBuf::from("/home/u/.config/sceno/lyrics"));
    }

    #[test]
    fn missing_xdg_uses_home() {
        let p = resolve(None, Some("/home/u"), ".cache", "tuner").unwrap();
        assert_eq!(p, PathBuf::from("/home/u/.cache/sceno/tuner"));
    }

    #[test]
    fn no_home_no_xdg_is_none() {
        assert!(resolve(None, None, ".config", "lyrics").is_none());
    }

    #[test]
    fn data_dir_uses_data_home_layout_per_app() {
        let p = resolve(
            Some("/run/data"),
            Some("/home/u"),
            ".local/share",
            "karaoke",
        )
        .unwrap();
        assert_eq!(p, PathBuf::from("/run/data/sceno/karaoke"));
        let p = resolve(None, Some("/home/u"), ".local/share", "lyrics").unwrap();
        assert_eq!(p, PathBuf::from("/home/u/.local/share/sceno/lyrics"));
    }

    #[test]
    fn rejects_app_names_that_escape() {
        for bad in ["/etc", "../../etc", "a/b", "", ".hidden"] {
            assert!(
                resolve(Some("/run/cfg"), Some("/home/u"), ".config", bad).is_none(),
                "should reject {bad:?}"
            );
        }
    }
}
