//! Cautious model-fit estimator.
//!
//! Given a model's parameter count, quantization label, and the host's
//! physical-memory size, this module produces a green / amber / red
//! verdict plus the rough byte estimate that drove it. We err toward
//! amber/red because the cost of telling a user "go ahead" on a model
//! that thrashes their swap is much worse than the cost of telling
//! them "this is going to be tight".
//!
//! The estimate covers three components:
//!
//!   1. **Weights** — `parameter_count × bytes_per_param(quant)`. The
//!      bytes-per-param table is conservative: where the GGUF spec
//!      lists a range, we pick the high end.
//!   2. **KV cache + activations** — a fraction of the weight cost
//!      that grows with context length. Without per-architecture
//!      head/layer math we approximate as 15% of weights, which lines
//:      up reasonably with what GGUF runners observe at 4k–8k context.
//!   3. **Host-side overhead** — Plume itself, the WebView, the OS
//!      kernel and background daemons, the runtime daemon (Ollama),
//!      plus a comfort buffer. We reserve a flat 4 GiB and never go
//!      lower; on a 16 GiB Mac the model genuinely cannot use all
//!      16 GiB without paging the system out.
//!
//! Thresholds against host RAM:
//!
//!   - `total < 0.40 * ram` → comfortable (green).
//!   - `total < 0.70 * ram` → tight (amber).
//!   - otherwise → too-large (red).
//!   - host RAM unknown OR parameter count missing → unknown.
//!
//! These numbers are heuristics, not promises. The estimator is the
//! UI's first-pass honesty signal; the real benchmark is the user
//! pressing "load" and watching memory pressure — which the chat slice
//! will surface.

