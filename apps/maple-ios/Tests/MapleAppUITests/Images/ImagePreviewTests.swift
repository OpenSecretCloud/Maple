import CoreGraphics
import Foundation
import ImageIO
import MapleAppCore
import Testing
import UniformTypeIdentifiers
@testable import MapleAppUI

struct ImagePreviewTests {
    @Test func animatedHistoryUsesFirstFrame() throws {
        let data = try fixture(type: .gif, width: 40, height: 20, frames: 2)
        let source = try #require(CGImageSourceCreateWithData(data as CFData, nil))
        #expect(CGImageSourceGetCount(source) == 2)
        let preview = try ImagePreviewDecoding.thumbnail(data, edge: 20)
        #expect(preview.width == 20)
        #expect(preview.height == 10)
        let context = try #require(
            CGContext(
                data: nil, width: 1, height: 1, bitsPerComponent: 8, bytesPerRow: 4,
                space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue))
        context.draw(preview, in: CGRect(x: 0, y: 0, width: 1, height: 1))
        let pixel = try #require(context.data).assumingMemoryBound(to: UInt8.self)
        #expect(pixel[0] > 240 && pixel[2] < 10)
    }

    @Test func encodedAxisLimitIsCheckedBeforeThumbnailAllocation() throws {
        let data = try fixture(type: .png, width: 100_001, height: 1)
        #expect(throws: ImageImportIssue.tooLarge) {
            try ImagePreviewDecoding.thumbnail(data, edge: 84)
        }
    }

    @Test(
        arguments: [
            (0.0, 1.0), (-1, 20), (1, Double.infinity), (Double.nan, 1),
            (100_001, 1), (1, 100_001), (10_001, 10_000),
        ] as [(Double, Double)])
    func untrustedDimensionsCannotAllocate(width: Double, height: Double) {
        #expect(throws: ImageImportIssue.tooLarge) {
            try ImagePreviewDecoding.validateDimensions(width: width, height: height)
        }
    }

    @Test func acceptedDimensionBoundaryDoesNotOverflow() throws {
        try ImagePreviewDecoding.validateDimensions(width: 100_000, height: 1_000)
        try ImagePreviewDecoding.validateDimensions(width: 1_000, height: 100_000)
    }

    @Test func incompleteAndOversizedHistoryAreRejected() throws {
        let png = try fixture(type: .png, width: 40, height: 20)
        #expect(throws: ImageImportIssue.invalidImage) {
            try ImagePreviewDecoding.thumbnail(Data(png.prefix(png.count / 2)), edge: 84)
        }
        #expect(throws: ImageImportIssue.tooLarge) {
            try ImagePreviewDecoding.thumbnail(
                Data(count: ImageImportProcessor.maximumInputBytes + 1), edge: 84)
        }
    }

    private func fixture(type: UTType, width: Int, height: Int, frames: Int = 1) throws -> Data {
        let context = try #require(
            CGContext(
                data: nil, width: width, height: height, bitsPerComponent: 8,
                bytesPerRow: width * 4, space: CGColorSpaceCreateDeviceRGB(),
                bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue))
        let output = NSMutableData()
        let destination = try #require(
            CGImageDestinationCreateWithData(output, type.identifier as CFString, frames, nil))
        for frame in 0..<frames {
            context.setFillColor(CGColor(red: frame == 0 ? 1 : 0, green: 0, blue: frame == 0 ? 0 : 1, alpha: 1))
            context.fill(CGRect(x: 0, y: 0, width: width, height: height))
            CGImageDestinationAddImage(destination, try #require(context.makeImage()), nil)
        }
        #expect(CGImageDestinationFinalize(destination))
        return output as Data
    }
}
