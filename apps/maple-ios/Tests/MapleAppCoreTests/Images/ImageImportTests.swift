import CoreGraphics
import Foundation
import ImageIO
import Testing
import UniformTypeIdentifiers
@testable import MapleAppCore

private actor ImageFixtureProcessor: ImageProcessing {
    func process(_ data: Data) throws -> ChatImageAttachment {
        if data.first == 0 { throw ImageImportIssue.invalidImage }
        return ChatImageAttachment(data: data, contentType: "image/png")
    }
}

@MainActor private final class ImageAdmissionState { var allowed = true }

@MainActor struct ImageImportOwnershipTests {
    private func makeRuntime(snapshot: ChatSnapshot = ChatSnapshot()) async -> ChatRuntime {
        let store = ChatFixtureStore(); await store.seed(snapshot)
        let runtime = ChatRuntime(
            scope: AccountScope(apiOrigin: "http://localhost:1234", principalID: "image-fixture"),
            service: ChatFixtureService(store: store), persistence: store)
        await runtime.load(chatID: nil)
        return runtime
    }

    @Test func unavailableAccessDoesNotReadAProvider() async {
        let runtime = await makeRuntime(); runtime.draftText = "Keep this draft"
        let importer = ImageImportController(runtime: runtime, processor: ImageFixtureProcessor(), admission: { false })
        importer.importImages(
            [
                SelectedImageSource {
                    Issue.record("An unavailable account read an image provider"); return Data([1])
                }
            ], draftID: runtime.snapshot.draft.id)
        await importer.waitForImports()
        #expect(importer.issue == .unavailable)
        #expect(runtime.snapshot.draft.text == "Keep this draft")
        #expect(runtime.snapshot.draft.images.isEmpty)
        runtime.invalidate()
    }

    @Test func revokedAccessDuringProviderReadPreservesTheEntireBatch() async {
        let runtime = await makeRuntime(); runtime.draftText = "Keep this draft"
        let state = ImageAdmissionState(), gate = ChatTestGate()
        let importer = ImageImportController(
            runtime: runtime, processor: ImageFixtureProcessor(), admission: { state.allowed })
        importer.importImages(
            [
                SelectedImageSource { Data([1]) },
                SelectedImageSource {
                    await gate.hold(); return Data([2])
                },
            ], draftID: runtime.snapshot.draft.id)
        await gate.waitForArrival(); state.allowed = false; await gate.release()
        await importer.waitForImports()
        #expect(importer.issue == .unavailable)
        #expect(runtime.snapshot.draft.text == "Keep this draft")
        #expect(runtime.snapshot.draft.images.isEmpty)
        #expect(runtime.busyDraftIDs.isEmpty)
        runtime.invalidate()
    }

    @Test func quiescencePreventsAReplacementImportFromStarting() async {
        let runtime = await makeRuntime()
        let importer = ImageImportController(runtime: runtime, processor: ImageFixtureProcessor())
        let fence = UUID(); runtime.beginDeletionFence(fence); runtime.requestDeletionQuiescence(fence)
        importer.importImages(
            [
                SelectedImageSource {
                    Issue.record("A fenced importer read its provider"); return Data([1])
                }
            ],
            draftID: runtime.snapshot.draft.id)
        await importer.waitForImports()
        #expect(importer.issue == .retired)
        #expect(runtime.snapshot.draft.images.isEmpty)
        runtime.invalidate()
    }

    @Test func temporaryBytesAreBoundedAcrossSimultaneousDrafts() async {
        let budget = ImageImportBudget(maximumTemporaryBytes: 10)
        let first = await makeRuntime(), second = await makeRuntime()
        let firstImporter = ImageImportController(runtime: first, processor: ImageFixtureProcessor(), budget: budget)
        let secondImporter = ImageImportController(runtime: second, processor: ImageFixtureProcessor(), budget: budget)
        let gate = ChatTestGate()
        firstImporter.importImages(
            [
                SelectedImageSource { Data(repeating: 1, count: 6) },
                SelectedImageSource {
                    await gate.hold(); return Data([2, 2])
                },
            ], draftID: first.snapshot.draft.id)
        await gate.waitForArrival()
        secondImporter.importImages(
            [SelectedImageSource { Data(repeating: 2, count: 6) }], draftID: second.snapshot.draft.id)
        await secondImporter.waitForImports()
        #expect(secondImporter.issue == .attachmentLimit)
        #expect(second.snapshot.draft.images.isEmpty)
        await gate.release(); await firstImporter.waitForImports()
        #expect(first.snapshot.draft.images.count == 2)
        first.invalidate(); second.invalidate()
    }

