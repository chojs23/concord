//! Allocator tuning for the image pipeline's allocation pattern.
//!
//! Decoding images and building terminal graphics protocols allocates and frees
//! buffers continuously, large ones on the blocking pool. Measured over ten
//! minutes of scrolling image-heavy channels, before any of this: 400MB
//! resident against 16MB of live cache data, with every cache inside its
//! budget the whole time. Two glibc behaviours account for that gap.
//!
//! The mmap threshold is the larger one. glibc raises it every time it frees an
//! mmapped block, assuming the next allocation of that size is better served
//! from the arena; here that heuristic backfires, because those arena blocks
//! only return to the OS when they happen to sit at the top of the heap.
//! Pinning it kept retention to 0.3MB across 400 rounds of multi-megabyte churn
//! where the default kept 7.3MB.
//!
//! The remainder comes back from trimming on a timer. Eight blocking-pool
//! threads churning mixed buffer sizes peaked at 17.3MB above baseline
//! untuned; the pinned threshold alone took the peak to 8.9MB and a trim took
//! what was left to 6.2MB.
//!
//! Capping arenas with `M_ARENA_MAX` was measured here and deliberately left
//! out. It does bind (12 arenas down to 2), but with the mmap threshold
//! already pinned it held 7.3MB after a trim against 6.2MB uncapped, while a
//! loop that does nothing but allocate ran 66% slower. It only looked like a
//! win when measured against an untuned mmap threshold.
//!
//! mimalloc was measured as an alternative and rejected: fastest of the three,
//! but it retained 31MB on the same churn because it holds thread-local heaps
//! instead of returning them.

#[cfg(all(target_os = "linux", target_env = "gnu"))]
mod glibc {
    use std::ffi::c_int;

    // glibc malloc.h. Negative values, since they predate the standard set.
    const M_TRIM_THRESHOLD: c_int = -1;
    const M_MMAP_THRESHOLD: c_int = -3;

    // Large enough that ordinary allocations keep using the arena, small enough
    // that image buffers and protocol payloads always go through mmap and are
    // handed back on free.
    const MMAP_THRESHOLD_BYTES: c_int = 128 * 1024;
    const TRIM_THRESHOLD_BYTES: c_int = 4 * 1024 * 1024;

    unsafe extern "C" {
        fn mallopt(param: c_int, value: c_int) -> c_int;
        fn malloc_trim(pad: usize) -> c_int;
    }

    pub(super) fn tune() {
        unsafe {
            // Pinning the threshold disables glibc's dynamic adjustment.
            mallopt(M_MMAP_THRESHOLD, MMAP_THRESHOLD_BYTES);
            mallopt(M_TRIM_THRESHOLD, TRIM_THRESHOLD_BYTES);
        }
    }

    pub(super) fn trim() {
        unsafe {
            malloc_trim(0);
        }
    }
}

/// Configures the allocator for long sessions. Call once at startup, before any
/// threads are spawned.
pub fn tune() {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    glibc::tune();
}

/// Returns free arena memory to the operating system. Call on a timer: it is
/// what recovers the churn the pinned mmap threshold does not.
pub fn trim() {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    glibc::trim();
}
