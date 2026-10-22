//! The level's total EXP. The readout's percent is rounded or truncated to
//! 0.01%, so one reading only bounds the total; intersecting the bounds across
//! a level tightens them and exposes misreads. Once exactly one level's
//! table total fits the bounds, the player's level and exact total are known.

use crate::read::exp::ExpReading;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TotalBounds {
    pub lo: u64,
    pub hi: u64,
}

impl TotalBounds {
    /// Total ∈ [exp·10000/(p+1), exp·10000/(p−1)]. `None` when p < 2 or exp = 0.
    pub fn from_reading(r: ExpReading) -> Option<Self> {
        if r.pct_bp < 2 || r.exp == 0 {
            return None;
        }
        let scaled = u128::from(r.exp) * 10_000;
        let lo = scaled / u128::from(r.pct_bp + 1);
        let hi = scaled.div_ceil(u128::from(r.pct_bp - 1));
        Some(Self { lo: u64::try_from(lo).ok()?, hi: u64::try_from(hi).ok()? })
    }

    pub fn intersect(self, other: Self) -> Option<Self> {
        let lo = self.lo.max(other.lo);
        let hi = self.hi.min(other.hi);
        (lo <= hi).then_some(Self { lo, hi })
    }

    pub fn estimate(self) -> u64 {
        self.lo + (self.hi - self.lo) / 2
    }

    pub fn contains(self, total: u64) -> bool {
        (self.lo..=self.hi).contains(&total)
    }

    /// The one level whose table total lies inside the bounds; `None` if none or several do.
    pub fn matching_level(self) -> Option<u32> {
        let mut fits = (1..=MAX_LEVEL).filter(|&l| exp_to_next(l).is_some_and(|t| self.contains(t)));
        let level = fits.next()?;
        fits.next().is_none().then_some(level)
    }
}

const MAX_LEVEL: u32 = 99;

/// EXP needed to go from `level` to `level + 1` (levels 1–99).
///
/// Source: the community table at meowdb.com/msclassic/guides/exp-table-level-1-to-100
/// (levels 1–89 are pre-Big-Bang MapleStory's rates). NOT confirmed by Nexon.
/// Level 41's total (324,240) matched this tracker's own measurement on 2026-10-16;
/// the tracker logs a warning whenever readings disagree with the table.
pub fn exp_to_next(level: u32) -> Option<u64> {
    if !(1..=MAX_LEVEL).contains(&level) {
        return None;
    }
    let l = u64::from(level);
    Some(match level {
        // Levels 1–5: (floor(level² / 2) + 15) × level
        1..=5 => (l * l / 2 + 15) * l,
        // Levels 6–50: q = floor(level² / 3), then (q + 19) × q
        6..=50 => {
            let q = l * l / 3;
            (q + 19) * q
        }
        // Levels 51–89: previous level × 1.0548, rounded down
        51..=89 => exp_to_next(level - 1)? * 10_548 / 10_000,
        // Levels 90–99: previous level × 2.1, rounded down (fits the table; no formula published)
        _ => exp_to_next(level - 1)? * 21 / 10,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_matches_the_published_values() {
        let published = [(1, 15), (41, 324_240), (50, 709_716), (51, 748_608), (89, 5_684_790), (90, 11_938_059), (99, 9_482_161_548)];
        for (level, exp) in published {
            assert_eq!(exp_to_next(level), Some(exp), "level {level}");
        }
    }
}
