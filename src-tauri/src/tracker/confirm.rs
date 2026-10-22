//! Debounces map changes: a signature counts once seen on CONFIRM_FRAMES consecutive
//! readable frames. Callers must not feed unreadable frames — those neither confirm
//! nor break a streak.

use crate::geom::Crop;
use crate::read::map_strip::Signature;

const CONFIRM_FRAMES: usize = 3;

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
