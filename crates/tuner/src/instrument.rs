//! Instrument presets: a fixed set of open-string MIDI targets the tuner snaps
//! to. `Chromatic` (the default) has no targets, so the tuner shows the nearest
//! of all 12 notes; every other preset reports the nearest string instead.

/// Selectable tuning preset. Indices are stable (persisted in the config).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Instrument {
    Chromatic,
    Guitar,
    Bass,
    Ukulele,
    Violin,
}

impl Instrument {
    pub fn index(self) -> usize {
        match self {
            Instrument::Chromatic => 0,
            Instrument::Guitar => 1,
            Instrument::Bass => 2,
            Instrument::Ukulele => 3,
            Instrument::Violin => 4,
        }
    }

    pub fn from_idx(i: usize) -> Self {
        match i {
            1 => Instrument::Guitar,
            2 => Instrument::Bass,
            3 => Instrument::Ukulele,
            4 => Instrument::Violin,
            _ => Instrument::Chromatic,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Instrument::Chromatic => "Cromático",
            Instrument::Guitar => "Violão",
            Instrument::Bass => "Baixo",
            Instrument::Ukulele => "Ukulele",
            Instrument::Violin => "Violino",
        }
    }

    /// Open-string MIDI numbers for this preset; empty for `Chromatic`.
    pub fn targets(self) -> &'static [i64] {
        match self {
            // E2 A2 D3 G3 B3 E4
            Instrument::Guitar => &[40, 45, 50, 55, 59, 64],
            // E1 A1 D2 G2
            Instrument::Bass => &[28, 33, 38, 43],
            // G4 C4 E4 A4 (standard reentrant)
            Instrument::Ukulele => &[67, 60, 64, 69],
            // G3 D4 A4 E5
            Instrument::Violin => &[55, 62, 69, 76],
            Instrument::Chromatic => &[],
        }
    }

    /// All presets in index order, for building the tray menu.
    pub const ALL: [Instrument; 5] = [
        Instrument::Chromatic,
        Instrument::Guitar,
        Instrument::Bass,
        Instrument::Ukulele,
        Instrument::Violin,
    ];
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idx_roundtrips() {
        for inst in Instrument::ALL {
            assert_eq!(Instrument::from_idx(inst.index()), inst);
        }
    }

    #[test]
    fn unknown_idx_falls_back_to_chromatic() {
        assert_eq!(Instrument::from_idx(99), Instrument::Chromatic);
    }

    #[test]
    fn target_counts() {
        assert_eq!(Instrument::Chromatic.targets().len(), 0);
        assert_eq!(Instrument::Guitar.targets().len(), 6);
        assert_eq!(Instrument::Bass.targets().len(), 4);
        assert_eq!(Instrument::Ukulele.targets().len(), 4);
        assert_eq!(Instrument::Violin.targets().len(), 4);
    }
}
