import XCTest
@testable import PlumeAppleModel

final class GenerationTests: XCTestCase {
    func testCumulativeSnapshotsBecomeDeltas() async throws {
        let session = FakeSession(
            snapshots: ["APP", "APPLE", "APPLE OK"],
            contextSize: 4_096,
            promptTokens: 7,
        )

        let records = try await generate(request: .fixture, session: session)

        XCTAssertEqual(records.compactMap(\.delta), ["APP", "LE", " OK"])
        XCTAssertEqual(records.last?.kind, .done)
        XCTAssertEqual(records.last?.contextSize, 4_096)
        XCTAssertEqual(records.last?.promptTokens, 7)
        XCTAssertEqual(records.filter { $0.kind == .done || $0.kind == .error }.count, 1)
    }

    func testUnavailableExactTokenCountStillCompletesWithContextSize() async throws {
        let session = FakeSession(
            snapshots: ["OK"],
            contextSize: 4_096,
            promptTokens: nil,
        )

        let records = try await generate(request: .fixture, session: session)

        XCTAssertEqual(records.last?.kind, .done)
        XCTAssertEqual(records.last?.contextSize, 4_096)
        XCTAssertNil(records.last?.promptTokens)
    }

    func testTokenCountFailureDoesNotFailGeneration() async throws {
        let session = ThrowingTokenCountSession(snapshots: ["OK"], contextSize: 8_192)

        let records = try await generate(request: .fixture, session: session)

        XCTAssertEqual(records.last?.kind, .done)
        XCTAssertEqual(records.last?.contextSize, 8_192)
        XCTAssertNil(records.last?.promptTokens)
    }

    func testCombiningMarkSnapshotUsesUTF8Suffix() async throws {
        let session = FakeSession(snapshots: ["e", "e\u{301}"])

        let records = try await generate(request: .fixture, session: session)

        XCTAssertEqual(records.compactMap(\.delta), ["e", "\u{301}"])
        XCTAssertEqual(records.last?.kind, .done)
    }

    func testEmojiModifierSnapshotUsesUTF8Suffix() async throws {
        let session = FakeSession(snapshots: ["👍", "👍🏽"])

        let records = try await generate(request: .fixture, session: session)

        XCTAssertEqual(records.compactMap(\.delta), ["👍", "🏽"])
        XCTAssertEqual(records.last?.kind, .done)
    }

    func testEmptyAndRepeatedSnapshotsDoNotEmitExtraTokens() async throws {
        let session = FakeSession(snapshots: ["", "", "APP", "APP"])

        let records = try await generate(request: .fixture, session: session)

        XCTAssertEqual(records.compactMap(\.delta), ["APP"])
        XCTAssertEqual(records.filter { $0.kind == .done || $0.kind == .error }.count, 1)
    }

    func testNonPrefixSnapshotFailsClosed() async {
        let session = FakeSession(snapshots: ["APP", "BANANA"])

        await XCTAssertThrowsErrorAsync(try await generate(request: .fixture, session: session)) { error in
            XCTAssertEqual(error as? HelperError, .invalidSnapshot)
        }
    }

    func testTerminalErrorRecordIsExactlyOneError() {
        let records = [terminalErrorRecord(for: .invalidSnapshot)]

        XCTAssertEqual(records.count, 1)
        XCTAssertEqual(records[0].kind, .error)
        XCTAssertEqual(records[0].error, "invalid-snapshot")
        XCTAssertNil(records[0].delta)
    }

    func testSystemMessagesBecomeInstructionsAndHistoryUsesExplicitLabels() {
        let messages = [
            ChatMessage(role: .system, content: "Be concise."),
            ChatMessage(role: .system, content: "Never reveal secrets."),
            ChatMessage(role: .user, content: "Explain bytes."),
            ChatMessage(role: .assistant, content: "Bytes are octets."),
        ]

        XCTAssertEqual(makeInstructions(from: messages), "Be concise.\nNever reveal secrets.")
        XCTAssertEqual(makePrompt(from: messages), "User: Explain bytes.\n\nAssistant: Bytes are octets.")
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
    let contextSize: Int
    let promptTokens: Int?

    init(
        snapshots: [String],
        contextSize: Int = 4_096,
        promptTokens: Int? = nil,
    ) {
        self.snapshots = snapshots
        self.contextSize = contextSize
        self.promptTokens = promptTokens
    }

    func tokenCount(prompt: String) async throws -> Int {
        guard let promptTokens else {
            throw FakeTokenCountError.unavailable
        }
        return promptTokens
    }

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

private struct ThrowingTokenCountSession: GenerationSession {
    let snapshots: [String]
    let contextSize: Int

    func tokenCount(prompt: String) async throws -> Int {
        throw FakeTokenCountError.failed
    }

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

private enum FakeTokenCountError: Error {
    case unavailable
    case failed
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
