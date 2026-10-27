//! Conservative tile-based border detector. Scores support review; they are not probabilities.
mod correlation;
mod reference;
mod robust;
mod tile_detector;
pub use tile_detector::detect_border;
