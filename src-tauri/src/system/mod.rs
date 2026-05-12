//! Host machine introspection.
//!
//! D3 needs the host's physical-memory size so the model-fit estimator
//! can answer "does this model fit on this Mac?" honestly. The signal
//! is per-platform; today only macOS is implemented because that is
//! Plume's first-class target (`docs/PLUME_PROJECT_SPEC.md § 5`). Other
//! platforms return `None` and the UI reads that as "we do not know"
//! rather than "fits comfortably".
//!
//! No new crate deps. macOS reads `hw.memsize` via the `sysctl` CLI;
//! adding `libc` or `sysctlbyname-rs` just to skip one `Command::new`
//! is not worth the dep surface today.

#[cfg(target_os = "macos")]
mod macos;

/// Total physical memory in bytes for the host machine, when the
/// platform supports a cheap read. Returns `None` when the signal is
/// unavailable; callers must distinguish "unknown" from "low" to
/// avoid handing the user a wrong fit verdict.
pub fn physical_memory_bytes() -> Option<u64> {
    #[cfg(target_os = "macos")]
    {
        macos::physical_memory_bytes()
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(target_os = "macos")]
    fn physical_memory_is_reported_on_macos() {
        // We refuse to assert a specific size, but a Mac must have at
        // least 2 GiB of RAM to run a modern release; if we get anything
        // back, it ought to be in that ballpark or higher.
        let bytes = physical_memory_bytes().expect("macOS reports hw.memsize");
        assert!(bytes > 2 * 1024 * 1024 * 1024, "implausibly small: {bytes}");
    }
}