    @Test func aFailedMultiSelectionLeavesTheEntireDraftUnchanged() async {
        let original = ChatDraft(
            text: "Keep this text", images: [ChatImageAttachment(data: Data([8]), contentType: "image/png")])
        let runtime = await makeRuntime(snapshot: ChatSnapshot(draft: original))
        let importer = ImageImportController(runtime: runtime, processor: ImageFixtureProcessor())
        importer.importImages(
            [SelectedImageSource { Data([1]) }, SelectedImageSource { Data([0]) }], draftID: original.id)
        await importer.waitForImports()
        #expect(importer.issue == .invalidImage)
        #expect(runtime.snapshot.draft == original)
        #expect(runtime.busyDraftIDs.isEmpty)
        runtime.invalidate()
    }

    @Test func importToHeldDraftDoesNotOverwriteQueueEditorOrLaterTyping() async {
        let original = ChatDraft(text: "B")
        let queued = ChatQueuedTurn(draft: ChatDraft(text: "A"))
        let runtime = await makeRuntime(
            snapshot: ChatSnapshot(draft: original, queue: ChatQueueState(items: [queued], paused: true)))
        let importer = ImageImportController(runtime: runtime, processor: ImageFixtureProcessor())
        let gate = ChatTestGate()
        importer.importImages(
            [
                SelectedImageSource {
                    await gate.hold(); return Data([1])
                }
            ], draftID: original.id)
        await gate.waitForArrival()
        #expect(runtime.beginQueueEdit(id: queued.id))
        runtime.draftText = "A edited while B imports"
        await gate.release(); await importer.waitForImports()
        #expect(runtime.snapshot.draft.text == "A edited while B imports")
        #expect(runtime.snapshot.draft.images.isEmpty)
        #expect(runtime.snapshot.queue?.edit?.heldDraft.text == "B")
        #expect(runtime.snapshot.queue?.edit?.heldDraft.images.count == 1)
        runtime.cancelQueueEdit()
        #expect(runtime.snapshot.draft.id == original.id)
        #expect(runtime.snapshot.draft.images.count == 1)
        runtime.invalidate()
    }

    @Test func retiredImportCannotWriteIntoReplacementAccount() async {
        let runtime = await makeRuntime(); runtime.draftText = "Old account"
        let replacement = await makeRuntime(); replacement.draftText = "Replacement"
        let importer = ImageImportController(runtime: runtime, processor: ImageFixtureProcessor())
        let gate = ChatTestGate()
        importer.importImages(
            [
                SelectedImageSource {
                    await gate.hold(); return Data([1])
                }
            ], draftID: runtime.snapshot.draft.id)
        await gate.waitForArrival()
        importer.invalidate(); runtime.invalidate()
        await gate.release(); await importer.waitForImports()
        #expect(replacement.snapshot.draft.text == "Replacement")
        #expect(replacement.snapshot.draft.images.isEmpty)
        #expect(runtime.snapshot.draft.images.isEmpty)
        #expect(runtime.busyDraftIDs.isEmpty)
        replacement.invalidate()
    }

    @Test func capacityFailureDoesNotReplaceExistingImages() async {
        let runtime = await makeRuntime(); runtime.draftText = "Keep"
        runtime.canAdmitSnapshot = { _ in false }
        let importer = ImageImportController(runtime: runtime, processor: ImageFixtureProcessor())
        importer.importImages([SelectedImageSource { Data([1]) }], draftID: runtime.snapshot.draft.id)
        await importer.waitForImports()
        #expect(importer.issue == .attachmentLimit)
        #expect(runtime.snapshot.draft.text == "Keep")
        #expect(runtime.snapshot.draft.images.isEmpty)
        runtime.invalidate()
    }
}

