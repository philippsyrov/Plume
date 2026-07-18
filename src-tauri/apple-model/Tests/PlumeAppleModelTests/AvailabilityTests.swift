import FoundationModels
import XCTest
@testable import PlumeAppleModel

final class AvailabilityTests: XCTestCase {
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
