//! macOS-specific machine introspection.
//!
//! Every reader here shells out to a stock macOS CLI tool (`sysctl`,
//! `vm_stat`, `uname`, `sw_vers`) and parses the output. The cost is
//! microseconds per command — Activity Monitor and `top` use the same
//! underlying APIs — so a 5–10 s polling cadence stays comfortably in
//! the noise. We deliberately do NOT pull in `libc` / `mach-sys` to
//! call `host_statistics` / `sysctl` directly; the dep cost is too
//! large for the value, and we'd have to recreate the same parsing
//! logic anyway. If a future slice needs faster sampling or signals
//! the CLI tools don't expose (per-process pressure, GPU power), swap
//! to mach-sys then.
//!
//! Format references — verified by running each command on a real
//! Mac before writing the parsers:
//!   * `vm_stat` — header line `(page size of N bytes)`, then `Pages
//!     X: N.` lines.
//!   * `sysctl vm.swapusage` — `vm.swapusage: total = X.YYU  used =
//!     X.YYU  free = X.YYU  (encrypted)`.
//!   * `sysctl -n vm.loadavg` — `{ 1.23 1.50 1.75 }`.
//!   * `uname -m` — `arm64`.
//!   * `sw_vers -productName` / `-productVersion` — single-line value.
//!   * `sysctl -n machdep.cpu.brand_string` — single-line value.

use std::process::Command;

use super::{unix_ms, LoadAverage, MachineSnapshot, MemoryPressure, MemoryStats, SwapStats};

/// Read `hw.memsize`. Lives here (not inline in `mod.rs`) so the
/// fit estimator can keep calling `super::physical_memory_bytes()`
/// while the full snapshot machinery sits next to it.
pub fn physical_memory_bytes() -> Option<u64> {
    let text = sysctl(&["-n", "hw.memsize"])?;
    text.trim().parse::<u64>().ok()
}

pub fn snapshot() -> MachineSnapshot {
    let memory = vm_stat_output().and_then(|text| parse_vm_stat(&text));
    let swap = sysctl(&["vm.swapusage"]).and_then(|text| parse_swapusage(&text));
    let load_average = sysctl(&["-n", "vm.loadavg"]).and_then(|text| parse_loadavg(&text));
    let pressure = MemoryPressure::derive(memory.as_ref(), swap.as_ref());

    MachineSnapshot {
        probed_at_ms: unix_ms(),
        physical_memory_bytes: physical_memory_bytes(),
        memory,
        swap,
        load_average,
        pressure,
        arch: command_first_line(&["/usr/bin/uname", "-m"], &[]),
        os_name: sw_vers("productName"),
        os_version: sw_vers("productVersion"),
        cpu_brand: sysctl(&["-n", "machdep.cpu.brand_string"]).map(|s| s.trim().to_string()),
    }
}

// --- command helpers -------------------------------------------------

fn sysctl(args: &[&str]) -> Option<String> {
    let out = Command::new("/usr/sbin/sysctl").args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = std::str::from_utf8(&out.stdout).ok()?.to_string();
    Some(text)
}

fn sw_vers(field: &str) -> Option<String> {
    let arg = match field {
        "productName" => "-productName",
        "productVersion" => "-productVersion",
        _ => return None,
    };
    command_first_line(&["/usr/bin/sw_vers", arg], &[])
}

/// Run `cmd args` and return the trimmed first line of stdout, or
/// `None` on any failure. Used for the small single-value commands
/// (`uname`, `sw_vers`).
fn command_first_line(cmd_with_args: &[&str], extra: &[&str]) -> Option<String> {
    let (cmd, args) = cmd_with_args.split_first()?;
    let out = Command::new(cmd).args(args).args(extra).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = std::str::from_utf8(&out.stdout).ok()?;
    text.lines().next().map(|s| s.trim().to_string())
}

fn vm_stat_output() -> Option<String> {
    let out = Command::new("/usr/bin/vm_stat").output().ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8(out.stdout).ok()
}

// --- parsers ---------------------------------------------------------

