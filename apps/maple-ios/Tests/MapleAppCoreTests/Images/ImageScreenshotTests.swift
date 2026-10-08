import CoreGraphics
import CoreText
import Foundation
import ImageIO
import Testing
import UniformTypeIdentifiers
import Vision
@testable import MapleAppCore

/// Entirely synthetic text at screenshot density. The source exceeds the
/// importer's maximum edge, so this fixture cannot pass by preserving its pixels.
/// Kept as drawing instructions to avoid a large binary fixture in the repository.
enum DenseScreenshotFixture {
    static let width = 5_120
    static let height = 3_200
    static let anchors = [
        "MAPLE SYNTHETIC SCREENSHOT",
        "Consent stays local",
        "No message text in logs",
        "Review every pending message",
    ]

    static func png() throws -> Data {
        let context = try #require(
            CGContext(
                data: nil, width: width, height: height, bitsPerComponent: 8, bytesPerRow: width * 4,
                space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue))
        context.setFillColor(CGColor(gray: 1, alpha: 1))
        context.fill(CGRect(x: 0, y: 0, width: width, height: height))
        context.setFillColor(CGColor(gray: 0.94, alpha: 1))
        context.fill(CGRect(x: 48, y: height - 142, width: width - 96, height: 100))
        context.setFillColor(CGColor(gray: 0.05, alpha: 1))
        context.textMatrix = .identity

        func line(_ text: String, x: CGFloat, y: CGFloat, size: CGFloat) {
            let font = CTFontCreateWithName("Menlo" as CFString, size, nil)
            let attributes: [NSAttributedString.Key: Any] = [
                NSAttributedString.Key(kCTFontAttributeName as String): font,
                NSAttributedString.Key(kCTForegroundColorFromContextAttributeName as String): true,
            ]
            let value = CTLineCreateWithAttributedString(NSAttributedString(string: text, attributes: attributes))
            context.textPosition = CGPoint(x: x, y: y)
            CTLineDraw(value, context)
        }
        line(anchors[0], x: 80, y: CGFloat(height - 112), size: 56)
        let samples = [
            "Consent stays local | Account: synthetic | Images: PNG | Queue: 16 | Draft: preserved",
            "No message text in logs | Native screenshot fixture | Width 5120 | Source is synthetic",
            "Review every pending message | project = local | send = explicit | owner = generation",
            "0123456789 ABCDEFGHIJKLMNOPQRSTUVWXYZ abcdefghijklmnopqrstuvwxyz [] {} () + - = / : ;",
        ]
        for column in 0..<2 {
            for row in 0..<54 {
                let text = String(format: "%02d.%02d  ", column + 1, row + 1) + samples[row % samples.count]
                line(text, x: CGFloat(80 + column * 2_520), y: CGFloat(height - 210 - row * 50), size: 32)
            }
        }
        line("END OF SYNTHETIC FIXTURE — no account or conversation data", x: 80, y: 120, size: 36)
        let image = try #require(context.makeImage())
        let data = NSMutableData()
        let destination = try #require(
            CGImageDestinationCreateWithData(data, UTType.png.identifier as CFString, 1, nil))
        CGImageDestinationAddImage(destination, image, nil)
        #expect(CGImageDestinationFinalize(destination))
        return data as Data
    }
}

struct ImageScreenshotTests {
    @Test func denseScreenshotActuallyDownscalesToTheBoundedLosslessWireImage() async throws {
        let input = try DenseScreenshotFixture.png()
        #expect(input.count <= ImageImportProcessor.maximumInputBytes)
        let source = try #require(CGImageSourceCreateWithData(input as CFData, nil))
        let before = try #require(CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any])
        #expect((before[kCGImagePropertyPixelWidth] as? NSNumber)?.intValue == 5_120)
        #expect((before[kCGImagePropertyPixelHeight] as? NSNumber)?.intValue == 3_200)

        let output = try await ImageImportProcessor().process(input)
        let result = try #require(CGImageSourceCreateWithData(output.data as CFData, nil))
        let after = try #require(CGImageSourceCopyPropertiesAtIndex(result, 0, nil) as? [CFString: Any])
        #expect(output.contentType == "image/png")
        #expect(CGImageSourceGetType(result) as String? == UTType.png.identifier)
        #expect(CGImageSourceGetStatus(result) == .statusComplete)
        #expect((after[kCGImagePropertyPixelWidth] as? NSNumber)?.intValue == 4_096)
        #expect((after[kCGImagePropertyPixelHeight] as? NSNumber)?.intValue == 2_560)
        #expect(output.data.count <= ImageImportProcessor.maximumInputBytes)
        #expect(output.data != input)
        let beforeText = try recognize(input), afterText = try recognize(output.data)
        for anchor in DenseScreenshotFixture.anchors {
            #expect(beforeText.contains(normalize(anchor)))
            #expect(afterText.contains(normalize(anchor)))
        }
        try exportSyntheticEvidence(input: input, output: output.data, recognized: afterText)
        // Native fullscreen/thumbnail inspection separately establishes usable
        // zoom and dismissal; OCR proves only these synthetic text anchors survive.
    }

