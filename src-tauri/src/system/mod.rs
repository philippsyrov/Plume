//! Host machine introspection.
//!
//! D3 added `physical_memory_bytes()` so the model-fit estimator could
//! ask the host how much RAM it has. D5 grows the module into a full
//! cheap-and-honest machine snapshot used by the trusted-project
//! status strip: total / used / free memory, swap, the kernel's
//! 1/5/15-minute load average, CPU brand, arch, and macOS version.
//!
//! Every signal is best-effort and per-platform. Today only macOS is
//! implemented because that is Plume's first-class target
//! (`docs/PLUME_PROJECT_SPEC.md § 5`). Other platforms get a snapshot
//! with every field `None` and the UI reads that as "we don't know"
//! rather than "things are fine".
//!
//! No new crate deps. macOS readers shell out to the same `sysctl`
//! / `vm_stat` / `uname` / `sw_vers` tools Activity Monitor and
//! `top` use internally. The cost is microseconds per command; even
//! a 5–10 s polling cadence stays comfortably in the noise.

use serde::Serialize;

#[cfg(target_os = "macos")]
mod macos;

/// Total physical memory in bytes for the host machine, when the
/// platform supports a cheap read. Returns `None` when the signal is
/// unavailable; callers must distinguish "unknown" from "low" to
/// avoid handing the user a wrong fit verdict.
///
/// Kept as a standalone helper (separate from [`snapshot`]) because
/// the fit estimator (`providers::fit`) reads it on every
/// `providers.modelDetails` call and does not need the rest of the
/// machine snapshot.
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

/// Take a single cheap snapshot of host machine state — memory,
/// swap, load average, identifying labels. Every field is optional;
/// a missing field is "we could not read this", not "the value is
/// zero". Callers must render the two differently.
pub fn snapshot() -> MachineSnapshot {
    #[cfg(target_os = "macos")]
    {
        macos::snapshot()
    }
    #[cfg(not(target_os = "macos"))]
    {
        MachineSnapshot {
            probed_at_ms: unix_ms(),
            physical_memory_bytes: None,
            memory: None,
            swap: None,
            load_average: None,
            pressure: MemoryPressure::Unknown,
            arch: None,
            os_name: None,
            os_version: None,
            cpu_brand: None,
        }
    }
}

/// Wall-clock helper. Duplicated from `providers::health` rather
/// than shared because both call sites are one-liners and pulling
/// it up costs another module dependency for no real win.
pub(crate) fn unix_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}

/// One reading of host machine state. The struct is wire-compatible
/// with the `system.snapshot` IPC verb (see `docs/IPC_CONTRACT.md`).
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MachineSnapshot {
    /// Unix epoch milliseconds when the snapshot was taken.
    pub probed_at_ms: u64,
    /// Authoritative total physical RAM (from `hw.memsize` on macOS).
    /// `MemoryStats.total_bytes` should match this when both are
    /// present, but kernel page accounting can drift by a few MB; the
    /// UI should treat this field as canonical.
    pub physical_memory_bytes: Option<u64>,
    pub memory: Option<MemoryStats>,
    pub swap: Option<SwapStats>,
    pub load_average: Option<LoadAverage>,
    /// Best-effort headline of how close to memory pressure the host
    /// feels. Derived from `(active + wired + compressed) / total`
    /// because the kernel's `kern.memorystatus_vm_pressure_level`
    /// sysctl requires elevated privileges on most macOS versions.
    pub pressure: MemoryPressure,
    /// `uname -m`: `"arm64"`, `"x86_64"`.
    pub arch: Option<String>,
    /// `sw_vers -productName`: usually `"macOS"`.
    pub os_name: Option<String>,
    /// `sw_vers -productVersion`: e.g. `"14.5"`.
    pub os_version: Option<String>,
    /// `machdep.cpu.brand_string`: e.g. `"Apple M2 Pro"`.
    pub cpu_brand: Option<String>,
}

/// Per-category memory bytes. Numbers approximate what Activity
/// Monitor shows under "Memory" — they come from the same `vm_stat`
/// the system tool uses, so they match within a few MB.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MemoryStats {
    /// macOS kernel page size, in bytes. 16 384 on Apple Silicon,
    /// 4 096 on Intel. Surfaced for callers that want to do their
    /// own math with the raw page counts.
    pub page_size_bytes: u64,
    /// Page counts × page size, rounded to bytes.
    pub free_bytes: u64,
    pub active_bytes: u64,
    pub inactive_bytes: u64,
    pub wired_bytes: u64,
    pub compressed_bytes: u64,
    /// "Memory Used" the way Activity Monitor displays it: active +
    /// wired + compressed.
    pub used_bytes: u64,
    /// Best-effort headroom: free + inactive (purgeable). Matches
    /// the macOS notion of "memory we can probably get back without
    /// hurting anyone".
    pub available_bytes: u64,
    /// vm_stat's running total of page counts × page size. May
    /// differ from `physical_memory_bytes` by a few MB because of
    /// kernel-reserved pages.
    pub total_bytes: u64,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SwapStats {
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub free_bytes: u64,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LoadAverage {
    pub one: f64,
    pub five: f64,
    pub fifteen: f64,
}

/// Memory-pressure headline displayed on the strip. Derived
/// heuristically from used vs total; *not* the kernel pressure
/// level (which we cannot read without elevated privileges).
///
/// The `Normal` / `Warn` / `High` verdicts are only ever constructed
/// by `MemoryPressure::derive`, which today runs exclusively in the
/// macOS backend (`system::macos`). On other targets the snapshot
/// reports `Unknown`, so those three variants read as dead code there
/// — allow it off-macOS while keeping the lint live on macOS, where
/// the variants must stay wired.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub enum MemoryPressure {
    /// Plenty of headroom. Today: used < 60 % of total.
    Normal,
    /// Getting full. Today: used in [60 %, 85 %).
    Warn,
    /// Close to swapping or already swapping hard. Today: used
    /// ≥ 85 % of total, OR swap-used > 50 % of swap-total.
    High,
    /// We could not read enough signal to classify.
    Unknown,
}

