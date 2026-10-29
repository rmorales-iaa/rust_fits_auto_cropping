//! Worker-count policy. Memory is bounded separately by weighted per-image
//! admission (`crate::memory`), so workers follow the CPU configuration.

/// Resolve the effective processing worker count.
///
/// A configured value of zero means all available logical CPUs; explicit
/// counts are capped by the available CPUs.
pub fn effective_worker_count(configured_threads: usize, available_cores: usize) -> usize {
    let available = available_cores.max(1);
    if configured_threads == 0 {
        available
    } else {
        configured_threads.min(available).max(1)
    }
}

#[cfg(test)]
mod tests {
    use super::effective_worker_count;

    #[test]
    fn four_is_the_default_worker_limit() {
        assert_eq!(effective_worker_count(4, 32), 4);
    }

    #[test]
    fn zero_uses_all_available_cores() {
        assert_eq!(effective_worker_count(0, 24), 24);
    }

    #[test]
    fn worker_count_is_capped_by_cpu() {
        assert_eq!(effective_worker_count(8, 4), 4);
        assert_eq!(effective_worker_count(0, 1), 1);
    }
}
