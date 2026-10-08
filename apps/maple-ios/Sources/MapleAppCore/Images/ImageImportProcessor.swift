import CoreGraphics
import Foundation
import ImageIO
import UniformTypeIdentifiers

public enum ImageImportIssue: Error, Equatable, Sendable {
    case unsupported, tooLarge, invalidImage, unavailable, retired, attachmentLimit
}

/// ImageIO decodes one bounded thumbnail at a time off the UI actor. Re-encoding
/// removes source metadata and converts native HEIF captures into a wire format.
public protocol ImageProcessing: Sendable {
    func process(_ data: Data) async throws -> ChatImageAttachment
}

public actor ImageImportProcessor: ImageProcessing {
    public static let maximumInputBytes = 20 * 1_024 * 1_024
    public init() {}

    public func process(_ data: Data) throws -> ChatImageAttachment {
        try Task.checkCancellation()
        guard !data.isEmpty, data.count <= Self.maximumInputBytes else { throw ImageImportIssue.tooLarge }
        guard
            let source = CGImageSourceCreateWithData(
                data as CFData, [kCGImageSourceShouldCache: false] as CFDictionary),
            CGImageSourceGetStatus(source) == .statusComplete,
            CGImageSourceGetCount(source) == 1,
            let type = CGImageSourceGetType(source) as String?,
            [
                UTType.png.identifier, UTType.jpeg.identifier, UTType.webP.identifier,
                UTType.heic.identifier, UTType.heif.identifier,
            ].contains(type),
            let properties = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any],
            let width = properties[kCGImagePropertyPixelWidth] as? NSNumber,
            let height = properties[kCGImagePropertyPixelHeight] as? NSNumber
        else { throw ImageImportIssue.unsupported }
        guard width.doubleValue.isFinite, height.doubleValue.isFinite,
            width.doubleValue >= 1, height.doubleValue >= 1,
            width.doubleValue <= 100_000, height.doubleValue <= 100_000,
            width.doubleValue * height.doubleValue <= 100_000_000
        else { throw ImageImportIssue.tooLarge }
        let lossless = type == UTType.png.identifier || type == UTType.webP.identifier
        let outputType = lossless ? UTType.png : UTType.jpeg
        // Most screenshots retain their native dimensions. Exceptionally large
        // encoded results step down conservatively instead of truncating bytes.
        for edge in [4_096, 3_072, 2_048] {
            try Task.checkCancellation()
            let options: [CFString: Any] = [
                kCGImageSourceCreateThumbnailFromImageAlways: true,
                kCGImageSourceCreateThumbnailWithTransform: true,
                kCGImageSourceThumbnailMaxPixelSize: edge,
                kCGImageSourceShouldCacheImmediately: true,
            ]
            guard let image = CGImageSourceCreateThumbnailAtIndex(source, 0, options as CFDictionary) else {
                throw ImageImportIssue.invalidImage
            }
            let result = NSMutableData()
            guard let destination = CGImageDestinationCreateWithData(result, outputType.identifier as CFString, 1, nil)
            else {
                throw ImageImportIssue.unavailable
            }
            CGImageDestinationAddImage(
                destination, image,
                [kCGImageDestinationLossyCompressionQuality: 0.92] as CFDictionary)
            guard CGImageDestinationFinalize(destination) else { throw ImageImportIssue.invalidImage }
            try Task.checkCancellation()
            if result.length <= Self.maximumInputBytes {
                return ChatImageAttachment(data: result as Data, contentType: lossless ? "image/png" : "image/jpeg")
            }
        }
        throw ImageImportIssue.tooLarge
    }

    /// Read only an explicitly selected provider URL while its grant is held.
    /// No user source is changed or removed, and file size is checked again by
    /// the bounded read even if the provider's metadata is stale.
    public nonisolated static func readSelection(_ url: URL) throws -> Data {
        let scoped = url.startAccessingSecurityScopedResource()
        defer { if scoped { url.stopAccessingSecurityScopedResource() } }
        let attributes = try url.resourceValues(forKeys: [.isRegularFileKey, .fileSizeKey])
        guard attributes.isRegularFile == true else { throw ImageImportIssue.unsupported }
        guard let bytes = attributes.fileSize, bytes > 0, bytes <= maximumInputBytes else {
            throw ImageImportIssue.tooLarge
        }
        let file = try FileHandle(forReadingFrom: url)
        defer { try? file.close() }
        var data = Data()
        while data.count <= maximumInputBytes {
            try Task.checkCancellation()
            let remaining = maximumInputBytes + 1 - data.count
            guard let chunk = try file.read(upToCount: min(remaining, 128 * 1_024)), !chunk.isEmpty else { break }
            data.append(chunk)
        }
        guard !data.isEmpty, data.count <= maximumInputBytes else { throw ImageImportIssue.tooLarge }
        return data
    }
}
