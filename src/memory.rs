//! Weighted memory admission (plan R6).
//!
//! Each image reserves its estimated peak working set from one shared budget
//! after the header preflight and before any pixel buffer is allocated, and
//! releases it when processing ends. Large and small images therefore share
//! the budget by weight instead of a fixed worker cap.
//!
//! The estimate is an upper envelope fitted to measured peak RSS of release
//! single-image runs (validation.md, 2026-10-29), not a hard guarantee:
//!   peak = pixels * (stored bytes + 8 + detector + preview) + 64 MiB
//! with 8 bytes for the f64 analysis values, detector overhead 16 B/px for
//! background_v2, 2 B/px for tile_v1 single-scale and 4 B/px for multiscale,
//! and 12 B/px when crop previews are rendered.
use crate::config::{DetectorKind, EvidenceModel, HeuristicRules};
use anyhow::bail;
use std::sync::{Arc, Condvar, Mutex};

const MIB: u64 = 1024 * 1024;
const BASE_BYTES: u64 = 64 * MIB;

/// Per-run workload parameters that scale the per-pixel estimate.
#[derive(Clone, Copy, Debug)]
pub struct WorkloadProfile {
    detector_bytes_per_pixel: u64,
    preview_bytes_per_pixel: u64,
}

impl WorkloadProfile {
    pub fn new(rules: &HeuristicRules, previews: bool) -> Self {
        let detector_bytes_per_pixel = match (rules.detector, rules.evidence_model) {
            (DetectorKind::BackgroundV2, _) => 16,
            (DetectorKind::TileV1, EvidenceModel::SingleScale) => 2,
            (DetectorKind::TileV1, EvidenceModel::MultiscaleConsensus) => 4,
        };
        Self {
            detector_bytes_per_pixel,
            preview_bytes_per_pixel: if previews { 12 } else { 0 },
        }
    }

    /// Estimated peak bytes for an image of `pixels` samples at `bitpix`.
    pub fn peak_bytes(&self, pixels: usize, bitpix: i64) -> u64 {
        let stored = bitpix.unsigned_abs() / 8;
        let per_pixel = stored + 8 + self.detector_bytes_per_pixel + self.preview_bytes_per_pixel;
        (pixels as u64)
            .saturating_mul(per_pixel)
            .saturating_add(BASE_BYTES)
    }
}

impl Default for WorkloadProfile {
    /// The shipped detector without previews.
    fn default() -> Self {
        Self {
            detector_bytes_per_pixel: 16,
            preview_bytes_per_pixel: 0,
        }
    }
}

/// Shared memory budget; reservations block until enough is free.
pub struct MemoryBudget {
    total: u64,
    used: Mutex<u64>,
    freed: Condvar,
}

impl MemoryBudget {
    pub fn new(total_mib: usize) -> Arc<Self> {
        Arc::new(Self {
            total: (total_mib as u64).saturating_mul(MIB),
            used: Mutex::new(0),
            freed: Condvar::new(),
        })
    }

    /// Reserve `bytes`, waiting while other images hold the budget. A request
    /// larger than the whole budget fails immediately.
    pub fn reserve(self: &Arc<Self>, bytes: u64) -> anyhow::Result<Reservation> {
        if bytes > self.total {
            bail!(
                "image needs an estimated {} MiB, more than memory_budget_mib={}",
                bytes.div_ceil(MIB),
                self.total / MIB
            );
        }
        let mut used = self.used.lock().unwrap_or_else(|e| e.into_inner());
        while *used + bytes > self.total {
            used = self.freed.wait(used).unwrap_or_else(|e| e.into_inner());
        }
        *used += bytes;
        Ok(Reservation {
            budget: Arc::clone(self),
            bytes,
        })
    }

    #[cfg(test)]
    fn used(&self) -> u64 {
        *self.used.lock().unwrap()
    }
}

/// Held for the lifetime of an image's processing; releases on drop.
pub struct Reservation {
    budget: Arc<MemoryBudget>,
    bytes: u64,
}

impl Drop for Reservation {
    fn drop(&mut self) {
        let mut used = self.budget.used.lock().unwrap_or_else(|e| e.into_inner());
        *used = used.saturating_sub(self.bytes);
        self.budget.freed.notify_all();
    }
}

/// Admission parameters handed to the reader.
#[derive(Clone)]
pub struct Admission {
    pub budget: Arc<MemoryBudget>,
    pub profile: WorkloadProfile,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn estimate_bounds_the_measured_peaks() {
        // (pixels, bitpix, detector overhead, previews, measured peak MiB)
        let v2 = WorkloadProfile::default();
        let v2_png = WorkloadProfile {
            preview_bytes_per_pixel: 12,
            ..v2
        };
        let v1 = WorkloadProfile {
            detector_bytes_per_pixel: 2,
            preview_bytes_per_pixel: 0,
        };
        let n = 7100 * 5325;
        for (profile, bitpix, measured) in [
            (v2, -32, 877.0),
            (v2, 16, 805.0),
            (v2_png, -32, 1228.0),
            (v2_png, 16, 1161.0),
            (v1, -32, 450.0),
        ] {
            let estimate = profile.peak_bytes(n, bitpix) as f64 / MIB as f64;
            assert!(estimate >= measured, "{estimate} < {measured}");
            assert!(
                estimate <= 1.6 * measured,
                "{estimate} too loose for {measured}"
            );
        }
    }

    #[test]
    fn reservations_block_until_released_and_oversized_requests_fail() {
        let budget = MemoryBudget::new(100);
        assert!(budget.reserve(101 * MIB).is_err());
        let first = budget.reserve(60 * MIB).unwrap();
        let waiter = {
            let budget = Arc::clone(&budget);
            std::thread::spawn(move || budget.reserve(60 * MIB).map(|r| r.bytes).unwrap())
        };
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(budget.used(), 60 * MIB, "second reservation must wait");
        drop(first);
        assert_eq!(waiter.join().unwrap(), 60 * MIB);
        assert_eq!(budget.used(), 0);
    }
}
