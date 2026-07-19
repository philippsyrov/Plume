import FoundationModels
import XCTest
@testable import PlumeAppleModel

final class AvailabilityTests: XCTestCase {
    func testCapabilityProjectionPreservesConservativeMacOS26Shape() {
        let response = capabilities(
            from: FakeCapabilitySource(
                contextSize: 4_096,
                exactTokenCountAvailable: false,
            ),
        )

        XCTAssertEqual(response.contextSize, 4_096)
        XCTAssertFalse(response.exactTokenCountAvailable)
    }

    func testCapabilityProjectionAllowsFutureLargerContext() {
        let response = capabilities(
            from: FakeCapabilitySource(
                contextSize: 8_192,
                exactTokenCountAvailable: true,
            ),
        )

        XCTAssertEqual(response.contextSize, 8_192)
        XCTAssertTrue(response.exactTokenCountAvailable)
    }

    func testAppleIntelligenceDisabledIsAnUnavailableReason() {
        XCTAssertEqual(
            mapAvailability(.unavailable(.appleIntelligenceNotEnabled)).reason,
            .appleIntelligenceDisabled,
        )
    }

    func testDeviceIneligibleIsAnUnavailableReason() {
        XCTAssertEqual(
            mapAvailability(.unavailable(.deviceNotEligible)).reason,
            .deviceIneligible,
        )
    }

    func testModelNotReadyIsAnUnavailableReason() {
        XCTAssertEqual(
            mapAvailability(.unavailable(.modelNotReady)).reason,
            .modelNotReady,
        )
    }

    func testAvailableModelHasNoUnavailableReason() {
        let response = mapAvailability(.available)

        XCTAssertTrue(response.available)
        XCTAssertNil(response.reason)
    }
}

private struct FakeCapabilitySource: ModelCapabilitySource {
    let contextSize: Int
    let exactTokenCountAvailable: Bool
}
