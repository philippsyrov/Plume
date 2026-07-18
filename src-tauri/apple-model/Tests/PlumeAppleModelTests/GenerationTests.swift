import XCTest
@testable import PlumeAppleModel

final class GenerationTests: XCTestCase {
    func testCumulativeSnapshotsBecomeDeltas() async throws {
        let session = FakeSession(snapshots: ["APP", "APPLE", "APPLE OK"])

        let records = try await generate(request: .fixture, session: session)

        XCTAssertEqual(records.compactMap(\.delta), ["APP", "LE", " OK"])
        XCTAssertEqual(records.last?.kind, .done)
        XCTAssertEqual(records.filter { $0.kind == .done || $0.kind == .error }.count, 1)
    }

    func testMessageOver256KiBIsRejected() async {
        let request = GenerationRequest(
            requestId: "test",
            messages: [ChatMessage(role: .user, content: String(repeating: "a", count: maximumMessageBytes + 1))],
            maxOutputTokens: 32,
        )

        await XCTAssertThrowsErrorAsync(try await generate(request: request, session: FakeSession(snapshots: []))) { error in
            XCTAssertEqual(error as? HelperError, .messageTooLarge)
        }
    }

    func testOutputTokenLimitMustBeWithinOneAnd4096() async {
        let request = GenerationRequest(
            requestId: "test",
            messages: [ChatMessage(role: .user, content: "Say hello")],
            maxOutputTokens: 0,
        )

        await XCTAssertThrowsErrorAsync(try await generate(request: request, session: FakeSession(snapshots: []))) { error in
            XCTAssertEqual(error as? HelperError, .invalidOutputTokenLimit)
        }
    }

    func testMoreThan128MessagesIsRejected() async {
        let request = GenerationRequest(
            requestId: "test",
            messages: (0 ... maximumMessageCount).map { _ in ChatMessage(role: .user, content: "hi") },
            maxOutputTokens: 32,
        )

        await XCTAssertThrowsErrorAsync(try await generate(request: request, session: FakeSession(snapshots: []))) { error in
            XCTAssertEqual(error as? HelperError, .tooManyMessages)
        }
    }

    func testSnapshotThatCannotFitOneOutputRecordIsRejected() async {
        let oversizedSnapshot = String(repeating: "a", count: maximumOutputRecordBytes)

        await XCTAssertThrowsErrorAsync(
            try await generate(request: .fixture, session: FakeSession(snapshots: [oversizedSnapshot])),
        ) { error in
            XCTAssertEqual(error as? HelperError, .outputTooLarge)
        }
    }
}

private struct FakeSession: GenerationSession {
    let snapshots: [String]

    func stream(
        prompt: String,
        maxOutputTokens: Int,
        onSnapshot: (String) throws -> Void,
    ) async throws {
        for snapshot in snapshots {
            try onSnapshot(snapshot)
        }
    }
}

private extension GenerationRequest {
    static var fixture: GenerationRequest {
        GenerationRequest(
            requestId: "test",
            messages: [ChatMessage(role: .user, content: "Say hello")],
            maxOutputTokens: 32,
        )
    }
}

private func XCTAssertThrowsErrorAsync<T>(
    _ expression: @autoclosure () async throws -> T,
    _ errorHandler: (Error) -> Void,
) async {
    do {
        _ = try await expression()
        XCTFail("Expected expression to throw")
    } catch {
        errorHandler(error)
    }
}
