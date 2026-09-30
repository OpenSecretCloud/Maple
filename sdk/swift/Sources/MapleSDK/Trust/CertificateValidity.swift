import Foundation

/// Only extracts X.509 validity. Security.framework owns path, constraints and
/// signature validation; this parser never implements certificate trust.
struct CertificateValidity {
    let notBefore: Date
    let notAfter: Date

    init(der: Data) throws {
        let root = try DERNode.parse(der)
        guard root.tag == 0x30 else { throw TrustError.certificateChain }
        let certificate = try root.children()
        guard certificate.count == 3, certificate[0].tag == 0x30 else { throw TrustError.certificateChain }
        let tbs = try certificate[0].children()
        let validityIndex = tbs.first?.tag == 0xa0 ? 4 : 3
        guard tbs.count > validityIndex, tbs[validityIndex].tag == 0x30 else { throw TrustError.certificateChain }
        let validity = try tbs[validityIndex].children()
        guard validity.count == 2 else { throw TrustError.certificateChain }
        notBefore = try Self.date(validity[0])
        notAfter = try Self.date(validity[1])
        guard notBefore <= notAfter else { throw TrustError.certificateChain }
    }

    private static func date(_ node: DERNode) throws -> Date {
        guard var text = String(data: node.content, encoding: .ascii), text.last == "Z" else {
            throw TrustError.certificateChain
        }
        if node.tag == 0x17, text.count == 13, let year = Int(text.prefix(2)) {
            text = (year >= 50 ? "19" : "20") + text
        } else if node.tag != 0x18 || text.count != 15 {
            throw TrustError.certificateChain
        }
        guard text.dropLast().utf8.allSatisfy({ (48...57).contains($0) }) else {
            throw TrustError.certificateChain
        }
        let formatter = DateFormatter()
        formatter.calendar = Calendar(identifier: .gregorian)
        formatter.locale = Locale(identifier: "en_US_POSIX")
        formatter.timeZone = TimeZone(secondsFromGMT: 0)
        formatter.dateFormat = "yyyyMMddHHmmss'Z'"
        formatter.isLenient = false
        guard let date = formatter.date(from: text), formatter.string(from: date) == text else {
            throw TrustError.certificateChain
        }
        return date
    }

    func isValid(at date: Date) -> Bool {
        date >= notBefore.addingTimeInterval(-300) && date <= notAfter
    }
}

private struct DERNode {
    let tag: UInt8
    let content: Data

    static func parse(_ data: Data) throws -> DERNode {
        var reader = DERReader(bytes: Array(data))
        let node = try reader.next()
        guard reader.offset == data.count else { throw TrustError.certificateChain }
        return node
    }

    func children() throws -> [DERNode] {
        var reader = DERReader(bytes: Array(content))
        var children: [DERNode] = []
        while reader.offset < content.count {
            guard children.count < 64 else { throw TrustError.certificateChain }
            children.append(try reader.next())
        }
        return children
    }
}

private struct DERReader {
    let bytes: [UInt8]
    var offset = 0

    mutating func next() throws -> DERNode {
        guard offset + 2 <= bytes.count else { throw TrustError.certificateChain }
        let tag = bytes[offset]
        let size = bytes[offset + 1]
        offset += 2
        // X.509 fields inspected here use one-byte tags and definite DER lengths.
        guard tag & 31 != 31 else { throw TrustError.certificateChain }
        var length = Int(size)
        if size & 0x80 != 0 {
            let count = Int(size & 0x7f)
            guard (1...4).contains(count), offset + count <= bytes.count, bytes[offset] != 0 else {
                throw TrustError.certificateChain
            }
            length = 0
            for byte in bytes[offset..<(offset + count)] { length = length << 8 | Int(byte) }
            offset += count
            guard length >= 128 else { throw TrustError.certificateChain }
        }
        guard length <= bytes.count - offset else { throw TrustError.certificateChain }
        defer { offset += length }
        return DERNode(tag: tag, content: Data(bytes[offset..<(offset + length)]))
    }
}
