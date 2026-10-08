#if os(iOS) && DEBUG
    import Foundation
    import Network
    import SwiftUI
    import UIKit

    struct EquationDiagnosticView: View {
        @State private var status = "Not run"
        @State private var images: [Data] = []
        @State private var running = false
        @State private var windowHost = EquationLifecycleWindowHost(frame: .zero)
        var body: some View {
            VStack(alignment: .leading) {
                Button("Run equation checks") { running = true }
                    .accessibilityIdentifier("renderer.equation-checks").disabled(running)
                Text(status).accessibilityIdentifier("renderer.equation-status")
                ForEach(images.indices, id: \.self) { index in
                    if let image = UIImage(data: images[index]) {
                        Image(uiImage: image).resizable().scaledToFit().frame(maxHeight: 70)
                            .accessibilityLabel("Equation diagnostic \(index + 1)")
                    }
                }
            }
            .background {
                EquationLifecycleWindowAnchor(host: windowHost)
                    .frame(width: 0, height: 0)
                    .allowsHitTesting(false)
                    .accessibilityHidden(true)
            }
            .task(id: running) {
                guard running else { return }
                defer { running = false }
                let canary = EquationLoopbackCanary()
                defer { canary.stop() }
                do {
                    status = "Starting control"
                    let endpoint = try await canary.start()
                    try canary.startControl(endpoint: endpoint)
                    for _ in 0..<30
                    where !canary.controlCompleted
                        || canary.connections == 0 || canary.receivedConnections == 0
                    {
                        try await Task.sleep(for: .milliseconds(10))
                    }
                    guard canary.controlCompleted, canary.connections == 1,
                        canary.receivedConnections == 1, canary.requestKinds == 1
                    else {
                        status =
                            "Control failed; connections \(canary.connections)"
                            + "; received \(canary.receivedConnections); kinds \(canary.requestKinds)"
                            + "; control closed \(canary.controlCompleted ? 1 : 0)"
                        return
                    }
                    canary.resetProbeObservations()
                    status = "Checking containment"
                    let policy = EquationRenderer(host: windowHost)
                    let completed = try await policy.exerciseContainmentForTesting(endpoint: endpoint)
                    try await Task.sleep(for: .milliseconds(500))
                    guard completed, canary.connections == 0 else {
                        status =
                            "Containment failed; completed \(completed ? 1 : 0)"
                            + "; connections \(canary.connections)"
                            + "; received \(canary.receivedConnections); kinds \(canary.requestKinds)"
                        return
                    }
                    status = "Checking equation pixels"
                    let source = Self.formulas.enumerated().map { index, latex in
                        MessageFormula(
                            id: String(index), latex: latex, original: latex, display: true, isWithinBudget: true)
                    }
                    let rendered = try await EquationRenderer(host: windowHost).render(source)
                    images = source.compactMap { rendered[$0.id]?.png }
                    let count = images.filter(Self.hasVisibleGlyphs).count
                    status = "Equations \(count)/6; network blocked"
                } catch { status = Task.isCancelled ? "Cancelled" : status + " failed" }
            }
        }

        static let formulas = [
            #"\frac{1}{2}+\sqrt{x^2+1}"#, #"\operatorname{argmax}_{x} f(x)"#,
            #"\Bigl(\frac{a}{b}\Bigr)"#, #"\boxed{E=mc^2}"#,
            #"\newcommand{\RR}{\mathbb{R}}\RR^2"#, #"E=mc^2\tag{1}"#,
        ]

        static func hasVisibleGlyphs(_ data: Data) -> Bool {
            guard let image = UIImage(data: data)?.cgImage else { return false }
            let width = image.width, height = image.height
            guard width > 0, height > 0, width * height <= 4_194_304 else { return false }
            var pixels = [UInt8](repeating: 0, count: width * height * 4)
            let drawn = pixels.withUnsafeMutableBytes { buffer -> Bool in
                guard
                    let context = CGContext(
                        data: buffer.baseAddress, width: width, height: height,
                        bitsPerComponent: 8, bytesPerRow: width * 4, space: CGColorSpaceCreateDeviceRGB(),
                        bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)
                else { return false }
                context.draw(image, in: CGRect(x: 0, y: 0, width: width, height: height))
                return true
            }
            guard drawn else { return false }
            var ink = 0
            var touchesEdge = false
            for offset in stride(from: 0, to: pixels.count, by: 4) {
                if pixels[offset + 3] > 32, pixels[offset] < 128, pixels[offset + 1] < 128, pixels[offset + 2] < 128 {
                    ink += 1
                    let index = offset / 4
                    let x = index % width, y = index / width
                    if x == 0 || y == 0 || x == width - 1 || y == height - 1 { touchesEdge = true }
                }
            }
            // The renderer includes padding. Ink at the raster edge indicates
            // stale font metrics or clipped output, not a complete equation.
            return ink > 5 && ink < width * height * 3 / 4 && !touchesEdge
        }
    }

    @MainActor
    private final class EquationLoopbackCanary {
        enum Failure: Error { case unavailable, timeout, stopped }
        private var listener: NWListener?
        private var ready: CheckedContinuation<URL, Error>?
        private var timer: Task<Void, Never>?
        private let queue = DispatchQueue(label: "maple.synthetic-equation-canary")
        var connections = 0
        private(set) var receivedConnections = 0
        private(set) var requestKinds: UInt32 = 0
        private(set) var controlCompleted = false
        private var controlConnection: NWConnection?
        private var controlStarted = false
        private var controlResponseValid = false
        nonisolated private static let response = Data(
            "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK".utf8)

        // One DEBUG positive-control connection, without a session pool or retry.
        // Its valid response and final cancellation must precede the probe reset.
        func startControl(endpoint: URL) throws {
            guard controlConnection == nil, endpoint.scheme == "http", endpoint.host == "127.0.0.1",
                let port = endpoint.port, (1...65535).contains(port),
                endpoint.user == nil, endpoint.password == nil, endpoint.query == nil, endpoint.fragment == nil,
                endpoint.path.isEmpty || endpoint.path == "/",
                let networkPort = NWEndpoint.Port(rawValue: UInt16(port))
            else { throw Failure.unavailable }
            let connection = NWConnection(host: .ipv4(.loopback), port: networkPort, using: .tcp)
            controlConnection = connection
            connection.stateUpdateHandler = { [weak self] state in
                switch state {
                case .ready:
                    Task { @MainActor [weak self] in
                        guard let self, self.controlConnection === connection, !self.controlStarted else { return }
                        self.controlStarted = true
                        let request = Data(
                            "GET /control HTTP/1.1\r\nHost: 127.0.0.1:\(port)\r\nConnection: close\r\n\r\n".utf8)
                        connection.send(
                            content: request,
                            completion: .contentProcessed { error in
                                guard error == nil else { connection.cancel(); return }
                                connection.receive(
                                    minimumIncompleteLength: Self.response.count, maximumLength: 4096
                                ) { [weak self] data, _, _, error in
                                    let valid = error == nil && data == Self.response
                                    Task { @MainActor [weak self] in
                                        guard let self, self.controlConnection === connection else {
                                            connection.cancel(); return
                                        }
                                        self.controlResponseValid = valid
                                        connection.cancel()
                                    }
                                }
                            })
                    }
                case .cancelled:
                    connection.stateUpdateHandler = nil
                    Task { @MainActor [weak self] in
                        guard let self, self.controlConnection === connection else { return }
                        self.controlCompleted = self.controlResponseValid
                        self.controlConnection = nil
                    }
                case .failed: connection.cancel()
                default: break
                }
            }
            connection.start(queue: queue)
        }

        func resetProbeObservations() {
            connections = 0
            receivedConnections = 0
            requestKinds = 0
        }

        // DEBUG canary only: fixed path categories, never captured bytes or URLs.
        // Bits0...15: control/fetch/https/xhr/websocket/image/relative-image/
        // stylesheet/import/font/frame/beacon/form/popup/download/navigation.
        // Bits16...18: TLS record/no payload/unrecognized. A still-pending
        // receive increments neither receivedConnections nor requestKinds.
        nonisolated private static func requestKind(_ data: Data?) -> UInt32 {
            guard let data, !data.isEmpty else { return 1 << 17 }
            guard data.count <= 4096 else { return 1 << 18 }
            let prefix = data.prefix(3)
            if prefix.first == 0x16, prefix.dropFirst().first == 0x03 { return 1 << 16 }
            guard let request = String(data: data, encoding: .utf8)?.components(separatedBy: "\r\n").first
            else { return 1 << 18 }
            let fields = request.split(separator: " ", omittingEmptySubsequences: false)
            guard fields.count == 3, fields[0] == "GET" || fields[0] == "POST",
                fields[2] == "HTTP/1.1" || fields[2] == "HTTP/1.0"
            else { return 1 << 18 }
            switch fields[1] {
            case "/control": return 1
            case "/fetch": return 1 << 1
            case "/https": return 1 << 2
            case "/xhr": return 1 << 3
            case "/websocket": return 1 << 4
            case "/image": return 1 << 5
            case "/relative-image": return 1 << 6
            case "/stylesheet": return 1 << 7
            case "/import": return 1 << 8
            case "/font": return 1 << 9
            case "/frame": return 1 << 10
            case "/beacon": return 1 << 11
            case "/form": return 1 << 12
            case "/popup": return 1 << 13
            case "/download": return 1 << 14
            case "/navigation": return 1 << 15
            default: return 1 << 18
            }
        }

        func start() async throws -> URL {
            let parameters = NWParameters.tcp
            parameters.requiredLocalEndpoint = .hostPort(host: .ipv4(.loopback), port: .any)
            let listener = try NWListener(using: parameters)
            self.listener = listener
            listener.newConnectionHandler = { [weak self, queue] connection in
                Task { @MainActor in self?.connections += 1 }
                connection.start(queue: queue)
                connection.receive(minimumIncompleteLength: 1, maximumLength: 4096) { [weak self] data, _, _, _ in
                    let kind = Self.requestKind(data)
                    Task { @MainActor in
                        guard let self else { return }
                        self.receivedConnections = min(32, self.receivedConnections + 1)
                        self.requestKinds |= kind
                    }
                    connection.send(
                        content: Self.response,
                        completion: .contentProcessed { _ in connection.cancel() })
                }
            }
            listener.stateUpdateHandler = { [weak self, weak listener] state in
                Task { @MainActor in
                    guard let self else { return }
                    if case .ready = state, let port = listener?.port {
                        let endpoint = URL(string: "http://127.0.0.1:\(port.rawValue)")!
                        self.ready?.resume(returning: endpoint); self.ready = nil; self.timer?.cancel()
                    } else if case .failed = state {
                        self.stop()
                    }
                }
            }
            return try await withTaskCancellationHandler {
                try await withCheckedThrowingContinuation { continuation in
                    ready = continuation
                    timer = Task { @MainActor [weak self] in
                        do { try await Task.sleep(for: .seconds(3)) } catch { return }
                        self?.stop()
                    }
                    listener.start(queue: queue)
                }
            } onCancel: {
                Task { @MainActor [weak self] in self?.stop() }
            }
        }
        func stop() {
            controlConnection?.cancel(); controlConnection = nil
            timer?.cancel(); timer = nil
            listener?.cancel(); listener = nil
            ready?.resume(throwing: Failure.stopped); ready = nil
        }
    }
#endif