struct ImageCodecTests {
    @Test func orientationIsAppliedAndSourceMetadataIsRemoved() async throws {
        let input = try fixture(type: .jpeg, orientation: 6)
        let image = try await ImageImportProcessor().process(input)
        let source = try #require(CGImageSourceCreateWithData(image.data as CFData, nil))
        let properties = try #require(CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any])
        #expect((properties[kCGImagePropertyPixelWidth] as? NSNumber)?.intValue == 96)
        #expect((properties[kCGImagePropertyPixelHeight] as? NSNumber)?.intValue == 160)
        #expect(properties[kCGImagePropertyGPSDictionary] == nil)
        #expect((properties[kCGImagePropertyTIFFDictionary] as? [CFString: Any])?[kCGImagePropertyTIFFArtist] == nil)
        #expect(image.contentType == "image/jpeg")
    }

    @Test func pngKeepsScreenshotDimensionsAndPixels() async throws {
        let input = try fixture(type: .png)
        let output = try await ImageImportProcessor().process(input)
        func pixels(_ data: Data) throws -> Data {
            let source = try #require(CGImageSourceCreateWithData(data as CFData, nil))
            let image = try #require(CGImageSourceCreateImageAtIndex(source, 0, nil))
            let context = try #require(
                CGContext(
                    data: nil, width: 160, height: 96, bitsPerComponent: 8, bytesPerRow: 640,
                    space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue))
            context.draw(image, in: CGRect(x: 0, y: 0, width: 160, height: 96))
            return Data(bytes: try #require(context.data), count: 640 * 96)
        }
        #expect(try pixels(input) == pixels(output.data))
        #expect(output.contentType == "image/png")
    }

    @Test func corruptAndOversizedInputsAreRejected() async {
        let processor = ImageImportProcessor()
        await #expect(throws: ImageImportIssue.self) { try await processor.process(Data("not an image".utf8)) }
        await #expect(throws: ImageImportIssue.tooLarge) {
            try await processor.process(Data(count: ImageImportProcessor.maximumInputBytes + 1))
        }
    }

    @Test func heicIsConvertedToJPEG() async throws {
        let input = try fixture(type: .heic)
        let output = try await ImageImportProcessor().process(input)
        #expect(output.contentType == "image/jpeg")
        #expect(output.data.prefix(2) == Data([0xff, 0xd8]))
    }

    private func fixture(type: UTType, orientation: Int = 1) throws -> Data {
        let context = try #require(
            CGContext(
                data: nil, width: 160, height: 96, bitsPerComponent: 8, bytesPerRow: 640,
                space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue))
        context.setFillColor(CGColor(red: 1, green: 1, blue: 1, alpha: 1));
        context.fill(CGRect(x: 0, y: 0, width: 160, height: 96))
        context.setFillColor(CGColor(red: 0, green: 0, blue: 0, alpha: 1))
        for x in stride(from: 8, to: 150, by: 8) { context.fill(CGRect(x: x, y: 8, width: 2, height: 60)) }
        let image = try #require(context.makeImage())
        let data = NSMutableData()
        let destination = try #require(CGImageDestinationCreateWithData(data, type.identifier as CFString, 1, nil))
        CGImageDestinationAddImage(
            destination, image,
            [
                kCGImagePropertyOrientation: orientation,
                kCGImagePropertyTIFFDictionary: [kCGImagePropertyTIFFArtist: "Synthetic author"],
                kCGImagePropertyGPSDictionary: [
                    kCGImagePropertyGPSLatitude: 10.0, kCGImagePropertyGPSLatitudeRef: "N",
                ],
            ] as CFDictionary)
        #expect(CGImageDestinationFinalize(destination))
        return data as Data
    }
}