/// Parse the output of `vm_stat`. The header carries the page size
/// (`16384` on Apple Silicon, `4096` on Intel); subsequent lines
/// look like `Pages X: N.` where N is a page count followed by a
/// trailing period. Unknown lines are tolerated — we only read the
/// keys we surface.
fn parse_vm_stat(text: &str) -> Option<MemoryStats> {
    let mut page_size_bytes: Option<u64> = None;
    let mut free = 0u64;
    let mut active = 0u64;
    let mut inactive = 0u64;
    let mut wired = 0u64;
    let mut compressed = 0u64;

    for line in text.lines() {
        let line = line.trim();
        if let Some(size) = parse_page_size_header(line) {
            page_size_bytes = Some(size);
            continue;
        }
        let Some((key, pages)) = parse_pages_line(line) else {
            continue;
        };
        match key.as_str() {
            "Pages free" => free = pages,
            "Pages active" => active = pages,
            "Pages inactive" => inactive = pages,
            "Pages wired down" => wired = pages,
            "Pages occupied by compressor" => compressed = pages,
            _ => {}
        }
    }

    let page_size_bytes = page_size_bytes?;
    let free_bytes = free.saturating_mul(page_size_bytes);
    let active_bytes = active.saturating_mul(page_size_bytes);
    let inactive_bytes = inactive.saturating_mul(page_size_bytes);
    let wired_bytes = wired.saturating_mul(page_size_bytes);
    let compressed_bytes = compressed.saturating_mul(page_size_bytes);

    // "Memory Used" the way Activity Monitor displays it: app memory
    // + wired + compressed. Inactive pages are *not* used in the
    // user-facing sense because they can be reclaimed on demand.
    let used_bytes = active_bytes
        .saturating_add(wired_bytes)
        .saturating_add(compressed_bytes);
    // Best-effort "free for apps": free + inactive (purgeable).
    let available_bytes = free_bytes.saturating_add(inactive_bytes);

    // `vm_stat` doesn't expose a single "total" line, so we
    // reconstruct it from the categories we read. Will be off by a
    // few MiB vs `hw.memsize` because we ignored speculative,
    // throttled, etc.; the UI should treat `physical_memory_bytes`
    // as canonical and use this as a sanity check.
    let total_bytes = free_bytes
        .saturating_add(active_bytes)
        .saturating_add(inactive_bytes)
        .saturating_add(wired_bytes)
        .saturating_add(compressed_bytes);

    Some(MemoryStats {
        page_size_bytes,
        free_bytes,
        active_bytes,
        inactive_bytes,
        wired_bytes,
        compressed_bytes,
        used_bytes,
        available_bytes,
        total_bytes,
    })
}

fn parse_page_size_header(line: &str) -> Option<u64> {
    // "Mach Virtual Memory Statistics: (page size of 16384 bytes)"
    let marker = "page size of ";
    let start = line.find(marker)? + marker.len();
    let tail = &line[start..];
    let end = tail.find(' ')?;
    tail[..end].parse::<u64>().ok()
}

fn parse_pages_line(line: &str) -> Option<(String, u64)> {
    // `Pages free:                  12345.` — key is everything left
    // of the colon, value is the int before the trailing period.
    let (key, rest) = line.split_once(':')?;
    let key = key.trim().to_string();
    // Only treat lines whose key starts with "Pages " — `vm_stat`
    // emits other lines like "Translation faults:" we don't care
    // about.
    if !key.starts_with("Pages ") {
        return None;
    }
    let value = rest.trim().trim_end_matches('.');
    let pages: u64 = value.parse().ok()?;
    Some((key, pages))
}

/// Parse `vm.swapusage: total = X.YYU  used = X.YYU  free = X.YYU
/// (encrypted)`. The unit is M/K/G — we normalize to bytes.
fn parse_swapusage(text: &str) -> Option<SwapStats> {
    let line = text.lines().next()?.trim();
    let body = line.strip_prefix("vm.swapusage:")?.trim();
    let total = extract_swap_field(body, "total")?;
    let used = extract_swap_field(body, "used")?;
    let free = extract_swap_field(body, "free")?;
    Some(SwapStats {
        total_bytes: total,
        used_bytes: used,
        free_bytes: free,
    })
}

