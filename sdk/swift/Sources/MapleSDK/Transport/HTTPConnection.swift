import Foundation

struct HTTPConnection: Sendable {
    let status: Int
    let headers: [String: String]
    let read: @Sendable (Int) async throws -> Data
    let cancel: @Sendable () -> Void

    func collect(maximum: Int) async throws -> Data {
        var result = Data()
        do {
            while true {
                let bytes = try await read(min(4096, maximum + 1 - result.count))
                if bytes.isEmpty { return result }
                result.append(bytes)
                guard result.count <= maximum else { throw TransportError.resourceLimit }
            }
        } catch { cancel(); throw error }
    }
}

protocol HTTPNetworking: Sendable {
    func open(_ request: URLRequest) async throws -> HTTPConnection
}

final class URLSessionNetworking: NSObject, HTTPNetworking, URLSessionTaskDelegate, Sendable {
    private let session: URLSession
    override init() {
        let configuration = URLSessionConfiguration.ephemeral
        configuration.httpCookieStorage = nil
        configuration.httpShouldSetCookies = false
        configuration.urlCache = nil
        configuration.requestCachePolicy = .reloadIgnoringLocalCacheData
        configuration.timeoutIntervalForRequest = 60
        configuration.timeoutIntervalForResource = 3600
        session = URLSession(configuration: configuration)
        super.init()
    }

    func urlSession(
        _ session: URLSession, task: URLSessionTask,
        willPerformHTTPRedirection response: HTTPURLResponse, newRequest request: URLRequest,
        completionHandler: @escaping @Sendable (URLRequest?) -> Void
    ) { completionHandler(nil) }

    func open(_ request: URLRequest) async throws -> HTTPConnection {
        let (bytes, response) = try await session.bytes(for: request, delegate: self)
        guard let http = response as? HTTPURLResponse, http.url == request.url else {
            bytes.task.cancel(); throw TransportError.unauthenticatedResponse
        }
        var headers: [String: String] = [:]
        for (key, value) in http.allHeaderFields {
            guard let key = key as? String, let value = value as? String else { continue }
            let name = key.lowercased()
            if let prior = headers[name] { headers[name] = prior + ", " + value } else { headers[name] = value }
        }
        let reader = HTTPByteReader(bytes)
        return HTTPConnection(
            status: http.statusCode, headers: headers,
            read: { try await reader.read($0) }, cancel: { bytes.task.cancel() })
    }
}

/// Pull-based reads preserve backpressure instead of buffering an entire response.
private actor HTTPByteReader {
    private var iterator: URLSession.AsyncBytes.AsyncIterator
    private let task: URLSessionTask
    private var reading = false
    init(_ bytes: URLSession.AsyncBytes) { iterator = bytes.makeAsyncIterator(); task = bytes.task }
    deinit { task.cancel() }

    func read(_ count: Int) async throws -> Data {
        guard count > 0, count <= 65553, !reading else { throw TransportError.invalidFrame }
        reading = true
        defer { reading = false }
        var local = iterator
        do {
            let result = try await withTaskCancellationHandler {
                var data = Data(); data.reserveCapacity(count)
                for _ in 0..<count {
                    try Task.checkCancellation()
                    guard let byte = try await local.next() else { break }
                    data.append(byte)
                }
                return data
            } onCancel: {
                task.cancel()
            }
            iterator = local
            return result
        } catch { task.cancel(); throw error }
    }
}

actor ResponseRecordReader {
    private let connection: HTTPConnection
    private let keys: SessionKeys
    private let requestID: Data
    private var sequence: UInt64 = 0
    private var terminal = false
    private var reading = false

    init(connection: HTTPConnection, keys: SessionKeys, requestID: Data) {
        self.connection = connection; self.keys = keys; self.requestID = requestID
    }
    deinit { connection.cancel() }

    func next() async throws -> WireProtocol.Record {
        guard !terminal, !reading else { throw TransportError.postTerminal }
        reading = true
        defer { reading = false }
        do {
            let prefix = try await connection.read(4)
            guard prefix.count == 4 else { throw TransportError.truncated }
            let length = prefix.reduce(0) { $0 * 256 + Int($1) }
            guard (16...WireProtocol.maximumCiphertext).contains(length) else { throw TransportError.invalidFrame }
            let ciphertext = try await connection.read(length)
            guard ciphertext.count == length else { throw TransportError.truncated }
            let plaintext = try TransportCrypto.openResponse(
                ciphertext, keys: keys, request: requestID, sequence: sequence)
            guard sequence < UInt64.max else { throw TransportError.invalidFrame }
            sequence += 1
            let record = try WireProtocol.decode(plaintext)
            if case .end = record { try await requireEnd() }
            if case .error = record { try await requireEnd() }
            return record
        } catch { connection.cancel(); terminal = true; throw error }
    }

    private func requireEnd() async throws {
        terminal = true
        guard try await connection.read(1).isEmpty else { throw TransportError.postTerminal }
    }
    nonisolated func cancel() { connection.cancel() }
}
