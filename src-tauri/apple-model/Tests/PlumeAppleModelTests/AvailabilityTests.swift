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

    func testAvailableModelHasNoUnavailableReason() {
        let response = mapAvailability(.available)

        XCTAssertTrue(response.available)
        XCTAssertNil(response.reason)
    }
}
