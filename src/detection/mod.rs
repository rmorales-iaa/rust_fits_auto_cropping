//! Conservative tile-based border detector. Scores support review; they are not probabilities.
mod correlation;
mod multiscale;
mod reference;
pub(crate) mod robust;
mod tile_detector;
mod v2;
pub use tile_detector::{Detection, detect_border};
