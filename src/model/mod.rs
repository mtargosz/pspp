//! Process tree construction, rollup/flat memory aggregation, sorting, and filtering.

mod tree;

use std::str::FromStr;

use crate::collect::MemSet;

pub use tree::{AggMode, AggregateRow, FilterMatchSet, ProcTree, SortBy, VisibleRow};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MemKind {
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
            MemKind::Rss => Some(mem.rss),
            MemKind::Vsz => Some(mem.vsz),
            MemKind::Footprint => mem.footprint,
            MemKind::Pss => mem.pss,
            MemKind::Uss => mem.uss,
            MemKind::Swap => mem.swap,
        }
    }

    pub fn available() -> Vec<MemKind> {
        let mut kinds = vec![MemKind::Rss, MemKind::Vsz];
        #[cfg(target_os = "macos")]
        kinds.push(MemKind::Footprint);
        #[cfg(target_os = "linux")]
        {
            kinds.push(MemKind::Pss);
            kinds.push(MemKind::Uss);
            kinds.push(MemKind::Swap);
        }
        kinds
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

impl FromStr for MemKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "rss" => Ok(MemKind::Rss),
            "vsz" => Ok(MemKind::Vsz),
            "footprint" => Ok(MemKind::Footprint),
            "pss" => Ok(MemKind::Pss),
            "uss" => Ok(MemKind::Uss),
            "swap" => Ok(MemKind::Swap),
            _ => Err(format!("unknown memory kind: {s}")),
        }
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
            rss: 1,
            vsz: 2,
            footprint: Some(3),
            pss: Some(4),
            uss: Some(5),
            swap: Some(6),
            approximate: false,
        };

        assert_eq!(MemKind::Rss.get(&mem), Some(1));
        assert_eq!(MemKind::Vsz.get(&mem), Some(2));
        assert_eq!(MemKind::Footprint.get(&mem), Some(3));
        assert_eq!(MemKind::Pss.get(&mem), Some(4));
        assert_eq!(MemKind::Uss.get(&mem), Some(5));
        assert_eq!(MemKind::Swap.get(&mem), Some(6));
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
