#if os(iOS)
    import CoreTransferable
    import ImageIO
    import MapleAppCore
    import PhotosUI
    import SwiftUI
    import UniformTypeIdentifiers

    private struct ImportedPhoto: Transferable, Sendable {
        let data: Data
        static var transferRepresentation: some TransferRepresentation {
            FileRepresentation(importedContentType: .image) { file in
                ImportedPhoto(data: try ImageImportProcessor.readSelection(file.file))
            }
        }
    }

    @MainActor private final class ClipboardImageSource {
        let provider: NSItemProvider
        init(provider: NSItemProvider) { self.provider = provider }
        func read() async throws -> Data {
            guard
                let type = provider.registeredTypeIdentifiers.first(where: { UTType($0)?.conforms(to: .image) == true })
            else {
                throw ImageImportIssue.unsupported
            }
            return try await withCheckedThrowingContinuation { continuation in
                // The selected provider owns this temporary URL only until its
                // callback returns, so consume its bounded file in that callback.
                _ = provider.loadFileRepresentation(forTypeIdentifier: type) { url, _ in
                    guard let url else { continuation.resume(throwing: ImageImportIssue.unavailable); return }
                    do { continuation.resume(returning: try ImageImportProcessor.readSelection(url)) } catch {
                        continuation.resume(throwing: ImageImportIssue.unavailable)
                    }
                }
            }
        }
    }

    private struct ImportOwner: Identifiable { let id: UUID }

    struct ImageAttachmentMenu: View {
        @Bindable var importer: ImageImportController
        let access: AccountAccess
        let onUnavailable: () -> Void
        @State private var photos = false
        @State private var files = false
        @State private var picked: [PhotosPickerItem] = []
        @State private var selectionOwner: UUID?
        @State private var cameraOwner: ImportOwner?
        @State private var pasteOwner: ImportOwner?
        @Environment(\.compactMenuPresentationDismissed) private var compactMenuPresentationDismissed

        var body: some View {
            Menu {
                Button(MapleStrings.text("Photo library"), systemImage: "photo.on.rectangle") {
                    select { photos = true }
                }
                Button(MapleStrings.text("Take photo"), systemImage: "camera") {
                    select {
                        if UIImagePickerController.isSourceTypeAvailable(.camera) {
                            cameraOwner = ImportOwner(id: importer.runtime.snapshot.draft.id)
                        } else {
                            importer.report(.unavailable)
                        }
                    }
                }
                Button(MapleStrings.text("Files"), systemImage: "folder") { select { files = true } }
                Button(MapleStrings.text("Paste image"), systemImage: "document.on.clipboard") {
                    select { pasteOwner = ImportOwner(id: importer.runtime.snapshot.draft.id) }
                }
            } label: {
                Image(systemName: "plus")
            }
            .frame(minWidth: 44, minHeight: 44)
            .accessibilityLabel(MapleStrings.text("Attach image")).accessibilityIdentifier("chat.attach")
            .accessibilityValue(access == .allowed ? "" : MapleStrings.text("Unavailable"))
            .tracksChatPresentation(photos || files || pasteOwner != nil || cameraOwner != nil)
            .photosPicker(
                isPresented: $photos, selection: $picked,
                maxSelectionCount: max(1, 10 - importer.runtime.snapshot.draft.images.count),
                matching: .images, preferredItemEncoding: .current
            )
            .onChange(of: picked) { _, values in
                guard !values.isEmpty, let owner = selectionOwner else { return }
                picked = []
                importer.importImages(
                    values.map { item in
                        SelectedImageSource {
                            guard let image = try await item.loadTransferable(type: ImportedPhoto.self) else {
                                throw ImageImportIssue.unavailable
                            }
                            return image.data
                        }
                    }, draftID: owner)
            }
            .fileImporter(
                isPresented: $files, allowedContentTypes: [.png, .jpeg, .webP, .heic, .heif],
                allowsMultipleSelection: true
            ) { result in
                guard let owner = selectionOwner else { return }
                switch result {
                case .success(let urls):
                    importer.importImages(
                        urls.map { url in
                            SelectedImageSource {
                                try await Task.detached(priority: .userInitiated) {
                                    try ImageImportProcessor.readSelection(url)
                                }.value
                            }
                        }, draftID: owner)
                case .failure: importer.report(.unavailable)
                }
            }
            .sheet(item: $pasteOwner, onDismiss: compactMenuPresentationDismissed) { owner in
                NavigationStack {
                    VStack(spacing: 16) {
                        Text(MapleStrings.text("Paste an image from the clipboard."))
                        PasteButton(supportedContentTypes: [.image]) { providers in
                            guard providers.count <= 10 else {
                                importer.report(.attachmentLimit); pasteOwner = nil; return
                            }
                            importer.importImages(
                                providers.map { provider in
                                    let source = ClipboardImageSource(provider: provider)
                                    return SelectedImageSource { try await source.read() }
                                }, draftID: owner.id)
                            pasteOwner = nil
                        }.tint(MapleAccent.nativeFilled).accessibilityIdentifier("images.paste")
                    }.padding()
                        .navigationTitle(MapleStrings.text("Paste image"))
                        .toolbar {
                            ToolbarItem(placement: .cancellationAction) {
                                Button(MapleStrings.text("Cancel")) { pasteOwner = nil }
                            }
                        }
                }.presentationDetents([.medium])
            }
            .chatFullScreenCover(item: $cameraOwner) { owner in
                CameraCaptureView { image, orientation in
                    if let image {
                        importer.importImages(
                            [
                                SelectedImageSource {
                                    try await ImageCaptureEncoder.shared.encode(image, orientation: orientation)
                                }
                            ], draftID: owner.id)
                    }
                    cameraOwner = nil
                }
            }

        }
        private func select(_ action: () -> Void) {
            guard access == .allowed else { onUnavailable(); return }
            guard importer.runtime.snapshot.draft.images.count < 10 else { importer.report(.attachmentLimit); return }
            selectionOwner = importer.runtime.snapshot.draft.id; action()
        }
    }

    private actor ImageCaptureEncoder {
        static let shared = ImageCaptureEncoder()
        func encode(_ image: CGImage, orientation: UInt32) throws -> Data {
            try Task.checkCancellation()
            guard image.width > 0, image.height > 0, image.width <= 100_000, image.height <= 100_000,
                Double(image.width) * Double(image.height) <= 100_000_000
            else { throw ImageImportIssue.tooLarge }
            let edge = max(image.width, image.height)
            let factor = min(1, 4_096.0 / Double(edge))
            let width = max(1, Int(Double(image.width) * factor)), height = max(1, Int(Double(image.height) * factor))
            guard
                let context = CGContext(
                    data: nil, width: width, height: height, bitsPerComponent: 8, bytesPerRow: 0,
                    space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)
            else { throw ImageImportIssue.invalidImage }
            context.interpolationQuality = .high
            context.draw(image, in: CGRect(x: 0, y: 0, width: width, height: height))
            guard let reduced = context.makeImage() else { throw ImageImportIssue.invalidImage }
            try Task.checkCancellation()
            let data = NSMutableData()
            guard let output = CGImageDestinationCreateWithData(data, UTType.jpeg.identifier as CFString, 1, nil) else {
                throw ImageImportIssue.unavailable
            }
            CGImageDestinationAddImage(
                output, reduced,
                [
                    kCGImageDestinationLossyCompressionQuality: 0.92,
                    kCGImagePropertyOrientation: orientation,
                ] as CFDictionary)
            guard CGImageDestinationFinalize(output), data.length <= ImageImportProcessor.maximumInputBytes else {
                throw ImageImportIssue.tooLarge
            }
            return data as Data
        }
    }

    /// Camera capture has no SwiftUI camera view. This adapter only returns an
    /// immutable captured image; encoding and validation stay off the UI actor.
    private struct CameraCaptureView: UIViewControllerRepresentable {
        let completed: (CGImage?, UInt32) -> Void
        func makeCoordinator() -> Coordinator { Coordinator(completed: completed) }
        func makeUIViewController(context: Context) -> UIImagePickerController {
            let picker = UIImagePickerController(); picker.sourceType = .camera
            picker.mediaTypes = [UTType.image.identifier]; picker.delegate = context.coordinator
            return picker
        }
        func updateUIViewController(_ uiViewController: UIImagePickerController, context: Context) {}
        final class Coordinator: NSObject, UIImagePickerControllerDelegate, UINavigationControllerDelegate {
            let completed: (CGImage?, UInt32) -> Void
            init(completed: @escaping (CGImage?, UInt32) -> Void) { self.completed = completed }
            func imagePickerControllerDidCancel(_ picker: UIImagePickerController) { completed(nil, 1) }
            func imagePickerController(
                _ picker: UIImagePickerController,
                didFinishPickingMediaWithInfo info: [UIImagePickerController.InfoKey: Any]
            ) {
                guard let image = info[.originalImage] as? UIImage, let bitmap = image.cgImage else {
                    completed(nil, 1); return
                }
                let orientation: CGImagePropertyOrientation
                switch image.imageOrientation {
                case .up: orientation = .up
                case .down: orientation = .down
                case .left: orientation = .left
                case .right: orientation = .right
                case .upMirrored: orientation = .upMirrored
                case .downMirrored: orientation = .downMirrored
                case .leftMirrored: orientation = .leftMirrored
                case .rightMirrored: orientation = .rightMirrored
                @unknown default: orientation = .up
                }
                completed(bitmap, orientation.rawValue)
            }
        }
    }

    struct ImageImportIssueView: View {
        let issue: ImageImportIssue
        var body: some View {
            Text(label).font(.footnote).foregroundStyle(MapleRedText()).accessibilityIdentifier("images.error")
        }
        private var label: String {
            switch issue {
            case .tooLarge: MapleStrings.text("This image is too large. Choose an image smaller than 20 MB.")
            case .attachmentLimit:
                MapleStrings.text(
                    "This selection exceeds your current attachment limits. Existing images are unchanged.")
            case .unsupported, .invalidImage:
                MapleStrings.text("This image could not be read. Choose a PNG, JPEG, WebP, or HEIF image.")
            case .retired: MapleStrings.text("The draft for this image is no longer open.")
            case .unavailable: MapleStrings.text("The image could not be imported. Your existing draft is unchanged.")
            }
        }
    }
#endif