use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FitEstimate {
    pub state: FitState,
    /// Estimated peak working-set in bytes, including weights, a KV
    /// cache approximation, and a fixed host-side overhead reserve.
    /// `None` when we lack the inputs (no parameter count, no host
    /// RAM signal, or quantization we cannot map).
    pub estimated_ram_bytes: Option<u64>,
    /// Host RAM in bytes when the platform reports it.
    pub machine_ram_bytes: Option<u64>,
    /// One-sentence human-readable explanation. Surfaced verbatim in
    /// the UI so the verdict is auditable.
    pub rationale: String,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum FitState {
    /// Estimated working set is well under host RAM.
    Comfortable,
    /// Estimated working set is plausible but leaves little headroom.
    Tight,
    /// Estimated working set exceeds what the host can serve without
    /// hurting performance.
    TooLarge,
    /// Missing inputs (no parameter count, no host RAM, unknown quant).
    /// UI must NOT render this as a pass.
    Unknown,
}

/// Reserved overhead for Plume, the WebView, the OS, and the runtime
/// daemon itself. Picked conservatively — on a 16 GiB Mac the typical
/// idle reserve is 3–5 GiB once Plume + Ollama are both running.
pub const HOST_OVERHEAD_BYTES: u64 = 4 * 1024 * 1024 * 1024;

/// Fraction of weight bytes reserved for KV cache + activations.
/// Empirically lines up with 4k–8k context on llama-family GGUF runs.
const KV_OVERHEAD_RATIO: f64 = 0.15;

const COMFORTABLE_RATIO: f64 = 0.40;
const TIGHT_RATIO: f64 = 0.70;

/// Compute the fit verdict.
///
/// All inputs are optional because the upstream probe might omit any
/// of them. Missing parameter count or host RAM short-circuits to
/// `Unknown` so we never hand back a confident verdict drawn from
/// guesses.
pub fn estimate_fit(
    parameter_count: Option<u64>,
    quantization: Option<&str>,
    machine_ram_bytes: Option<u64>,
) -> FitEstimate {
    let Some(param_count) = parameter_count else {
        return FitEstimate {
            state: FitState::Unknown,
            estimated_ram_bytes: None,
            machine_ram_bytes,
            rationale: "Plume could not read the parameter count from this model.".into(),
        };
    };
    let Some(bytes_per_param) = bytes_per_param_for(quantization) else {
        return FitEstimate {
            state: FitState::Unknown,
            estimated_ram_bytes: None,
            machine_ram_bytes,
            rationale: format!(
                "Quantization {} is not in Plume's fit table yet.",
                quantization.unwrap_or("(missing)"),
            ),
        };
    };

    // Compute estimated working set in f64 to avoid intermediate
    // overflow on a 70B fp16 model (~140 GiB). The cast back to u64
    // at the end is bounded by the value space and saturates if a
    // pathological input slips through.
    let weights = param_count as f64 * bytes_per_param;
    let kv = weights * KV_OVERHEAD_RATIO;
    let total = weights + kv + HOST_OVERHEAD_BYTES as f64;
    let estimated_bytes = total.min(u64::MAX as f64) as u64;

    let Some(ram_bytes) = machine_ram_bytes else {
        return FitEstimate {
            state: FitState::Unknown,
            estimated_ram_bytes: Some(estimated_bytes),
            machine_ram_bytes: None,
            rationale: format!(
                "Estimated working set is about {}, but Plume could not read the host's physical memory.",
                format_bytes(estimated_bytes),
            ),
        };
    };

    let ram_f = ram_bytes as f64;
    let (state, headline) = if total < ram_f * COMFORTABLE_RATIO {
        (FitState::Comfortable, "comfortable")
    } else if total < ram_f * TIGHT_RATIO {
        (FitState::Tight, "tight")
    } else {
        (FitState::TooLarge, "likely too large")
    };

    let rationale = format!(
        "Estimated working set is about {} against {} of host memory — {}.",
        format_bytes(estimated_bytes),
        format_bytes(ram_bytes),
        headline,
    );

    FitEstimate {
        state,
        estimated_ram_bytes: Some(estimated_bytes),
        machine_ram_bytes: Some(ram_bytes),
        rationale,
    }
}

/// Bytes-per-parameter for the quantization labels Ollama returns in
/// `details.quantization_level`. Values are the conservative end of
/// the published GGUF averages; if a label is not in this table the
/// estimator gives up rather than guess.
///
/// References: GGUF quant docs, ggml-quants.h, ggerganov/llama.cpp.
pub fn bytes_per_param_for(label: Option<&str>) -> Option<f64> {
    let q = label?.to_ascii_uppercase();
    // Strip common prefixes so "Q4_K_M" and "q4_k_m" both match.
    let key = q.trim();
    Some(match key {
        // Full precision and standard floats.
        "F32" | "FP32" => 4.0,
        "F16" | "FP16" | "BF16" => 2.0,
        // 8-bit families.
        "Q8_0" | "Q8_1" | "Q8_K" => 1.1,
        // 6-bit family.
        "Q6_K" => 0.82,
        // 5-bit families.
        "Q5_0" | "Q5_1" => 0.72,
        "Q5_K_S" | "Q5_K_M" | "Q5_K" => 0.7,
        // 4-bit families. The K variants pack ~0.56 bpw; the older
        // Q4_0/Q4_1 are slightly heavier. We round both up.
        "Q4_0" | "Q4_1" => 0.6,
        "Q4_K_S" | "Q4_K_M" | "Q4_K" => 0.58,
        // 3-bit family. Quality varies wildly; we still report a
        // working-set estimate so the UI can warn it is tight rather
        // than refuse to estimate.
        "Q3_K_S" | "Q3_K_M" | "Q3_K_L" | "Q3_K" => 0.45,
        // 2-bit. Same caveat as 3-bit.
        "Q2_K" => 0.35,
        _ => return None,
    })
}

/// Render a byte count as a short human-readable string. Lives here
/// (not in a shared util) because the rationale strings round-trip
/// through serde and we want the formatting to be stable.
fn format_bytes(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;
    let b = bytes as f64;
    if b >= GIB {
        format!("{:.1} GiB", b / GIB)
    } else if b >= MIB {
        format!("{:.0} MiB", b / MIB)
    } else if b >= KIB {
        format!("{:.0} KiB", b / KIB)
    } else {
        format!("{bytes} B")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 8 B Q4_0 model on a 16 GiB Mac: classic Gemma / Llama 3 8B
    /// territory. We expect the verdict to land in "tight" — it
    /// physically fits but leaves the user with little headroom.
    #[test]
    fn estimates_an_8b_q4_as_tight_on_16gb() {
        let est = estimate_fit(
            Some(8_030_261_248),
            Some("Q4_0"),
            Some(16 * 1024 * 1024 * 1024),
        );
        assert_eq!(est.state, FitState::Tight);
        assert!(est.estimated_ram_bytes.is_some());
    }

    /// 2 B Q4 fits comfortably on a 16 GiB Mac.
    #[test]
    fn estimates_a_small_model_as_comfortable() {
        let est = estimate_fit(
            Some(2_500_000_000),
            Some("Q4_K_M"),
            Some(16 * 1024 * 1024 * 1024),
        );
        assert_eq!(est.state, FitState::Comfortable);
    }

    /// 35 B Q4 is workstation-class on a 16 GiB Mac — must come back
    /// red.
    #[test]
    fn estimates_a_35b_q4_as_too_large_on_16gb() {
        let est = estimate_fit(
            Some(35_000_000_000),
            Some("Q4_K_M"),
            Some(16 * 1024 * 1024 * 1024),
        );
        assert_eq!(est.state, FitState::TooLarge);
    }

    /// 14 B Q4 on a 64 GiB workstation is comfortable.
    #[test]
    fn estimates_a_14b_q4_as_comfortable_on_64gb() {
        let est = estimate_fit(
            Some(14_000_000_000),
            Some("Q4_K_M"),
            Some(64 * 1024 * 1024 * 1024),
        );
        assert_eq!(est.state, FitState::Comfortable);
    }

    /// Missing host RAM must hand back Unknown — never assume.
    #[test]
    fn missing_host_ram_returns_unknown() {
        let est = estimate_fit(Some(8_000_000_000), Some("Q4_0"), None);
        assert_eq!(est.state, FitState::Unknown);
        // We still surface the estimated bytes so the UI can show the
        // model size even without a verdict.
        assert!(est.estimated_ram_bytes.is_some());
    }

    /// Missing parameter count short-circuits to Unknown.
    #[test]
    fn missing_parameter_count_returns_unknown() {
        let est = estimate_fit(None, Some("Q4_0"), Some(16 * 1024 * 1024 * 1024));
        assert_eq!(est.state, FitState::Unknown);
        assert!(est.estimated_ram_bytes.is_none());
    }

    /// Quantization label we have not mapped yet returns Unknown
    /// instead of guessing.
    #[test]
    fn unknown_quantization_returns_unknown() {
        let est = estimate_fit(
            Some(8_000_000_000),
            Some("IQ2_XXS"),
            Some(16 * 1024 * 1024 * 1024),
        );
        assert_eq!(est.state, FitState::Unknown);
    }

    /// FP16 doubles the byte cost vs Q4 — a 7B fp16 must be too large
    /// on a 16 GiB Mac.
    #[test]
    fn fp16_is_heavier_than_q4() {
        let est = estimate_fit(
            Some(7_000_000_000),
            Some("F16"),
            Some(16 * 1024 * 1024 * 1024),
        );
        assert_eq!(est.state, FitState::TooLarge);
    }

    #[test]
    fn bytes_per_param_table_covers_common_labels() {
        // Spot check: every label here must be in the table. This is
        // a guard against accidental table edits that silently break
        // the most common models.
        for label in &["Q4_0", "Q4_K_M", "Q5_K_M", "Q6_K", "Q8_0", "F16"] {
            assert!(
                bytes_per_param_for(Some(label)).is_some(),
                "label {label} dropped out of fit table"
            );
        }
    }
}
