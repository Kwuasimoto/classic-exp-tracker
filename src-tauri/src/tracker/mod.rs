//! Pure EXP/session state machine and map debouncing. No I/O.

mod confirm;

pub use confirm::{ConfirmOutcome, MapConfirmer, CONFIRM_FRAMES};
