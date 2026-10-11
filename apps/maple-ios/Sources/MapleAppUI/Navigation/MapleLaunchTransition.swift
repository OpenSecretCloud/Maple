#if os(iOS)
    import Observation
    import SwiftUI
    import UIKit

    /// A scene-local visual handoff. It never delays account restoration or
    /// guesses the toolbar's position from a device size or safe-area constant.
    @MainActor @Observable final class MapleLaunchTransition {
        var finished = false
        var departing = false
        private(set) var destination: CGRect?
        private var ready = false
        private var reduceMotion = false

        func accountReady(_ ready: Bool, reduceMotion: Bool) {
            self.ready = ready
            self.reduceMotion = reduceMotion
            beginIfReady()
        }

        func locateLock(_ frame: CGRect) {
            guard !finished, !departing, !frame.isNull, !frame.isInfinite,
                frame.width > 0, frame.height > 0
            else { return }
            destination = frame
            beginIfReady()
        }

        func finishWithoutChat() {
            guard !finished else { return }
            finished = true
        }

        private func beginIfReady() {
            guard ready, destination != nil, !finished, !departing else { return }
            withAnimation(reduceMotion ? .easeOut(duration: 0.15) : .spring(duration: 0.38, bounce: 0.08)) {
                departing = true
            } completion: {
                self.finished = true
            }
        }
    }

    private struct MapleLaunchTransitionKey: EnvironmentKey {
        static let defaultValue: MapleLaunchTransition? = nil
    }

    extension EnvironmentValues {
        var mapleLaunchTransition: MapleLaunchTransition? {
            get { self[MapleLaunchTransitionKey.self] }
            set { self[MapleLaunchTransitionKey.self] = newValue }
        }
    }

    struct MaplePrivacyLock: View {
        @Environment(\.mapleLaunchTransition) private var launch

        var body: some View {
            Image(systemName: "lock.fill")
                .font(.caption2)
                .foregroundStyle(.secondary)
                .opacity(launch?.finished == false ? 0 : 1)
                .background(LaunchLockAnchor(launch: launch))
                .accessibilityLabel(MapleStrings.text("Private conversation"))
                .accessibilityIdentifier("chat.privacy")
        }
    }

    /// The chat is hosted by a UINavigationController on compact screens;
    /// measure the real toolbar in its window rather than crossing SwiftUI
    /// hosting boundaries with a matched-geometry preference.
    private struct LaunchLockAnchor: UIViewRepresentable {
        let launch: MapleLaunchTransition?

        func makeUIView(context: Context) -> Anchor { Anchor() }
        func updateUIView(_ view: Anchor, context: Context) {
            view.launch = launch
            view.reportAfterLayout()
        }

        final class Anchor: UIView {
            weak var launch: MapleLaunchTransition?

            override func didMoveToWindow() {
                super.didMoveToWindow()
                reportAfterLayout()
            }

            override func layoutSubviews() {
                super.layoutSubviews()
                reportAfterLayout()
            }

            func reportAfterLayout() {
                guard launch?.finished == false else { return }
                DispatchQueue.main.async { [weak self] in
                    guard let self, let window = self.window else { return }
                    self.launch?.locateLock(self.convert(self.bounds, to: window))
                }
            }
        }
    }

    struct MapleLaunchSurface: View {
        var body: some View {
            GeometryReader { geometry in
                ZStack {
                    MapleSurface.canvas
                    MapleWordmark().frame(width: 130, height: 28)
                        .position(x: geometry.size.width / 2, y: geometry.size.height / 2)
                        .accessibilityLabel(Text(verbatim: "Maple"))
                        .accessibilityIdentifier("startup.logo")
                    Image(systemName: "lock.fill").font(.caption2).foregroundStyle(.secondary)
                        .position(x: geometry.size.width / 2, y: geometry.size.height / 2 + 38)
                        .accessibilityLabel(MapleStrings.text("Private conversation"))
                        .accessibilityIdentifier("startup.lock")
                }
            }
            .ignoresSafeArea()
            .accessibilityElement(children: .contain)
            .accessibilityLabel(Text(verbatim: "Maple"))
            .accessibilityIdentifier("startup.wordmark")
        }
    }

    struct MapleLaunchOverlay: View {
        let launch: MapleLaunchTransition

        var body: some View {
            GeometryReader { geometry in
                let origin = geometry.frame(in: .global).origin
                let destination = launch.destination
                ZStack {
                    MapleSurface.canvas.opacity(launch.departing ? 0 : 1)
                    MapleWordmark().frame(width: 130, height: 28)
                        .position(x: geometry.size.width / 2, y: geometry.size.height / 2)
                        .opacity(launch.departing ? 0 : 1)
                    Image(systemName: "lock.fill").font(.caption2).foregroundStyle(.secondary)
                        .position(
                            x: launch.departing ? (destination?.midX ?? origin.x) - origin.x : geometry.size.width / 2,
                            y: launch.departing
                                ? (destination?.midY ?? origin.y) - origin.y : geometry.size.height / 2 + 38)
                }
            }
            .ignoresSafeArea()
            .allowsHitTesting(false)
            .accessibilityHidden(true)
        }
    }

    #if DEBUG
        /// Pauses only a synthetic launch so XCTest can observe the same
        /// full-scene surface and native toolbar handoff without network timing.
        struct MapleLaunchTransitionFixture: View {
            @State private var launch = MapleLaunchTransition()
            @State private var ready = false
            @Environment(\.accessibilityReduceMotion) private var reduceMotion

            var body: some View {
                Group {
                    if ready {
                        NavigationStack {
                            MapleSurface.canvas.ignoresSafeArea()
                                .toolbar {
                                    ToolbarItem(placement: .principal) { MaplePrivacyLock() }
                                }
                        }
                    } else {
                        MapleLaunchSurface()
                    }
                }
                .environment(\.mapleLaunchTransition, launch)
                .overlay {
                    if !launch.finished { MapleLaunchOverlay(launch: launch) }
                }
                .overlay(alignment: .bottom) {
                    if !ready {
                        Button("Continue") { ready = true }
                            .accessibilityIdentifier("startup.fixture.continue")
                    } else if launch.finished {
                        Text("Complete").accessibilityIdentifier("startup.fixture.complete")
                    }
                }
                .onChange(of: ready) { _, value in launch.accountReady(value, reduceMotion: reduceMotion) }
            }
        }
    #endif
#endif
