import Foundation

let maximumRequestBytes = 1_048_576
let maximumMessageBytes = 262_144
let maximumMessageCount = 128
let maximumOutputRecordBytes = 1_048_576
let minimumOutputTokens = 1
let maximumOutputTokens = 4_096

enum ChatRole: String, Codable, Equatable {
    case system
    case user
    case assistant
}

struct ChatMessage: Codable, Equatable {
    let role: ChatRole
    let content: String
}

struct GenerationRequest: Codable, Equatable {
    let requestId: String
    let messages: [ChatMessage]
    let maxOutputTokens: Int
}

enum OutputKind: String, Codable, Equatable {
    case token
    case done
    case error
}

struct OutputRecord: Codable, Equatable {
    let kind: OutputKind
    let delta: String?
    let error: String?
}

enum AvailabilityReason: String, Codable, Equatable {
    case osUnsupported = "os-unsupported"
    case deviceIneligible = "device-ineligible"
    case appleIntelligenceDisabled = "apple-intelligence-disabled"
    case modelNotReady = "model-not-ready"
    case failed
}

struct AvailabilityResponse: Codable, Equatable {
    let available: Bool
    let reason: AvailabilityReason?
    let detail: String?
}

enum HelperError: Error, Equatable {
    case invalidMode
    case requestTooLarge
    case invalidRequest
    case tooManyMessages
    case messageTooLarge
    case invalidOutputTokenLimit
    case outputTooLarge
    case invalidSnapshot
    case generationFailed

    var code: String {
        switch self {
        case .invalidMode: "invalid-mode"
        case .requestTooLarge: "request-too-large"
        case .invalidRequest: "invalid-request"
        case .tooManyMessages: "too-many-messages"
        case .messageTooLarge: "message-too-large"
        case .invalidOutputTokenLimit: "invalid-output-token-limit"
        case .outputTooLarge: "output-too-large"
        case .invalidSnapshot: "invalid-snapshot"
        case .generationFailed: "generation-failed"
        }
    }
}

enum HelperMode {
    case availability
    case generate
    case invalid
}

func parseMode(arguments: [String]) -> HelperMode {
    guard arguments.count == 1 else {
        return .invalid
    }
    switch arguments[0] {
    case "availability": return .availability
    case "generate": return .generate
    default: return .invalid
    }
}

func decodeRequest(_ data: Data) throws -> GenerationRequest {
    guard data.count <= maximumRequestBytes else {
        throw HelperError.requestTooLarge
    }
    do {
        let request = try JSONDecoder().decode(GenerationRequest.self, from: data)
        try validate(request: request)
        return request
    } catch let error as HelperError {
        throw error
    } catch {
        throw HelperError.invalidRequest
    }
}

func validate(request: GenerationRequest) throws {
    guard request.messages.count <= maximumMessageCount else {
        throw HelperError.tooManyMessages
    }
    guard (minimumOutputTokens ... maximumOutputTokens).contains(request.maxOutputTokens) else {
        throw HelperError.invalidOutputTokenLimit
    }
    guard request.messages.allSatisfy({ $0.content.lengthOfBytes(using: .utf8) <= maximumMessageBytes }) else {
        throw HelperError.messageTooLarge
    }
}

func encodeOutputRecord(_ record: OutputRecord) throws -> Data {
    let data = try JSONEncoder().encode(record)
    guard data.count <= maximumOutputRecordBytes else {
        throw HelperError.outputTooLarge
    }
    return data
}

func encodeAvailabilityResponse(_ response: AvailabilityResponse) throws -> Data {
    let data = try JSONEncoder().encode(response)
    guard data.count <= maximumOutputRecordBytes else {
        throw HelperError.outputTooLarge
    }
    return data
}

func readBoundedStandardInput() throws -> Data {
    let input = FileHandle.standardInput
    var data = Data()

    while true {
        let remaining = maximumRequestBytes + 1 - data.count
        guard remaining > 0 else {
            throw HelperError.requestTooLarge
        }
        guard let chunk = try input.read(upToCount: min(65_536, remaining)), !chunk.isEmpty else {
            return data
        }
        data.append(chunk)
        if data.count > maximumRequestBytes {
            throw HelperError.requestTooLarge
        }
    }
}
