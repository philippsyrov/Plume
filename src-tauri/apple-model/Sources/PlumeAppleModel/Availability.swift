import FoundationModels

func mapAvailability(_ availability: SystemLanguageModel.Availability) -> AvailabilityResponse {
    switch availability {
    case .available:
        AvailabilityResponse(available: true, reason: nil, detail: nil)
    case let .unavailable(reason):
        switch reason {
        case .deviceNotEligible:
            AvailabilityResponse(available: false, reason: .deviceIneligible, detail: "This Mac cannot use the Apple on-device model.")
        case .appleIntelligenceNotEnabled:
            AvailabilityResponse(available: false, reason: .appleIntelligenceDisabled, detail: "Apple Intelligence is turned off on this Mac.")
        case .modelNotReady:
            AvailabilityResponse(available: false, reason: .modelNotReady, detail: "The Apple on-device model is not ready yet.")
        @unknown default:
            AvailabilityResponse(available: false, reason: .failed, detail: "The Apple on-device model status is not recognized.")
        }
    @unknown default:
        AvailabilityResponse(available: false, reason: .failed, detail: "The Apple on-device model status is not recognized.")
    }
}

func currentAvailability() -> AvailabilityResponse {
    guard #available(macOS 26.0, *) else {
        return AvailabilityResponse(
            available: false,
            reason: .osUnsupported,
            detail: "This macOS version does not support the Apple on-device model.",
        )
    }
    return mapAvailability(SystemLanguageModel.default.availability)
}
