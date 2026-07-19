import Foundation

switch parseMode(arguments: Array(CommandLine.arguments.dropFirst())) {
case .availability:
    writeAvailability(currentAvailability())
case .capabilities:
    writeCapabilities(currentCapabilities())
case .generate:
    await runGeneration()
case .invalid:
    writeError(.invalidMode)
}

private func runGeneration() async {
    do {
        let request = try decodeRequest(readBoundedStandardInput())
        guard #available(macOS 26.0, *) else {
            throw HelperError.generationFailed
        }
        let session = AppleGenerationSession(instructions: makeInstructions(from: request.messages))
        try await streamGenerate(request: request, session: session) { record in
            try writeRecord(record)
        }
    } catch let error as HelperError {
        writeError(error)
    } catch {
        writeError(.generationFailed)
    }
}

private func writeCapabilities(_ response: CapabilitiesResponse) {
    guard let data = try? encodeCapabilitiesResponse(response) else {
        writeError(.generationFailed)
        return
    }
    FileHandle.standardOutput.write(data)
    FileHandle.standardOutput.write(Data([0x0A]))
}

private func writeAvailability(_ response: AvailabilityResponse) {
    guard let data = try? encodeAvailabilityResponse(response) else {
        writeError(.generationFailed)
        return
    }
    FileHandle.standardOutput.write(data)
    FileHandle.standardOutput.write(Data([0x0A]))
}

private func writeRecord(_ record: OutputRecord) throws {
    let data = try encodeOutputRecord(record)
    FileHandle.standardOutput.write(data)
    FileHandle.standardOutput.write(Data([0x0A]))
}

private func writeError(_ error: HelperError) {
    let record = terminalErrorRecord(for: error)
    if let data = try? encodeOutputRecord(record) {
        FileHandle.standardOutput.write(data)
        FileHandle.standardOutput.write(Data([0x0A]))
    }
    let diagnostic = "plume-apple-model: \(error.code)\n"
    FileHandle.standardError.write(Data(diagnostic.utf8))
}
