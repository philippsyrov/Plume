//! macOS-specific machine-RAM reader.
//!
//! `sysctl -n hw.memsize` prints the total physical-memory byte count
//! to stdout as a decimal integer. We exec it once per call — the cost
//! is microseconds and the value is stable across the lifetime of the
//! window — so there's no reason to cache.
//!
//! Returns `None` on any non-fatal failure (sysctl missing, weird
//! locale, output that does not parse). The caller must already
//! treat `None` as "we don't know" rather than "the machine is small".

use std::process::Command;

pub fn physical_memory_bytes() -> Option<u64> {
    let out = Command::new("/usr/sbin/sysctl")
        .args(["-n", "hw.memsize"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = std::str::from_utf8(&out.stdout).ok()?.trim();
    text.parse::<u64>().ok()
}