impl MemoryPressure {
    /// Run the heuristic. Public so tests in `macos` and future
    /// platform modules can share the verdict. Only the macOS backend
    /// calls this in a real build today; gated `allow(dead_code)` off
    /// macOS so non-Apple targets don't warn while the macOS lint
    /// stays honest. The `#[cfg(test)]` tests below exercise it on
    /// every platform.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub fn derive(memory: Option<&MemoryStats>, swap: Option<&SwapStats>) -> Self {
        let Some(memory) = memory else {
            return MemoryPressure::Unknown;
        };
        if memory.total_bytes == 0 {
            return MemoryPressure::Unknown;
        }
        let used_ratio = memory.used_bytes as f64 / memory.total_bytes as f64;
        let swap_pressure = swap
            .filter(|s| s.total_bytes > 0)
            .map(|s| s.used_bytes as f64 / s.total_bytes as f64)
            .unwrap_or(0.0);
        if used_ratio >= 0.85 || swap_pressure >= 0.5 {
            MemoryPressure::High
        } else if used_ratio >= 0.60 {
            MemoryPressure::Warn
        } else {
            MemoryPressure::Normal
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pressure_normal_for_idle_machine() {
        let memory = MemoryStats {
            page_size_bytes: 16384,
            free_bytes: 4 * 1024 * 1024 * 1024,
            active_bytes: 2 * 1024 * 1024 * 1024,
            inactive_bytes: 1024 * 1024 * 1024,
            wired_bytes: 2 * 1024 * 1024 * 1024,
            compressed_bytes: 0,
            used_bytes: 4 * 1024 * 1024 * 1024,
            available_bytes: 5 * 1024 * 1024 * 1024,
            total_bytes: 16 * 1024 * 1024 * 1024,
        };
        assert_eq!(
            MemoryPressure::derive(Some(&memory), None),
            MemoryPressure::Normal
        );
    }

    #[test]
    fn pressure_warn_for_busy_machine() {
        let memory = MemoryStats {
            page_size_bytes: 16384,
            free_bytes: 0,
            active_bytes: 8 * 1024 * 1024 * 1024,
            inactive_bytes: 0,
            wired_bytes: 3 * 1024 * 1024 * 1024,
            compressed_bytes: 0,
            used_bytes: 11 * 1024 * 1024 * 1024,
            available_bytes: 0,
            total_bytes: 16 * 1024 * 1024 * 1024,
        };
        assert_eq!(
            MemoryPressure::derive(Some(&memory), None),
            MemoryPressure::Warn
        );
    }

    #[test]
    fn pressure_high_for_almost_full_memory() {
        let memory = MemoryStats {
            page_size_bytes: 16384,
            free_bytes: 0,
            active_bytes: 10 * 1024 * 1024 * 1024,
            inactive_bytes: 0,
            wired_bytes: 4 * 1024 * 1024 * 1024,
            compressed_bytes: 0,
            used_bytes: 14 * 1024 * 1024 * 1024,
            available_bytes: 0,
            total_bytes: 16 * 1024 * 1024 * 1024,
        };
        assert_eq!(
            MemoryPressure::derive(Some(&memory), None),
            MemoryPressure::High
        );
    }

    #[test]
    fn pressure_high_when_swap_heavily_used() {
        // Memory itself looks fine — but if swap is more than half
        // used the machine is paging hard and we should flag it.
        let memory = MemoryStats {
            page_size_bytes: 16384,
            free_bytes: 0,
            active_bytes: 4 * 1024 * 1024 * 1024,
            inactive_bytes: 0,
            wired_bytes: 4 * 1024 * 1024 * 1024,
            compressed_bytes: 0,
            used_bytes: 8 * 1024 * 1024 * 1024,
            available_bytes: 0,
            total_bytes: 16 * 1024 * 1024 * 1024,
        };
        let swap = SwapStats {
            total_bytes: 4 * 1024 * 1024 * 1024,
            used_bytes: 3 * 1024 * 1024 * 1024,
            free_bytes: 1024 * 1024 * 1024,
        };
        assert_eq!(
            MemoryPressure::derive(Some(&memory), Some(&swap)),
            MemoryPressure::High
        );
    }

    #[test]
    fn pressure_unknown_without_memory_stats() {
        assert_eq!(MemoryPressure::derive(None, None), MemoryPressure::Unknown);
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn physical_memory_is_reported_on_macos() {
        // We refuse to assert a specific size, but a Mac must have at
        // least 2 GiB of RAM to run a modern release; if we get anything
        // back, it ought to be in that ballpark or higher.
        let bytes = physical_memory_bytes().expect("macOS reports hw.memsize");
        assert!(bytes > 2 * 1024 * 1024 * 1024, "implausibly small: {bytes}");
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn snapshot_is_populated_on_macos() {
        // Smoke-level: the real CLI tools live on every Mac, so a
        // snapshot in CI / dev must produce some non-`None` fields.
        // We don't assert specific numbers — they depend on what
        // else is running.
        let snap = snapshot();
        assert!(snap.physical_memory_bytes.is_some(), "no hw.memsize");
        assert!(snap.memory.is_some(), "no vm_stat");
        assert!(snap.load_average.is_some(), "no vm.loadavg");
        // Arch is read from `uname -m` and should always succeed.
        assert!(snap.arch.is_some(), "no arch");
    }
}
