// apple-speech-helper
//
// Sidecar process wrapping Apple's on-device SpeechAnalyzer/SpeechTranscriber
// (macOS 26+) behind the same newline-delimited JSON protocol used by
// llama-helper: one JSON object per line on stdin, one JSON object per line
// on stdout. Requests are handled one at a time, matching how Meetily's
// TranscriptionProvider trait calls a provider per VAD-detected speech
// segment rather than as a continuous stream.

import Foundation
@preconcurrency import AVFoundation
import Speech
import os

// ============================================================================
// Protocol
// ============================================================================

private struct Request: Decodable {
    let type: String
    let locale: String?
    let audioBase64: String?
    let sampleRate: Double?

    private enum CodingKeys: String, CodingKey {
        case type, locale
        case audioBase64 = "audio_base64"
        case sampleRate = "sample_rate"
    }
}

private func send(_ object: [String: Any]) {
    guard let data = try? JSONSerialization.data(withJSONObject: object) else { return }
    FileHandle.standardOutput.write(data)
    FileHandle.standardOutput.write("\n".data(using: .utf8)!)
}

private func sendError(_ message: String) {
    send(["type": "error", "message": message])
}

private enum HelperError: Error, CustomStringConvertible {
    case notReady
    case invalidAudio
    case unsupportedOS
    case timedOut

    var description: String {
        switch self {
        case .notReady: return "Locale assets are not installed; send an init request first"
        case .invalidAudio: return "Could not build an audio buffer from the given samples"
        case .unsupportedOS: return "Apple's on-device speech-to-text requires macOS 26 or later"
        case .timedOut: return "Transcription timed out"
        }
    }
}

// ============================================================================
// Audio buffer conversion (adapted from Apple's SpeechAnalyzer sample code)
// ============================================================================

@available(macOS 26.0, *)
private final class BufferConverter {
    private var converter: AVAudioConverter?

    func convertBuffer(_ buffer: AVAudioPCMBuffer, to format: AVAudioFormat) throws -> AVAudioPCMBuffer {
        let inputFormat = buffer.format
        guard inputFormat != format else { return buffer }

        if converter == nil || converter?.outputFormat != format {
            converter = AVAudioConverter(from: inputFormat, to: format)
            converter?.primeMethod = .none
        }
        guard let converter else { throw HelperError.invalidAudio }

        let ratio = converter.outputFormat.sampleRate / converter.inputFormat.sampleRate
        let capacity = AVAudioFrameCount((Double(buffer.frameLength) * ratio).rounded(.up))
        guard let output = AVAudioPCMBuffer(pcmFormat: converter.outputFormat, frameCapacity: capacity) else {
            throw HelperError.invalidAudio
        }

        let consumedLock = OSAllocatedUnfairLock(initialState: false)
        var conversionError: NSError?
        let status = converter.convert(to: output, error: &conversionError) { _, inputStatus in
            let alreadyConsumed = consumedLock.withLock { consumed in
                let was = consumed
                consumed = true
                return was
            }
            inputStatus.pointee = alreadyConsumed ? .noDataNow : .haveData
            return alreadyConsumed ? nil : buffer
        }

        guard status != .error else { throw HelperError.invalidAudio }
        return output
    }
}

// ============================================================================
// Timeout guard
//
// ponytail: SpeechTranscriber.results isn't documented on how promptly it
// completes after finalizeAndFinishThroughEndOfInput(); bound the wait so one
// stuck segment can't wedge the sidecar. Raise this if long segments start
// timing out in practice.
// ============================================================================

@available(macOS 26.0, *)
private func withTimeout<T: Sendable>(
    seconds: Double,
    operation: @escaping @Sendable () async throws -> T
) async throws -> T {
    try await withThrowingTaskGroup(of: T.self) { group in
        group.addTask { try await operation() }
        group.addTask {
            try await Task.sleep(nanoseconds: UInt64(seconds * 1_000_000_000))
            throw HelperError.timedOut
        }
        defer { group.cancelAll() }
        return try await group.next()!
    }
}

// ============================================================================
// Speech session: one per process, reused across transcribe requests
// ============================================================================

