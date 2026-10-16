//! The level's total EXP as an interval. The readout's percent is rounded or
//! truncated to 0.01%, so one reading only bounds the total; intersecting the
//! bounds across a level tightens them and exposes misreads.

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
}
