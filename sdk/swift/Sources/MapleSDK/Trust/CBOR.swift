import Foundation

/// A deliberately small, bounded CBOR implementation for Nitro's COSE profile.
/// Nitro emits both definite and indefinite containers. Duplicate keys, trailing
/// bytes, unsupported primitive types and unterminated containers are rejected.
indirect enum TrustCBOR: Equatable {
    case unsigned(UInt64), negative(Int64), bytes(Data), text(String)
    case array([TrustCBOR]), map([Pair]), tag(UInt64, TrustCBOR), null

    struct Pair: Equatable {
        let key: TrustCBOR
        let value: TrustCBOR
        init(_ key: TrustCBOR, _ value: TrustCBOR) { self.key = key; self.value = value }
    }

    var data: Data? { if case .bytes(let value) = self { value } else { nil } }
    var string: String? { if case .text(let value) = self { value } else { nil } }
    var uint: UInt64? { if case .unsigned(let value) = self { value } else { nil } }
    var items: [TrustCBOR]? { if case .array(let value) = self { value } else { nil } }
    var pairs: [Pair]? { if case .map(let value) = self { value } else { nil } }
    subscript(_ key: String) -> TrustCBOR? { pairs?.first { $0.key == .text(key) }?.value }

    static func decode(_ data: Data) throws -> TrustCBOR {
        guard !data.isEmpty, data.count <= 65_536 else { throw TrustError.malformedDocument }
        var reader = Reader(bytes: Array(data))
        let value = try reader.read(depth: 0)
        guard reader.offset == data.count else { throw TrustError.malformedDocument }
        return value
    }

    func encode() -> Data {
        switch self {
        case .unsigned(let n): Self.head(0, n)
        case .negative(let n): Self.head(1, UInt64(-(n + 1)))
        case .bytes(let data): Self.head(2, UInt64(data.count)) + data
        case .text(let text): Self.head(3, UInt64(text.utf8.count)) + Data(text.utf8)
        case .array(let items): Self.head(4, UInt64(items.count)) + items.reduce(Data()) { $0 + $1.encode() }
        case .map(let pairs):
            Self.head(5, UInt64(pairs.count)) + pairs.reduce(Data()) { $0 + $1.key.encode() + $1.value.encode() }
        case .tag(let tag, let value): Self.head(6, tag) + value.encode()
        case .null: Data([0xf6])
        }
    }

    private static func head(_ major: UInt8, _ n: UInt64) -> Data {
        if n < 24 { return Data([major << 5 | UInt8(n)]) }
        let width = n <= 0xff ? 1 : n <= 0xffff ? 2 : n <= 0xffff_ffff ? 4 : 8
        var result = Data([major << 5 | (width == 1 ? 24 : width == 2 ? 25 : width == 4 ? 26 : 27)])
        for i in (0..<width).reversed() { result.append(UInt8(truncatingIfNeeded: n >> (i * 8))) }
        return result
    }

    private struct Reader {
        let bytes: [UInt8]
        var offset = 0
        var nodes = 0

        mutating func take(_ count: Int) throws -> [UInt8] {
            guard count >= 0, count <= bytes.count - offset else { throw TrustError.malformedDocument }
            defer { offset += count }
            return Array(bytes[offset..<(offset + count)])
        }

        mutating func read(depth: Int) throws -> TrustCBOR {
            nodes += 1
            guard depth <= 16, nodes <= 1_024 else { throw TrustError.malformedDocument }
            let first = try take(1)[0]
            if first == 0xf6 { return .null }
            let major = first >> 5
            let additional = first & 31
            if additional == 31 {
                guard major == 4 || major == 5 else { throw TrustError.malformedDocument }
                var items: [TrustCBOR] = []
                var pairs: [Pair] = []
                while true {
                    guard offset < bytes.count else { throw TrustError.malformedDocument }
                    if bytes[offset] == 0xff { offset += 1; break }
                    guard items.count < 128, pairs.count < 128 else { throw TrustError.malformedDocument }
                    let key = try read(depth: depth + 1)
                    if major == 4 {
                        items.append(key)
                    } else {
                        guard !pairs.contains(where: { $0.key == key }) else { throw TrustError.malformedDocument }
                        pairs.append(Pair(key, try read(depth: depth + 1)))
                    }
                }
                return major == 4 ? .array(items) : .map(pairs)
            }
            let value: UInt64
            switch additional {
            case 0..<24: value = UInt64(additional)
            case 24...27:
                value = try take(1 << (additional - 24)).reduce(UInt64(0)) { $0 << 8 | UInt64($1) }
            default: throw TrustError.malformedDocument
            }
            switch major {
            case 0: return .unsigned(value)
            case 1:
                guard value <= UInt64(Int64.max) else { throw TrustError.malformedDocument }
                return .negative(-1 - Int64(value))
            case 2, 3:
                guard value <= 65_536 else { throw TrustError.malformedDocument }
                let data = Data(try take(Int(value)))
                if major == 2 { return .bytes(data) }
                guard let text = String(data: data, encoding: .utf8) else { throw TrustError.malformedDocument }
                return .text(text)
            case 4, 5:
                guard value <= 128 else { throw TrustError.malformedDocument }
                if major == 4 {
                    var items: [TrustCBOR] = []
                    for _ in 0..<value { items.append(try read(depth: depth + 1)) }
                    return .array(items)
                }
                var pairs: [Pair] = []
                for _ in 0..<value {
                    let key = try read(depth: depth + 1)
                    guard !pairs.contains(where: { $0.key == key }) else { throw TrustError.malformedDocument }
                    pairs.append(Pair(key, try read(depth: depth + 1)))
                }
                return .map(pairs)
            case 6: return .tag(value, try read(depth: depth + 1))
            default: throw TrustError.malformedDocument
            }
        }
    }
}
