use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use super::apple_foundation::{
    append_stderr_bounded, availability_with, capabilities_with, parse_availability_line,
    parse_capabilities_line, parse_generation_record, AppleAvailabilityReason, AppleCapabilities,
    HelperExit, HelperOutputRecord, HelperPort,
};

#[derive(Clone)]
struct FakeAvailabilityHelper {
    exit: HelperExit,
    calls: Arc<AtomicUsize>,
}

impl FakeAvailabilityHelper {
    fn new(stdout: &str, success: bool) -> Self {
        Self {
            exit: HelperExit {
                stdout: stdout.as_bytes().to_vec(),
                stderr: b"diagnostic".to_vec(),
                success,
            },
            calls: Arc::new(AtomicUsize::new(0)),
        }
    }
}

impl HelperPort for FakeAvailabilityHelper {
    fn availability(&self) -> Result<HelperExit, String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(self.exit.clone())
    }

    fn capabilities(&self) -> Result<HelperExit, String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(self.exit.clone())
    }

    fn start_generation(
        &self,
        _request: super::apple_foundation::AppleGenerationRequest,
    ) -> Result<Box<dyn super::apple_foundation::HelperProcess>, String> {
        panic!("availability tests must not start generation")
    }
}

#[test]
fn parses_bounded_apple_capabilities() {
    let parsed =
        parse_capabilities_line(b"{\"contextSize\":4096,\"exactTokenCountAvailable\":false}\n")
            .expect("capabilities must parse");
    assert_eq!(
        parsed,
        AppleCapabilities {
            context_tokens: 4096,
            exact_token_count: false,
        }
    );
}

#[test]
fn capabilities_reject_unknown_fields_and_invalid_context_sizes() {
    assert!(parse_capabilities_line(
        b"{\"contextSize\":4096,\"exactTokenCountAvailable\":false,\"extra\":1}\n"
    )
    .is_err());
    assert!(
        parse_capabilities_line(b"{\"contextSize\":0,\"exactTokenCountAvailable\":false}\n")
            .is_err()
    );
    assert!(parse_capabilities_line(
        b"{\"contextSize\":1000001,\"exactTokenCountAvailable\":true}\n"
    )
    .is_err());
}

#[test]
fn capabilities_use_the_same_bounded_helper_seam() {
    let helper = FakeAvailabilityHelper::new(
        "{\"contextSize\":8192,\"exactTokenCountAvailable\":true}\n",
        true,
    );
    assert_eq!(
        capabilities_with(&helper).expect("capability helper must parse"),
        AppleCapabilities {
            context_tokens: 8192,
            exact_token_count: true,
        }
    );
    assert_eq!(helper.calls.load(Ordering::SeqCst), 1);
}

#[test]
fn parses_terminal_generation_telemetry_without_weakening_record_shapes() {
    assert_eq!(
        parse_generation_record(b"{\"kind\":\"done\",\"contextSize\":4096,\"promptTokens\":17}")
            .expect("done telemetry must parse"),
        HelperOutputRecord::Done {
            context_size: Some(4096),
            prompt_tokens: Some(17),
        }
    );
    assert!(
        parse_generation_record(b"{\"kind\":\"token\",\"delta\":\"x\",\"contextSize\":4096}")
            .is_err()
    );
    assert!(parse_generation_record(
        b"{\"kind\":\"done\",\"contextSize\":4096,\"promptTokens\":17,\"extra\":true}"
    )
    .is_err());
}

#[test]
fn parses_nominal_apple_availability() {
    let parsed = parse_availability_line(b"{\"available\":true,\"reason\":null,\"detail\":null}\n")
        .expect("available helper response must parse");
    assert!(parsed.available);
    assert_eq!(parsed.reason, None);
    assert_eq!(parsed.detail, None);
}

#[test]
fn maps_every_typed_unavailable_reason() {
    for (wire, expected) in [
        ("os-unsupported", AppleAvailabilityReason::OsUnsupported),
        (
            "device-ineligible",
            AppleAvailabilityReason::DeviceIneligible,
        ),
        (
            "apple-intelligence-disabled",
            AppleAvailabilityReason::AppleIntelligenceDisabled,
        ),
        ("model-not-ready", AppleAvailabilityReason::ModelNotReady),
        ("failed", AppleAvailabilityReason::Failed),
    ] {
        let line =
            format!("{{\"available\":false,\"reason\":\"{wire}\",\"detail\":\"Not ready.\"}}\n");
        let parsed = parse_availability_line(line.as_bytes()).expect("typed reason must parse");
        assert!(!parsed.available);
        assert_eq!(parsed.reason, Some(expected));
    }
}

#[test]
fn availability_rejects_malformed_extra_and_oversized_lines() {
    assert!(parse_availability_line(b"not json\n").is_err());
    assert!(
        parse_availability_line(b"{\"available\":true,\"reason\":null,\"detail\":null}\n{}\n")
            .is_err()
    );
    let oversized = vec![b'x'; super::apple_foundation::MAX_HELPER_LINE_BYTES + 1];
    assert!(parse_availability_line(&oversized).is_err());
}

#[test]
fn unsupported_os_returns_stable_reason_without_spawning_helper() {
    let helper = FakeAvailabilityHelper::new(
        "{\"available\":true,\"reason\":null,\"detail\":null}\n",
        true,
    );
    let availability = availability_with(&helper, false);
    assert_eq!(
        availability.reason,
        Some(AppleAvailabilityReason::OsUnsupported)
    );
    assert_eq!(helper.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn failed_helper_exit_becomes_safe_failed_availability() {
    let helper = FakeAvailabilityHelper::new(
        "{\"available\":true,\"reason\":null,\"detail\":null}\n",
        false,
    );
    let availability = availability_with(&helper, true);
    assert!(!availability.available);
    assert_eq!(availability.reason, Some(AppleAvailabilityReason::Failed));
    assert!(
        availability.detail.is_none(),
        "stderr must not reach the wire"
    );
}

#[test]
fn stderr_capture_is_hard_bounded() {
    let mut captured = Vec::new();
    append_stderr_bounded(
        &mut captured,
        &vec![b'x'; super::apple_foundation::MAX_HELPER_LINE_BYTES],
    );
    append_stderr_bounded(&mut captured, b"later output");
    assert_eq!(captured.len(), 32 * 1024);
}
