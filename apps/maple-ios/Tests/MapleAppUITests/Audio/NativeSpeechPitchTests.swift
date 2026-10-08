import AVFoundation
import Foundation
import Testing

/// Objective software-rendering proof for the selected time-pitch mechanism.
/// This is not a claim about physical speakers, voice quality or locked playback.
@Suite(.serialized) struct NativeSpeechPitchTests {
    @Test(arguments: [Float(1), 1.25, 1.5, 2])
    func savedRatesPreserveSyntheticPitch(rate: Float) throws {
        let sampleRate = 48_000.0, frequency = 440.0, seconds = 3.0
        let format = try #require(AVAudioFormat(standardFormatWithSampleRate: sampleRate, channels: 1))
        let input = try #require(
            AVAudioPCMBuffer(pcmFormat: format, frameCapacity: AVAudioFrameCount(seconds * sampleRate)))
        input.frameLength = input.frameCapacity
        let source = try #require(input.floatChannelData?[0])
        for frame in 0..<Int(input.frameLength) {
            source[frame] = 0.25 * Float(sin(2 * Double.pi * frequency * Double(frame) / sampleRate))
        }
        let engine = AVAudioEngine(), player = AVAudioPlayerNode(), pitch = AVAudioUnitTimePitch()
        engine.attach(player); engine.attach(pitch)
        engine.connect(player, to: pitch, format: format)
        engine.connect(pitch, to: engine.mainMixerNode, format: format)
        pitch.pitch = 0; pitch.rate = rate
        try engine.enableManualRenderingMode(.offline, format: format, maximumFrameCount: 1_024)
        let output = try #require(
            AVAudioPCMBuffer(
                pcmFormat: engine.manualRenderingFormat, frameCapacity: engine.manualRenderingMaximumFrameCount))
        player.scheduleBuffer(input)
        try engine.start(); player.play()
        defer { player.stop(); engine.stop() }
        let framesToRender = Int(sampleRate * (seconds / Double(rate) + 0.4))
        var rendered = 0, iterations = 0
        var lastSample: Float = 0
        var crossings: [Double] = []
        var firstAudible: Int?, lastAudible: Int?
        while rendered < framesToRender && iterations < 1_000 {
            iterations += 1
            let status = try engine.renderOffline(min(1_024, AVAudioFrameCount(framesToRender - rendered)), to: output)
            if status == .cannotDoInCurrentContext { continue }
            #expect(status == .success)
            guard status == .success else { break }
            let data = try #require(output.floatChannelData?[0])
            for frame in 0..<Int(output.frameLength) {
                let current = rendered + frame, sample = data[frame]
                if abs(sample) > 0.02 { firstAudible = firstAudible ?? current; lastAudible = current }
                let time = Double(current) / sampleRate
                if time > 0.3 && time < seconds / Double(rate) - 0.3, lastSample <= 0, sample > 0 {
                    let fraction = Double(-lastSample / (sample - lastSample))
                    crossings.append(Double(current - 1) + fraction)
                }
                lastSample = sample
            }
            rendered += Int(output.frameLength)
        }
        #expect(rendered == framesToRender)
        let first = try #require(crossings.first), last = try #require(crossings.last)
        let measuredFrequency = Double(crossings.count - 1) * sampleRate / (last - first)
        #expect(abs(measuredFrequency - frequency) < 4)
        let audibleFirst = try #require(firstAudible), audibleLast = try #require(lastAudible)
        let audibleDuration = Double(audibleLast - audibleFirst) / sampleRate
        #expect(abs(audibleDuration - seconds / Double(rate)) < 0.25)
    }
}