    @Test func selectedSourceSurvivesImportAndDraftCleanup() async throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        let sourceURL = directory.appendingPathComponent("selected-source.png")
        let original = try DenseScreenshotFixture.png()
        try original.write(to: sourceURL, options: .withoutOverwriting)
        let read = try ImageImportProcessor.readSelection(sourceURL)
        let attachment = try await ImageImportProcessor().process(read)
        let store = ProtectedChatStore(root: directory.appendingPathComponent("protected-drafts"))
        let scope = AccountScope(apiOrigin: "http://127.0.0.1:1", principalID: "synthetic-image-source")
        try await store.activate(scope)
        try await store.save(ChatSnapshot(draft: ChatDraft(text: "Keep source", images: [attachment])), scope: scope)
        try await store.remove(chatID: ChatSnapshot.newChatKey, scope: scope)
        try await store.retire(scope)
        #expect(FileManager.default.fileExists(atPath: sourceURL.path))
        #expect(try Data(contentsOf: sourceURL) == original)
    }

    private func recognize(_ data: Data) throws -> String {
        let request = VNRecognizeTextRequest()
        request.recognitionLevel = .accurate
        request.usesLanguageCorrection = false
        request.recognitionLanguages = ["en-US"]
        request.minimumTextHeight = 0.006
        try VNImageRequestHandler(data: data, options: [:]).perform([request])
        return normalize(
            (request.results ?? []).compactMap { $0.topCandidates(1).first?.string }.joined(separator: " "))
    }

    private func normalize(_ text: String) -> String {
        text.lowercased().components(separatedBy: CharacterSet.alphanumerics.inverted)
            .filter { !$0.isEmpty }.joined(separator: " ")
    }

    /// Explicit opt-in writes only this generated fixture. Normal test runs have
    /// no persistent artifacts; no imported user image can enter this path.
    private func exportSyntheticEvidence(input: Data, output: Data, recognized: String) throws {
        guard let value = ProcessInfo.processInfo.environment["MAPLE_IMAGE_EVIDENCE_DIRECTORY"], !value.isEmpty else {
            return
        }
        guard value.hasPrefix("/") else { throw CocoaError(.fileWriteInvalidFileName) }
        let directory = URL(fileURLWithPath: value, isDirectory: true)
            .appendingPathComponent("downscaled-screenshot-" + UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(
            at: directory, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
        try input.write(to: directory.appendingPathComponent("synthetic-source.png"), options: .withoutOverwriting)
        try output.write(to: directory.appendingPathComponent("synthetic-imported.png"), options: .withoutOverwriting)
        let manifest: [String: Any] = [
            "synthetic": true, "sourceWidth": 5120, "sourceHeight": 3200,
            "outputWidth": 4096, "outputHeight": 2560,
            "sourceBytes": input.count, "outputBytes": output.count,
            "ocrAnchors": DenseScreenshotFixture.anchors,
            "allOutputAnchorsRecognized": DenseScreenshotFixture.anchors.allSatisfy {
                recognized.contains(normalize($0))
            },
        ]
        try JSONSerialization.data(withJSONObject: manifest, options: [.prettyPrinted, .sortedKeys])
            .write(to: directory.appendingPathComponent("manifest.json"), options: .withoutOverwriting)
        print("Synthetic image evidence: \(directory.path)")
    }

}