fn extract_swap_field(body: &str, key: &str) -> Option<u64> {
    // Find `key = NN.NNU`. Spacing varies between macOS versions, so
    // we search for `key` followed by `=`.
    let start = body.find(key)?;
    let after = &body[start + key.len()..];
    let eq = after.find('=')?;
    let rest = after[eq + 1..].trim_start();
    let end = rest.find(|c: char| c.is_whitespace()).unwrap_or(rest.len());
    let token = &rest[..end];
    parse_byte_quantity(token)
}

/// Parse a `vm.swapusage`-style number like `"3072.00M"` into bytes.
fn parse_byte_quantity(token: &str) -> Option<u64> {
    let (number, unit) = split_quantity(token)?;
    let factor: u64 = match unit {
        "" | "B" => 1,
        "K" | "KB" => 1024,
        "M" | "MB" => 1024 * 1024,
        "G" | "GB" => 1024 * 1024 * 1024,
        "T" | "TB" => 1024u64.pow(4),
        _ => return None,
    };
    let value: f64 = number.parse().ok()?;
    Some((value * factor as f64) as u64)
}

fn split_quantity(token: &str) -> Option<(&str, &str)> {
    let idx = token
        .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == '+'))
        .unwrap_or(token.len());
    let (num, unit) = token.split_at(idx);
    if num.is_empty() {
        return None;
    }
    Some((num, unit))
}

