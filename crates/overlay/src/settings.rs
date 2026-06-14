use iced_layershell::reexport::Anchor;

// ── Position ──────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Position {
    Bottom,
    Top,
}

impl Position {
    pub fn anchor(self) -> Anchor {
        match self {
            Position::Bottom => Anchor::Bottom | Anchor::Left | Anchor::Right,
            Position::Top => Anchor::Top | Anchor::Left | Anchor::Right,
        }
    }
    pub fn margin(self) -> (i32, i32, i32, i32) {
        match self {
            Position::Bottom => (0, 0, 40, 0),
            Position::Top => (40, 0, 0, 0),
        }
    }
    pub fn index(self) -> usize {
        match self {
            Position::Bottom => 0,
            Position::Top => 1,
        }
    }
}

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
