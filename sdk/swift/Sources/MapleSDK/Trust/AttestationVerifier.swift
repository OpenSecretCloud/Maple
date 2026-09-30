import CryptoKit
import Foundation
import Security

/// Authenticated Nitro fields. Nitro identity alone is insufficient: the session
/// client must also enforce its PCR policy and bind publicKey/userData to its handshake.
public struct AttestationDocument: Sendable {
    public let moduleID: String
    public let timestamp: UInt64
    public let digest: String
    public let pcrs: [Int: Data]
    public let certificate: Data
    public let caBundle: [Data]
    public let publicKey: Data?
    public let userData: Data?
    public let nonce: Data?

    init(payload: TrustCBOR) throws {
        guard let fields = payload.pairs, fields.allSatisfy({ $0.key.string != nil }),
            let moduleID = payload["module_id"]?.string, !moduleID.isEmpty, moduleID.utf8.count <= 1_024,
            let timestamp = payload["timestamp"]?.uint, timestamp > 0, timestamp <= 9_007_199_254_740_991,
            payload["digest"]?.string == "SHA384",
            let rawPCRs = payload["pcrs"]?.pairs, !rawPCRs.isEmpty, rawPCRs.count <= 32,
            let certificate = payload["certificate"]?.data, (1...1_024).contains(certificate.count),
            let bundle = payload["cabundle"]?.items, (1...16).contains(bundle.count)
        else { throw TrustError.malformedDocument }

        var pcrs: [Int: Data] = [:]
        for item in rawPCRs {
            guard let index = item.key.uint, index < 32, let bytes = item.value.data,
                [32, 48, 64].contains(bytes.count)
            else { throw TrustError.malformedDocument }
            pcrs[Int(index)] = bytes
        }
        var caBundle: [Data] = []
        for item in bundle {
            guard let cert = item.data, (1...1_024).contains(cert.count) else { throw TrustError.malformedDocument }
            caBundle.append(cert)
        }
        self.moduleID = moduleID
        self.timestamp = timestamp
        digest = "SHA384"
        self.pcrs = pcrs
        self.certificate = certificate
        self.caBundle = caBundle
        publicKey = try Self.optionalBytes(payload["public_key"])
        userData = try Self.optionalBytes(payload["user_data"])
        nonce = try Self.optionalBytes(payload["nonce"])
    }

    private static func optionalBytes(_ field: TrustCBOR?) throws -> Data? {
        guard let field, field != .null else { return nil }
        guard let bytes = field.data, bytes.count <= 1_024 else { throw TrustError.malformedDocument }
        return bytes
    }

    fileprivate init(loopbackPayload payload: TrustCBOR) throws {
        guard payload.pairs != nil else { throw TrustError.malformedDocument }
        // Deliberately unauthenticated metadata, never exposed by verify().
        moduleID = "mock-module"
        timestamp = 1
        digest = "SHA384"
        pcrs = [:]
        certificate = Data()
        caBundle = []
        publicKey = try Self.optionalBytes(payload["public_key"])
        userData = try Self.optionalBytes(payload["user_data"])
        nonce = try Self.optionalBytes(payload["nonce"])
    }
}

/// AWS Nitro COSE ES384 verification against the bundled AWS root. This API has
/// no mock switch, caller-supplied trust anchor, network chain fetch, or system-root fallback.
public struct AttestationVerifier: Sendable {
    public init() {}

    /// Transport-only local fixture boundary. The transport must select this
    /// exclusively after validating its exact HTTP loopback API endpoint, and
    /// must still check the nonce, client-key binding and server key length.
    /// This is not Nitro verification and is intentionally not public API.
    static func parseLoopbackDocument(base64: String) throws -> AttestationDocument {
        guard base64.utf8.count <= 90_000, let encoded = Data(base64Encoded: base64),
            let envelope = try TrustCBOR.decode(encoded).items, envelope.count == 4,
            let payload = envelope[2].data
        else { throw TrustError.malformedDocument }
        return try AttestationDocument(loopbackPayload: TrustCBOR.decode(payload))
    }

