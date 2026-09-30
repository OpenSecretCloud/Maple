import Foundation
import Testing
@testable import MapleSDK

@Suite struct TransportCryptoTests {
    func keys(_ byte: UInt8 = 0x44) throws -> SessionKeys {
        try TransportCrypto.derive(
            shared: Data(repeating: byte, count: 32),
            challenge: Data(repeating: 0x11, count: 32),
            client: Data(repeating: 0x22, count: 32), server: Data(repeating: 0x33, count: 32))
    }

    @Test func matchesBackendAndRustVectors() throws {
        let session = try keys()
        #expect(session.id.lowercaseHex == "f7258fb103137c612baab47ced4a5a02")
        let encrypted = try TransportCrypto.sealRequest(
            Data("vector plaintext".utf8), keys: session,
            request: Data(repeating: 0x55, count: 16))
        #expect(
            encrypted.lowercaseHex
                == "55555555555555555555555555555555671f5c411205cb00f769e6b2705052b795e91f44516fc6165e16a152e686b209")
        let fixture = Data([
            0x25, 0xa2, 0xd5, 0xed, 0x89, 0x86, 0x4b, 0xd7, 0xb5, 0xe1, 0x3c, 0x83, 0xeb, 0x49, 0xb1, 0xf3, 0x14, 0xa7,
            0x0a, 0xbf, 0x8b, 0xd7, 0xe8, 0x71, 0xb7, 0x06, 0xbb, 0x67, 0x68, 0xc9, 0xe1,
        ])
        let request = Data(repeating: 0x66, count: 16)
        #expect(
            try TransportCrypto.openResponse(fixture, keys: session, request: request, sequence: 0)
                == Data("vector response".utf8))
        #expect(throws: TransportError.authentication) {
            try TransportCrypto.openResponse(fixture, keys: session, request: request, sequence: 1)
        }
        #expect(throws: TransportError.authentication) {
            try TransportCrypto.openResponse(fixture, keys: keys(0x45), request: request, sequence: 0)
        }
        #expect(throws: TransportError.authentication) {
            try TransportCrypto.openResponse(
                fixture, keys: session, request: Data(repeating: 0x67, count: 16), sequence: 0)
        }
    }

    @Test func rejectsNoncontributoryExchange() {
        #expect(throws: TransportError.authentication) { try keys(0) }
    }

    @Test func rejectsNoncanonicalBase64() {
        #expect(Data(canonicalBase64: "AB==", count: 1) == nil)
        #expect(Data(canonicalBase64: "AA==", count: 1) == Data([0]))
        #expect(Data(canonicalBase64: "AA", count: 1) == nil)
    }
}
