//! Debounces map changes: a signature counts once seen on CONFIRM_FRAMES consecutive
//! readable frames. Callers must not feed unreadable frames — those neither confirm
//! nor break a streak.

use crate::geom::Crop;
use crate::read::map_strip::Signature;

pub const CONFIRM_FRAMES: usize = 3;

#[derive(Debug, PartialEq, Eq)]
pub enum ConfirmOutcome {
    Unchanged,
    /// A new map was confirmed; `strips` are the confirming crops (for OCR labelling).
    Changed { signature: Signature, strips: Vec<Crop> },
}

#[derive(Debug, Default)]
pub struct MapConfirmer {
    confirmed: Option<Signature>,
    candidate: Option<(Signature, Vec<Crop>)>,
}

impl MapConfirmer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn feed(&mut self, signature: Signature, strip: Crop) -> ConfirmOutcome {
        if self.confirmed.as_ref().is_some_and(|c| c.matches(&signature)) {
            self.candidate = None;
            return ConfirmOutcome::Unchanged;
        }
        let (sig, strips) = match self.candidate.take() {
            Some((sig, mut strips)) if sig.matches(&signature) => {
                strips.push(strip);
                (sig, strips)
            }
            _ => (signature, vec![strip]),
        };
        if strips.len() >= CONFIRM_FRAMES {
            self.confirmed = Some(sig.clone());
            return ConfirmOutcome::Changed { signature: sig, strips };
        }
        self.candidate = Some((sig, strips));
        ConfirmOutcome::Unchanged
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sig(v: u8) -> Signature {
        Signature(vec![v; 8])
    }
    fn strip() -> Crop {
        Crop::new(1, 1, vec![0; 4]).expect("valid buffer")
    }

    #[test]
    fn confirms_after_three_matching_frames() {
        let mut c = MapConfirmer::new();
        assert_eq!(c.feed(sig(10), strip()), ConfirmOutcome::Unchanged);
        assert_eq!(c.feed(sig(10), strip()), ConfirmOutcome::Unchanged);
        match c.feed(sig(10), strip()) {
            ConfirmOutcome::Changed { signature, strips } => {
                assert_eq!(signature, sig(10));
                assert_eq!(strips.len(), CONFIRM_FRAMES);
            }
            other => panic!("expected Changed, got {other:?}"),
        }
        assert_eq!(c.feed(sig(10), strip()), ConfirmOutcome::Unchanged);
    }

    #[test]
    fn a_different_frame_restarts_the_streak() {
        let mut c = MapConfirmer::new();
        c.feed(sig(10), strip());
        c.feed(sig(10), strip());
        c.feed(sig(50), strip());
        c.feed(sig(10), strip());
        c.feed(sig(10), strip());
        assert!(matches!(c.feed(sig(10), strip()), ConfirmOutcome::Changed { .. }));
    }

    #[test]
    fn flicker_back_to_confirmed_map_cancels_candidate() {
        let mut c = MapConfirmer::new();
        for _ in 0..3 {
            c.feed(sig(10), strip());
        }
        c.feed(sig(50), strip());
        c.feed(sig(50), strip());
        assert_eq!(c.feed(sig(10), strip()), ConfirmOutcome::Unchanged);
        assert_eq!(c.feed(sig(50), strip()), ConfirmOutcome::Unchanged);
    }
}