    public func verify(base64: String, nonce: Data, deviceTime: Date = Date()) throws -> AttestationDocument {
        guard !base64.isEmpty, base64.utf8.count <= 90_000,
            let encoded = Data(base64Encoded: base64), deviceTime.timeIntervalSince1970.isFinite
        else { throw TrustError.malformedDocument }
        let cose = try COSEDocument(encoded: encoded)
        let document = try AttestationDocument(payload: TrustCBOR.decode(cose.payload))
        guard let actualNonce = document.nonce, !nonce.isEmpty, trustEqual(actualNonce, nonce) else {
            throw TrustError.nonceMismatch
        }
        let rootURL = Bundle.module.url(forResource: "aws_nitro_root", withExtension: "der")
        guard let rootURL, let trustedRoot = try? Data(contentsOf: rootURL),
            trustEqual(document.caBundle[0], trustedRoot)
        else { throw TrustError.certificateChain }

        let orderedDER = [document.certificate] + document.caBundle.reversed()
        let certificates: [SecCertificate] = try orderedDER.map {
            guard let cert = SecCertificateCreateWithData(nil, $0 as CFData) else { throw TrustError.certificateChain }
            return cert
        }
        // Check COSE with the leaf key first so the timestamp is authenticated
        // before using it for Security.framework's independent path evaluation.
        guard let key = SecCertificateCopyKey(certificates[0]),
            let rawKey = SecKeyCopyExternalRepresentation(key, nil) as Data?
        else { throw TrustError.certificateChain }
        do {
            let publicKey = try P384.Signing.PublicKey(x963Representation: rawKey)
            let signature = try P384.Signing.ECDSASignature(rawRepresentation: cose.signature)
            guard publicKey.isValidSignature(signature, for: cose.signatureStructure) else {
                throw TrustError.documentSignature
            }
        } catch { throw TrustError.documentSignature }

        var trust: SecTrust?
        guard
            SecTrustCreateWithCertificates(certificates as CFArray, SecPolicyCreateBasicX509(), &trust)
                == errSecSuccess,
            let trust,
            SecTrustSetAnchorCertificates(trust, [certificates.last!] as CFArray) == errSecSuccess,
            SecTrustSetAnchorCertificatesOnly(trust, true) == errSecSuccess,
            SecTrustSetNetworkFetchAllowed(trust, false) == errSecSuccess
        else { throw TrustError.certificateChain }
        let enclaveTime = Date(timeIntervalSince1970: Double(document.timestamp) / 1_000)
        guard SecTrustSetVerifyDate(trust, enclaveTime as CFDate) == errSecSuccess,
            SecTrustEvaluateWithError(trust, nil),
            let evaluated = SecTrustCopyCertificateChain(trust) as? [SecCertificate],
            evaluated.count == certificates.count
        else { throw TrustError.certificateChain }
        for (actual, expected) in zip(evaluated, orderedDER) {
            guard trustEqual(SecCertificateCopyData(actual) as Data, expected) else {
                throw TrustError.certificateChain
            }
        }
        // Every certificate, including the anchor, must be valid on the device.
        // The signed enclave timestamp is used only for the clock error diagnosis.
        for (index, der) in orderedDER.enumerated() {
            let validity = try CertificateValidity(der: der)
            guard validity.isValid(at: deviceTime) else {
                throw AttestationClockSkewError(
                    deviceTime: deviceTime, enclaveTime: enclaveTime,
                    certificateIndex: index, notBefore: validity.notBefore, notAfter: validity.notAfter)
            }
        }
        return document
    }
}

struct COSEDocument {
    let protected: Data
    let payload: Data
    let signature: Data
    var signatureStructure: Data {
        TrustCBOR.array([.text("Signature1"), .bytes(protected), .bytes(Data()), .bytes(payload)]).encode()
    }

    init(encoded: Data) throws {
        var decoded = try TrustCBOR.decode(encoded)
        if case .tag(let tag, let content) = decoded {
            guard tag == 18 else { throw TrustError.malformedDocument }
            decoded = content
        }
        guard let array = decoded.items, array.count == 4,
            let protected = array[0].data, array[1].pairs?.isEmpty == true,
            let payload = array[2].data, let signature = array[3].data, signature.count == 96
        else { throw TrustError.malformedDocument }
        let header = try TrustCBOR.decode(protected)
        guard header == .map([.init(.unsigned(1), .negative(-35))]) else { throw TrustError.unsupportedAlgorithm }
        self.protected = protected
        self.payload = payload
        self.signature = signature
    }
}