@available(macOS 26.0, *)
private actor SpeechSession {
    private var locale = Locale(identifier: "en-US")
    private var ready = false

    func ensureReady(localeIdentifier: String) async throws -> String {
        let requested = Locale(identifier: localeIdentifier)
        let transcriber = SpeechTranscriber(locale: requested, preset: .transcription)

        let supported = await SpeechTranscriber.supportedLocales
        guard !supported.isEmpty else {
            throw HelperError.notReady
        }
        let resolved = supported.first { $0.identifier(.bcp47) == requested.identifier(.bcp47) }
            ?? supported.first { $0.identifier(.bcp47).hasPrefix("en") }
            ?? supported[0]

        let resolvedTranscriber = resolved == requested
            ? transcriber
            : SpeechTranscriber(locale: resolved, preset: .transcription)

        if let installer = try await AssetInventory.assetInstallationRequest(supporting: [resolvedTranscriber]) {
            try await installer.downloadAndInstall()
        }

        let reserved = await AssetInventory.reservedLocales
        if !reserved.contains(where: { $0.identifier(.bcp47) == resolved.identifier(.bcp47) }) {
            try await AssetInventory.reserve(locale: resolved)
        }

        locale = resolved
        ready = true
        return resolved.identifier(.bcp47)
    }

    func transcribe(samples: [Float], sampleRate: Double) async throws -> String {
        guard ready else { throw HelperError.notReady }
        guard samples.count >= 160 else { return "" } // shorter than 10ms @ 16kHz: nothing to transcribe

        let transcriber = SpeechTranscriber(locale: locale, preset: .transcription)
        let analyzer = SpeechAnalyzer(modules: [transcriber])

        let buffer = try Self.makeBuffer(samples: samples, sampleRate: sampleRate)
        guard let analyzerFormat = await SpeechAnalyzer.bestAvailableAudioFormat(compatibleWith: [transcriber]) else {
            throw HelperError.invalidAudio
        }
        let converted = analyzerFormat == buffer.format
            ? buffer
            : try BufferConverter().convertBuffer(buffer, to: analyzerFormat)

        let (stream, continuation) = AsyncStream<AnalyzerInput>.makeStream()
        continuation.yield(AnalyzerInput(buffer: converted))
        continuation.finish()

        let resultsTask = Task<String, Error> {
            var text = ""
            for try await result in transcriber.results where result.isFinal {
                text += String(result.text.characters)
            }
            return text
        }

        try await analyzer.start(inputSequence: stream)
        try await analyzer.finalizeAndFinishThroughEndOfInput()

        do {
            return try await withTimeout(seconds: 30) { try await resultsTask.value }
        } catch {
            resultsTask.cancel()
            throw error
        }
    }

    private static func makeBuffer(samples: [Float], sampleRate: Double) throws -> AVAudioPCMBuffer {
        guard let format = AVAudioFormat(standardFormatWithSampleRate: sampleRate, channels: 1),
              let buffer = AVAudioPCMBuffer(pcmFormat: format, frameCapacity: AVAudioFrameCount(samples.count)),
              let channelData = buffer.floatChannelData
        else {
            throw HelperError.invalidAudio
        }
        buffer.frameLength = buffer.frameCapacity
        samples.withUnsafeBufferPointer { source in
            channelData[0].update(from: source.baseAddress!, count: samples.count)
        }
        return buffer
    }
}

// ============================================================================
// Self-check: `apple-speech-helper --selftest`
//
// Exercises the JSON protocol parsing (the "loop + parser" logic that has no
// runtime OS-version dependency) without needing macOS 26 or a live
// SpeechAnalyzer session. Run after touching the Request/response shapes.
// ============================================================================

if CommandLine.arguments.contains("--selftest") {
    let initRequest = try! JSONDecoder().decode(
        Request.self, from: Data(#"{"type":"init","locale":"en-US"}"#.utf8))
    assert(initRequest.type == "init")
    assert(initRequest.locale == "en-US")

    let transcribeRequest = try! JSONDecoder().decode(
        Request.self,
        from: Data(#"{"type":"transcribe","audio_base64":"AACAPw==","sample_rate":16000}"#.utf8))
    assert(transcribeRequest.type == "transcribe")
    assert(transcribeRequest.sampleRate == 16000)

    let decoded = Data(base64Encoded: transcribeRequest.audioBase64!)!
    let samples = decoded.withUnsafeBytes { Array($0.bindMemory(to: Float.self)) }
    assert(samples == [1.0], "base64 audio round-trip produced \(samples), expected [1.0]")

    print("OK")
    exit(0)
}

// ============================================================================
// Main loop
// ============================================================================

guard #available(macOS 26.0, *) else {
    sendError(HelperError.unsupportedOS.description)
    exit(1)
}

private let session = SpeechSession()

while let line = readLine(strippingNewline: true) {
    guard !line.isEmpty else { continue }
    guard let data = line.data(using: .utf8),
          let request = try? JSONDecoder().decode(Request.self, from: data)
    else {
        sendError("Could not parse request")
        continue
    }

    switch request.type {
    case "init":
        do {
            let resolved = try await session.ensureReady(localeIdentifier: request.locale ?? "en-US")
            send(["type": "ready", "locale": resolved])
        } catch {
            sendError("\(error)")
        }

    case "transcribe":
        guard let base64 = request.audioBase64, let raw = Data(base64Encoded: base64) else {
            sendError("Missing or invalid audio_base64")
            continue
        }
        let samples = raw.withUnsafeBytes { rawBuffer in
            Array(rawBuffer.bindMemory(to: Float.self))
        }
        do {
            let text = try await session.transcribe(samples: samples, sampleRate: request.sampleRate ?? 16000)
            send(["type": "transcript", "text": text, "is_final": true])
        } catch {
            sendError("\(error)")
        }

    case "ping":
        send(["type": "pong"])

    case "shutdown":
        send(["type": "goodbye"])
        exit(0)

    default:
        sendError("Unknown request type: \(request.type)")
    }
}
