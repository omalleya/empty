import AVFoundation
import Foundation
import Speech

/// Live speech-to-text using the Speech framework + AVAudioEngine.
///
/// Tap to start, tap to stop. `transcript` updates with partial results while
/// recording so the UI can show words appearing in real time.
///
/// Requires these Info.plist keys (set in project.yml):
///   NSMicrophoneUsageDescription
///   NSSpeechRecognitionUsageDescription
@MainActor
final class SpeechRecognizer: ObservableObject {
    enum RecognizerError: Error { case notAuthorized, unavailable }

    @Published var transcript = ""
    @Published var isRecording = false

    private let recognizer = SFSpeechRecognizer()
    private let audioEngine = AVAudioEngine()
    private var request: SFSpeechAudioBufferRecognitionRequest?
    private var task: SFSpeechRecognitionTask?

    /// Ask for both speech-recognition and microphone permission. Returns true
    /// only if both were granted.
    func requestPermission() async -> Bool {
        let speechOK = await withCheckedContinuation { cont in
            SFSpeechRecognizer.requestAuthorization { status in
                cont.resume(returning: status == .authorized)
            }
        }
        let micOK = await withCheckedContinuation { cont in
            AVAudioSession.sharedInstance().requestRecordPermission { granted in
                cont.resume(returning: granted)
            }
        }
        return speechOK && micOK
    }

    func start() throws {
        // Reset any prior session.
        task?.cancel()
        task = nil

        guard let recognizer, recognizer.isAvailable else {
            throw RecognizerError.unavailable
        }

        let session = AVAudioSession.sharedInstance()
        try session.setCategory(.record, mode: .measurement, options: .duckOthers)
        try session.setActive(true, options: .notifyOthersOnDeactivation)

        let request = SFSpeechAudioBufferRecognitionRequest()
        request.shouldReportPartialResults = true
        self.request = request

        let input = audioEngine.inputNode
        let format = input.outputFormat(forBus: 0)
        input.installTap(onBus: 0, bufferSize: 1024, format: format) { [weak self] buffer, _ in
            self?.request?.append(buffer)
        }

        audioEngine.prepare()
        try audioEngine.start()
        isRecording = true

        task = recognizer.recognitionTask(with: request) { [weak self] result, error in
            // Callback is not on the main actor — hop back before mutating state.
            Task { @MainActor in
                guard let self else { return }
                if let result {
                    self.transcript = result.bestTranscription.formattedString
                }
                if error != nil || (result?.isFinal ?? false) {
                    self.stop()
                }
            }
        }
    }

    func stop() {
        audioEngine.stop()
        audioEngine.inputNode.removeTap(onBus: 0)
        request?.endAudio()
        request = nil
        task?.cancel()
        task = nil
        isRecording = false
    }
}
