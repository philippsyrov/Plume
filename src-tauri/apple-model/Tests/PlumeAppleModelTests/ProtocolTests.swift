import Foundation
import XCTest
@testable import PlumeAppleModel

final class ProtocolTests: XCTestCase {
    func testRequestUsesCamelCaseJSON() throws {
        let request = GenerationRequest(
            requestId: "test",
            messages: [ChatMessage(role: .user, content: "Say hello")],
            maxOutputTokens: 32,
        )

        let data = try JSONEncoder().encode(request)
        let object = try JSONSerialization.jsonObject(with: data) as? [String: Any]

        let requestID = object?["requestId"] as? String
        let outputLimit = object?["maxOutputTokens"] as? Int
        XCTAssertEqual(requestID, request.requestId)
        XCTAssertEqual(outputLimit, request.maxOutputTokens)
        XCTAssertNil(object?["request_id"])
        XCTAssertNil(object?["max_output_tokens"])
    }

    func testRequestOverOneMiBIsRejectedBeforeDecoding() {
        let data = Data(repeating: 0x41, count: maximumRequestBytes + 1)

        XCTAssertThrowsError(try decodeRequest(data)) { error in
            XCTAssertEqual(error as? HelperError, .requestTooLarge)
        }
    }

    func testUnknownModeIsRejectedWithoutEchoingArguments() {
        XCTAssertEqual(parseMode(arguments: ["surprise"]), .invalid)
    }

    func testCapabilitiesModeIsAccepted() {
        XCTAssertEqual(parseMode(arguments: ["capabilities"]), .capabilities)
    }

    func testCapabilitiesResponseUsesBoundedCamelCaseJSON() throws {
        let response = CapabilitiesResponse(
            contextSize: 4_096,
            exactTokenCountAvailable: false,
        )

        let data = try encodeCapabilitiesResponse(response)
        let object = try JSONSerialization.jsonObject(with: data) as? [String: Any]

        XCTAssertLessThanOrEqual(data.count, maximumOutputRecordBytes)
        XCTAssertEqual(object?["contextSize"] as? Int, 4_096)
        XCTAssertEqual(object?["exactTokenCountAvailable"] as? Bool, false)
        XCTAssertNil(object?["context_size"])
    }
}
