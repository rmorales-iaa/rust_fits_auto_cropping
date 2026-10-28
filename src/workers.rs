//! Worker-count policy combining configured CPU parallelism with memory bounds.

/// Resolve the effective processing worker count.
///
/// A configured value of zero means all available logical CPUs. Both explicit
/// and automatic counts are capped by available CPUs and the image workspace
/// budget so Rayon does not oversubscribe the machine or memory allowance.
pub fn effective_worker_count(
    configured_threads: usize,
    available_cores: usize,
    memory_budget_mib: usize,
    maximum_image_mib: usize,
) -> usize {
    let available = available_cores.max(1);
    let requested = if configured_threads == 0 {
        available
    } else {
        configured_threads.min(available).max(1)
    };
    let memory_limited = (memory_budget_mib / maximum_image_mib.max(1)).max(1);
    requested.min(memory_limited).max(1)
}

#[cfg(test)]
mod tests {
    use super::effective_worker_count;

    #[test]
    fn four_is_the_default_worker_limit() {
        assert_eq!(effective_worker_count(4, 32, 16_384, 512), 4);
    }

    #[test]
    fn zero_uses_all_available_cores_when_memory_allows() {
        assert_eq!(effective_worker_count(0, 24, 16_384, 512), 24);
    }

    #[test]
    fn worker_count_is_capped_by_cpu_and_memory_limits() {
        assert_eq!(effective_worker_count(8, 4, 1_024, 512), 2);
        assert_eq!(effective_worker_count(0, 16, 1_024, 512), 2);
    }
}