/// Parse `{ 2.59 2.67 2.80 }`.
fn parse_loadavg(text: &str) -> Option<LoadAverage> {
    let line = text.lines().next()?.trim();
    let trimmed = line.trim_start_matches('{').trim_end_matches('}').trim();
    let mut parts = trimmed.split_whitespace();
    let one = parts.next()?.parse().ok()?;
    let five = parts.next()?.parse().ok()?;
    let fifteen = parts.next()?.parse().ok()?;
    Some(LoadAverage { one, five, fifteen })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verbatim output of `vm_stat` on an Apple Silicon mac. Trimmed
    /// to the keys we read plus a couple noise lines we must ignore.
    const VM_STAT_FIXTURE: &str = "\
Mach Virtual Memory Statistics: (page size of 16384 bytes)
Pages free:                               14940.
Pages active:                            203531.
Pages inactive:                          200500.
Pages speculative:                         2035.
Pages throttled:                              0.
Pages wired down:                        155913.
Pages purgeable:                            597.
\"Translation faults\":                8996534152.
Pages occupied by compressor:            417531.
";

    #[test]
    fn parse_vm_stat_extracts_expected_fields() {
        let mem = parse_vm_stat(VM_STAT_FIXTURE).expect("parse_vm_stat");
        assert_eq!(mem.page_size_bytes, 16384);
        assert_eq!(mem.free_bytes, 14_940u64 * 16_384);
        assert_eq!(mem.active_bytes, 203_531u64 * 16_384);
        assert_eq!(mem.inactive_bytes, 200_500u64 * 16_384);
        assert_eq!(mem.wired_bytes, 155_913u64 * 16_384);
        assert_eq!(mem.compressed_bytes, 417_531u64 * 16_384);
        // Used = active + wired + compressed; matches Activity Monitor.
        assert_eq!(mem.used_bytes, (203_531u64 + 155_913 + 417_531) * 16_384);
        // Available = free + inactive.
        assert_eq!(mem.available_bytes, (14_940u64 + 200_500) * 16_384);
        // Total reconstructed from the categories we read.
        assert_eq!(
            mem.total_bytes,
            (14_940u64 + 203_531 + 200_500 + 155_913 + 417_531) * 16_384
        );
    }

    #[test]
    fn parse_vm_stat_rejects_output_without_page_size() {
        let text = "Pages free: 100.\nPages active: 200.\n";
        assert!(parse_vm_stat(text).is_none());
    }

    #[test]
    fn parse_pages_line_skips_non_pages_keys() {
        assert!(parse_pages_line("\"Translation faults\": 1234.").is_none());
        assert!(parse_pages_line("Pageins: 1234.").is_none());
        assert!(parse_pages_line("Swapouts: 1234.").is_none());
        let parsed = parse_pages_line("Pages free: 100.").unwrap();
        assert_eq!(parsed, ("Pages free".into(), 100));
    }

    #[test]
    fn parse_swapusage_extracts_total_used_free() {
        let line = "vm.swapusage: total = 3072.00M  used = 2104.44M  free = 967.56M  (encrypted)";
        let swap = parse_swapusage(line).expect("parse_swapusage");
        assert_eq!(swap.total_bytes, 3072u64 * 1024 * 1024);
        assert!(swap.used_bytes > 2_100u64 * 1024 * 1024);
        assert!(swap.used_bytes < 2_110u64 * 1024 * 1024);
        // total ≈ used + free; the f64→u64 cast can drop a byte at
        // the boundary, so allow a tiny rounding gap rather than
        // requiring exact equality.
        let sum = swap.used_bytes + swap.free_bytes;
        let diff = swap.total_bytes.abs_diff(sum);
        assert!(diff <= 2, "total {} vs used+free {}", swap.total_bytes, sum);
    }

    #[test]
    fn parse_swapusage_with_zero_swap() {
        // macOS reports zero-swap as `0.00M`, not as a missing line.
        let line = "vm.swapusage: total = 0.00M  used = 0.00M  free = 0.00M  (encrypted)";
        let swap = parse_swapusage(line).expect("parse_swapusage");
        assert_eq!(swap.total_bytes, 0);
        assert_eq!(swap.used_bytes, 0);
        assert_eq!(swap.free_bytes, 0);
    }

    #[test]
    fn parse_swapusage_handles_gigabytes() {
        // Tolerate future macOS versions that print "4.00G" instead
        // of "4096.00M" for very large swap files.
        let line = "vm.swapusage: total = 4.00G  used = 0.50G  free = 3.50G  (encrypted)";
        let swap = parse_swapusage(line).expect("parse_swapusage");
        assert_eq!(swap.total_bytes, 4u64 * 1024 * 1024 * 1024);
        assert_eq!(swap.used_bytes, (0.5 * 1024.0 * 1024.0 * 1024.0) as u64);
    }

    #[test]
    fn parse_swapusage_returns_none_for_garbage() {
        assert!(parse_swapusage("vm.swapusage: nope").is_none());
        assert!(parse_swapusage("").is_none());
    }

    #[test]
    fn parse_loadavg_extracts_three_floats() {
        let lav = parse_loadavg("{ 2.59 2.67 2.80 }").expect("parse");
        assert!((lav.one - 2.59).abs() < 1e-9);
        assert!((lav.five - 2.67).abs() < 1e-9);
        assert!((lav.fifteen - 2.80).abs() < 1e-9);
    }

    #[test]
    fn parse_loadavg_tolerates_no_braces() {
        // Defensive — some future sysctl version might drop braces.
        let lav = parse_loadavg("0.10 0.20 0.30").expect("parse");
        assert!((lav.one - 0.10).abs() < 1e-9);
    }

    #[test]
    fn parse_loadavg_returns_none_when_too_few_values() {
        assert!(parse_loadavg("{ 1.0 }").is_none());
        assert!(parse_loadavg("garbage").is_none());
    }

    #[test]
    fn parse_byte_quantity_units_round_trip() {
        assert_eq!(parse_byte_quantity("0.00M"), Some(0));
        assert_eq!(parse_byte_quantity("512.00K"), Some(512 * 1024));
        assert_eq!(parse_byte_quantity("1.00G"), Some(1024 * 1024 * 1024));
        assert_eq!(parse_byte_quantity("100B"), Some(100));
        assert_eq!(parse_byte_quantity("nope"), None);
    }
}
