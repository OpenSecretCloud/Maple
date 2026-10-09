import CoreGraphics
import Foundation
import ImageIO
import MapleAppCore
import UniformTypeIdentifiers

/// History images can originate in another client. Validate their encoded
/// dimensions before asking ImageIO to allocate even a downsampled raster.
enum ImagePreviewDecoding {
    static func thumbnail(_ data: Data, edge: Int) throws -> CGImage {
        try Task.checkCancellation()
        guard !data.isEmpty, data.count <= ImageImportProcessor.maximumInputBytes else {
            throw ImageImportIssue.tooLarge
        }
        let allowedTypes = [
            UTType.png.identifier, UTType.jpeg.identifier, UTType.webP.identifier,
            UTType.gif.identifier, UTType.heic.identifier, UTType.heif.identifier,
            UTType(mimeType: "image/avif")?.identifier,
        ].compactMap { $0 }
        guard
            let source = CGImageSourceCreateWithData(
                data as CFData, [kCGImageSourceShouldCache: false] as CFDictionary),
            CGImageSourceGetStatus(source) == .statusComplete,
            CGImageSourceGetCount(source) >= 1,
            CGImageSourceGetStatusAtIndex(source, 0) == .statusComplete,
            let type = CGImageSourceGetType(source) as String?, allowedTypes.contains(type),
            let properties = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any],
            let width = properties[kCGImagePropertyPixelWidth] as? NSNumber,
            let height = properties[kCGImagePropertyPixelHeight] as? NSNumber
        else { throw ImageImportIssue.invalidImage }
        try validateDimensions(width: width.doubleValue, height: height.doubleValue)
        try Task.checkCancellation()
        // Animated history attachments deliberately display only the first frame.
        guard
            let image = CGImageSourceCreateThumbnailAtIndex(
                source, 0,
                [
                    kCGImageSourceCreateThumbnailFromImageAlways: true,
                    kCGImageSourceCreateThumbnailWithTransform: true,
                    kCGImageSourceThumbnailMaxPixelSize: min(4_096, max(1, edge)),
                    kCGImageSourceShouldCacheImmediately: true,
                ] as CFDictionary)
        else { throw ImageImportIssue.invalidImage }
        try Task.checkCancellation()
        return image
    }

    static func validateDimensions(width: Double, height: Double) throws {
        guard width.isFinite, height.isFinite, width >= 1, height >= 1,
            width <= 100_000, height <= 100_000, width * height <= 100_000_000
        else { throw ImageImportIssue.tooLarge }
    }
}

