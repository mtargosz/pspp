//! Process tree construction, rollup/flat memory aggregation, sorting, and filtering.

mod tree;

use std::fmt;
use std::str::FromStr;

use clap::builder::PossibleValue;
use clap::ValueEnum;

use crate::collect::MemSet;

pub use tree::{AggMode, AggregateRow, FilterMatchSet, ProcTree, SortBy, VisibleRow};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum MemKind {
    #[default]
    Rss,
    Vsz,
    Footprint,
    Pss,
    Uss,
    Swap,
}

impl MemKind {
    pub fn get(&self, mem: &MemSet) -> Option<u64> {
        match self {
            MemKind::Rss => mem.rss,
            MemKind::Vsz => mem.vsz,
            MemKind::Footprint => mem.footprint,
            MemKind::Pss => mem.pss,
            MemKind::Uss => mem.uss,
            MemKind::Swap => mem.swap,
        }
    }

    /// Every kind, whether or not this platform can report it.
    pub const ALL: [MemKind; 6] = [
        MemKind::Rss,
        MemKind::Vsz,
        MemKind::Footprint,
        MemKind::Pss,
        MemKind::Uss,
        MemKind::Swap,
    ];

    /// Kinds this platform can report (`footprint` on macOS; `pss/uss/swap` on Linux).
    pub fn available() -> &'static [MemKind] {
        #[cfg(target_os = "macos")]
        const AVAILABLE: &[MemKind] = &[MemKind::Rss, MemKind::Vsz, MemKind::Footprint];
        #[cfg(target_os = "linux")]
        const AVAILABLE: &[MemKind] = &[
            MemKind::Rss,
            MemKind::Vsz,
            MemKind::Pss,
            MemKind::Uss,
            MemKind::Swap,
        ];
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        const AVAILABLE: &[MemKind] = &[MemKind::Rss, MemKind::Vsz];
        AVAILABLE
    }

    /// Config/CLI spelling (lowercase).
    pub fn as_str(&self) -> &'static str {
        match self {
            MemKind::Rss => "rss",
            MemKind::Vsz => "vsz",
            MemKind::Footprint => "footprint",
            MemKind::Pss => "pss",
            MemKind::Uss => "uss",
            MemKind::Swap => "swap",
        }
    }

    /// Parses a config value; unknown or platform-unavailable kinds fall back to RSS.
    pub fn from_config(value: &str) -> MemKind {
        value
            .parse::<MemKind>()
            .ok()
            .filter(|kind| MemKind::available().contains(kind))
            .unwrap_or_default()
    }

    pub fn label(&self) -> &'static str {
        match self {
            MemKind::Rss => "RSS",
            MemKind::Vsz => "VSZ",
            MemKind::Footprint => "Footprint",
            MemKind::Pss => "PSS",
            MemKind::Uss => "USS",
            MemKind::Swap => "Swap",
        }
    }
}

impl fmt::Display for MemKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for MemKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let lowered = s.trim().to_ascii_lowercase();
        MemKind::ALL
            .into_iter()
            .find(|kind| kind.as_str() == lowered)
            .ok_or_else(|| format!("unknown memory kind: {s}"))
    }
}

/// clap exposes only the kinds this platform can report, so `--help` and error messages list
/// exactly the valid choices.
impl ValueEnum for MemKind {
    fn value_variants<'a>() -> &'a [Self] {
        MemKind::available()
    }

    fn to_possible_value(&self) -> Option<PossibleValue> {
        Some(PossibleValue::new(self.as_str()))
    }
}

#[cfg(test)]
mod tests {
    use super::MemKind;
    use crate::collect::MemSet;
    use std::str::FromStr;

    #[test]
    fn mem_kind_get_reads_fields() {
        let mem = MemSet {
            rss: Some(1),
            vsz: Some(2),
            footprint: Some(3),
            pss: Some(4),
            uss: Some(5),
            swap: Some(6),
        };

        assert_eq!(MemKind::Rss.get(&mem), Some(1));
        assert_eq!(MemKind::Vsz.get(&mem), Some(2));
        assert_eq!(MemKind::Footprint.get(&mem), Some(3));
        assert_eq!(MemKind::Pss.get(&mem), Some(4));
        assert_eq!(MemKind::Uss.get(&mem), Some(5));
        assert_eq!(MemKind::Swap.get(&mem), Some(6));

        let empty = MemSet::default();
        for kind in MemKind::ALL {
            assert_eq!(kind.get(&empty), None);
        }
    }

    #[test]
    fn mem_kind_string_forms_round_trip() {
        for kind in MemKind::ALL {
            assert_eq!(kind.to_string(), kind.as_str());
            assert_eq!(MemKind::from_str(kind.as_str()), Ok(kind));
            assert_eq!(MemKind::from_str(&kind.as_str().to_uppercase()), Ok(kind));
        }
        assert_eq!(MemKind::default(), MemKind::Rss);
    }

    #[test]
    fn available_kinds_are_a_platform_subset_and_drive_clap() {
        use clap::ValueEnum;
        let available = MemKind::available();
        assert!(available.contains(&MemKind::Rss) && available.contains(&MemKind::Vsz));
        assert_eq!(MemKind::value_variants(), available);
        #[cfg(target_os = "macos")]
        assert!(available.contains(&MemKind::Footprint) && !available.contains(&MemKind::Pss));
        #[cfg(target_os = "linux")]
        assert!(available.contains(&MemKind::Pss) && !available.contains(&MemKind::Footprint));
    }

    #[test]
    fn from_config_falls_back_to_rss_for_unknown_or_unavailable_kinds() {
        assert_eq!(MemKind::from_config("vsz"), MemKind::Vsz);
        assert_eq!(MemKind::from_config("  VSZ "), MemKind::Vsz);
        assert_eq!(MemKind::from_config("bogus"), MemKind::Rss);
        assert_eq!(MemKind::from_config(""), MemKind::Rss);
        for kind in MemKind::ALL {
            let expected = if MemKind::available().contains(&kind) {
                kind
            } else {
                MemKind::Rss
            };
            assert_eq!(MemKind::from_config(kind.as_str()), expected);
        }
    }

    #[test]
    fn mem_kind_from_str_parses_cli_values() {
        assert_eq!(MemKind::from_str("rss"), Ok(MemKind::Rss));
        assert_eq!(MemKind::from_str("vsz"), Ok(MemKind::Vsz));
        assert_eq!(MemKind::from_str("footprint"), Ok(MemKind::Footprint));
        assert_eq!(MemKind::from_str("pss"), Ok(MemKind::Pss));
        assert_eq!(MemKind::from_str("uss"), Ok(MemKind::Uss));
        assert_eq!(MemKind::from_str("swap"), Ok(MemKind::Swap));
        assert!(MemKind::from_str("nope").is_err());
    }
}
