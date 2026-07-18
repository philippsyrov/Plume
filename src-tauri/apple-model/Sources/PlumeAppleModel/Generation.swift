import FoundationModels

protocol GenerationSession {
    func stream(
        prompt: String,
        maxOutputTokens: Int,
        onSnapshot: (String) throws -> Void,
    ) async throws
}

func generate(request: GenerationRequest, session: some GenerationSession) async throws -> [OutputRecord] {
    var records: [OutputRecord] = []
    try await streamGenerate(request: request, session: session) { record in
        records.append(record)
    }
    return records
}

func streamGenerate(
    request: GenerationRequest,
    session: some GenerationSession,
    emit: (OutputRecord) throws -> Void,
) async throws {
    try validate(request: request)
    let prompt = makePrompt(from: request.messages)
    var previousSnapshotBytes: [UInt8] = []

    do {
        try await session.stream(prompt: prompt, maxOutputTokens: request.maxOutputTokens) { snapshot in
            let snapshotBytes = Array(snapshot.utf8)
            guard snapshotBytes.starts(with: previousSnapshotBytes) else {
                throw HelperError.invalidSnapshot
            }
            let suffixBytes = snapshotBytes.dropFirst(previousSnapshotBytes.count)
            guard let delta = String(bytes: suffixBytes, encoding: .utf8) else {
                throw HelperError.invalidSnapshot
            }
            previousSnapshotBytes = snapshotBytes
            if !delta.isEmpty {
                let record = OutputRecord(kind: .token, delta: delta, error: nil)
                _ = try encodeOutputRecord(record)
                try emit(record)
            }
        }
    } catch let error as HelperError {
        throw error
    } catch {
        throw HelperError.generationFailed
    }

    let done = OutputRecord(kind: .done, delta: nil, error: nil)
    _ = try encodeOutputRecord(done)
    try emit(done)
}

func makeInstructions(from messages: [ChatMessage]) -> String? {
    let instructions = messages
        .filter { $0.role == .system }
        .map(\.content)
        .joined(separator: "\n")
    return instructions.isEmpty ? nil : instructions
}

func makePrompt(from messages: [ChatMessage]) -> String {
    messages.compactMap { message in
        switch message.role {
        case .system:
            nil
        case .user:
            "User: \(message.content)"
        case .assistant:
            "Assistant: \(message.content)"
        }
    }.joined(separator: "\n\n")
}

@available(macOS 26.0, *)
final class AppleGenerationSession: GenerationSession {
    private let session: LanguageModelSession

    init(instructions: String?) {
        session = LanguageModelSession(
            model: SystemLanguageModel.default,
            instructions: instructions,
        )
    }

    func stream(
        prompt: String,
        maxOutputTokens: Int,
        onSnapshot: (String) throws -> Void,
    ) async throws {
        let responseStream = session.streamResponse(
            to: prompt,
            options: GenerationOptions(maximumResponseTokens: maxOutputTokens),
        )
        for try await snapshot in responseStream {
            try onSnapshot(snapshot.content)
        }
    }
}