#if os(iOS)
    import MapleSDK
    import SwiftUI

    private actor PreviewDecoder {
        static let shared = PreviewDecoder()
        func decode(_ data: Data, edge: Int) throws -> CGImage {
            try ImagePreviewDecoding.thumbnail(data, edge: edge)
        }
        func inlineImage(_ value: JSONValue) throws -> ChatImageAttachment {
            let source: String
            switch value {
            case .string(let text): source = text
            case .object(let object):
                guard case .string(let text) = object["url"] else { throw ImageImportIssue.invalidImage }
                source = text
            default: throw ImageImportIssue.invalidImage
            }
            guard source.utf8.count <= 28 * 1_024 * 1_024,
                let separator = source.firstIndex(of: ",")
            else { throw ImageImportIssue.tooLarge }
            let prefix = String(source[..<separator])
            let types = ["image/png", "image/jpeg", "image/webp", "image/gif", "image/avif"]
            guard let type = types.first(where: { prefix == "data:\($0);base64" }),
                let data = Data(base64Encoded: String(source[source.index(after: separator)...])),
                !data.isEmpty, data.count <= ImageImportProcessor.maximumInputBytes
            else { throw ImageImportIssue.invalidImage }
            return ChatImageAttachment(data: data, contentType: type)
        }
    }

    struct DraftImageStrip: View {
        @Bindable var importer: ImageImportController
        @State private var preview: ChatImageAttachment?
        var body: some View {
            if !importer.runtime.snapshot.draft.images.isEmpty {
                ScrollView(.horizontal) {
                    HStack(spacing: 12) {
                        ForEach(importer.runtime.snapshot.draft.images) { image in
                            VStack(spacing: 2) {
                                Button {
                                    preview = image
                                } label: {
                                    ImageThumbnail(attachment: image)
                                }
                                .accessibilityLabel(MapleStrings.text("View attached image"))
                                Button(MapleStrings.text("Remove image"), systemImage: "xmark") {
                                    importer.removeImage(image.id, draftID: importer.runtime.snapshot.draft.id)
                                }.labelStyle(.iconOnly).frame(minWidth: 44, minHeight: 44)
                                    .accessibilityIdentifier("images.remove." + image.id.uuidString)
                            }
                        }
                    }
                }
                .chatFullScreenCover(item: $preview) { ImageZoomView(attachment: $0) }
            }
        }
    }

    struct MessageImages: View {
        let content: [MessageContent]
        var body: some View {
            ScrollView(.horizontal) {
                HStack(spacing: 12) {
                    ForEach(Array(content.enumerated()), id: \.offset) { _, part in
                        if let value = part.imageUrl { InlineMessageImage(value: value) }
                    }
                }
            }
        }
    }

    private struct InlineMessageImage: View {
        let value: JSONValue
        @State private var attachment: ChatImageAttachment?
        @State private var preview: ChatImageAttachment?
        @State private var failed = false
        var body: some View {
            Group {
                if let attachment {
                    Button {
                        preview = attachment
                    } label: {
                        ImageThumbnail(attachment: attachment, side: 180)
                    }
                    .accessibilityLabel(MapleStrings.text("View message image"))
                } else if failed {
                    Label(MapleStrings.text("Image unavailable"), systemImage: "photo.badge.exclamationmark").font(
                        .footnote)
                } else {
                    Image(systemName: "photo").foregroundStyle(.secondary)
                        .frame(width: 120, height: 120).accessibilityHidden(true)
                }
            }
            .task {
                do {
                    let decoded = try await PreviewDecoder.shared.inlineImage(value)
                    try Task.checkCancellation(); attachment = decoded
                } catch { if !Task.isCancelled { failed = true } }
            }
            .chatFullScreenCover(item: $preview) { ImageZoomView(attachment: $0) }
        }
    }

    private struct ImageThumbnail: View {
        let attachment: ChatImageAttachment
        var side: CGFloat = 84
        @State private var image: CGImage?
        var body: some View {
            Group {
                if let image {
                    Image(decorative: image, scale: 1).resizable().scaledToFit()
                } else {
                    Image(systemName: "photo").foregroundStyle(.secondary)
                }
            }
            .frame(width: side, height: side).background(.quaternary, in: RoundedRectangle(cornerRadius: 10))
            .task(id: attachment.id) {
                let decoded = try? await PreviewDecoder.shared.decode(attachment.data, edge: Int(side * 3))
                if !Task.isCancelled { image = decoded }
            }
        }
    }

    struct ImageZoomView: View {
        let attachment: ChatImageAttachment
        @Environment(\.dismiss) private var dismiss
        @State private var image: CGImage?
        @State private var failed = false
        @State private var scale: CGFloat = 1
        @State private var offset = CGSize.zero
        @GestureState private var magnification: CGFloat = 1
        @GestureState private var translation = CGSize.zero
        var body: some View {
            NavigationStack {
                GeometryReader { geometry in
                    ZStack {
                        Color(uiColor: .systemBackground)
                        if let image {
                            let zoom = min(6, max(1, scale * magnification))
                            Image(decorative: image, scale: 1).resizable().scaledToFit()
                                .frame(width: geometry.size.width, height: geometry.size.height)
                                .scaleEffect(zoom)
                                .offset(
                                    clamped(
                                        CGSize(
                                            width: offset.width + translation.width,
                                            height: offset.height + translation.height), in: geometry.size, scale: zoom)
                                )
                                .gesture(
                                    MagnifyGesture().updating($magnification) { value, state, _ in
                                        state = value.magnification
                                    }
                                    .onEnded { value in
                                        scale = min(6, max(1, scale * value.magnification))
                                        offset = clamped(offset, in: geometry.size, scale: scale)
                                    }
                                )
                                .simultaneousGesture(
                                    DragGesture().updating($translation) { value, state, _ in
                                        if scale > 1 { state = value.translation }
                                    }.onEnded { value in
                                        offset = clamped(
                                            CGSize(
                                                width: offset.width + value.translation.width,
                                                height: offset.height + value.translation.height), in: geometry.size,
                                            scale: scale)
                                    }
                                )
                                .accessibilityLabel(MapleStrings.text("Attached image"))
                                .accessibilityAction(named: MapleStrings.text("Zoom in")) { scale = min(6, scale + 1) }
                                .accessibilityAction(named: MapleStrings.text("Zoom out")) {
                                    scale = max(1, scale - 1); offset = .zero
                                }
                        } else if failed {
                            Text(MapleStrings.text("Image unavailable"))
                        } else {
                            Image(systemName: "photo").foregroundStyle(.secondary).accessibilityHidden(true)
                        }
                    }.clipped()
                }
                .navigationTitle(MapleStrings.text("Image")).navigationBarTitleDisplayMode(.inline)
                .toolbar {
                    ToolbarItem(placement: .primaryAction) {
                        Button(MapleStrings.text("Close image"), systemImage: "xmark") { dismiss() }
                            .accessibilityIdentifier(
                                "images.close")
                    }
                }
            }
            .task {
                do {
                    let value = try await PreviewDecoder.shared.decode(attachment.data, edge: 4_096)
                    try Task.checkCancellation(); image = value
                } catch { if !Task.isCancelled { failed = true } }
            }
        }
        private func clamped(_ value: CGSize, in bounds: CGSize, scale: CGFloat) -> CGSize {
            let x = max(0, bounds.width * (scale - 1) / 2), y = max(0, bounds.height * (scale - 1) / 2)
            return CGSize(width: min(x, max(-x, value.width)), height: min(y, max(-y, value.height)))
        }
    }
#endif
