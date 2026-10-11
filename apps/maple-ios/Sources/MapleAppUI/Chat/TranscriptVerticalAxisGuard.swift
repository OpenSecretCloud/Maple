#if os(iOS)
    import SwiftUI
    import UIKit

    /// SwiftUI can move a vertical scroll view sideways when a nested rich block
    /// grows. Keep only the owning transcript's horizontal offset at its inset.
    struct TranscriptVerticalAxisGuard: UIViewRepresentable {
        let active: Bool
        @Environment(\.scenePhase) private var scenePhase

        func makeUIView(context: Context) -> Marker { Marker(frame: .zero) }

        func updateUIView(_ view: Marker, context: Context) {
            view.active = active && scenePhase == .active
            view.reconcile()
        }

        static func dismantleUIView(_ view: Marker, coordinator: ()) {
            view.active = false
            view.detach()
        }

        @MainActor final class Marker: UIView {
            var active = false
            private weak var scrollView: UIScrollView?
            private var observation: NSKeyValueObservation?
            private var correcting = false

            override init(frame: CGRect) {
                super.init(frame: frame)
                isUserInteractionEnabled = false
                isAccessibilityElement = false
                accessibilityElementsHidden = true
            }

            required init?(coder: NSCoder) { nil }

            override func didMoveToSuperview() { super.didMoveToSuperview(); reconcile() }
            override func didMoveToWindow() { super.didMoveToWindow(); reconcile() }
            override func layoutSubviews() { super.layoutSubviews(); reconcile() }

            private func nearestScrollView() -> UIScrollView? {
                var parent = superview
                while let view = parent {
                    if let scroll = view as? UIScrollView { return scroll }
                    parent = view.superview
                }
                return nil
            }

            func detach() {
                observation?.invalidate(); observation = nil; scrollView = nil
            }

            func reconcile() {
                guard active, let window, window.windowScene?.activationState == .foregroundActive,
                    let candidate = nearestScrollView(), candidate.window === window
                else { detach(); return }
                if scrollView !== candidate {
                    detach(); scrollView = candidate
                    observation = candidate.observe(\.contentOffset, options: [.new]) { [weak self] _, _ in
                        if Thread.isMainThread {
                            MainActor.assumeIsolated { self?.correctHorizontalOffset() }
                        } else {
                            Task { @MainActor [weak self] in self?.correctHorizontalOffset() }
                        }
                    }
                }
                correctHorizontalOffset()
            }

            private func correctHorizontalOffset() {
                guard !correcting, active, let window,
                    window.windowScene?.activationState == .foregroundActive,
                    let scrollView, scrollView.window === window, nearestScrollView() === scrollView
                else { return }
                let current = scrollView.contentOffset
                let x = -scrollView.adjustedContentInset.left
                guard current.x.isFinite, current.y.isFinite, x.isFinite, abs(current.x - x) > 0.5 else { return }
                correcting = true
                defer { correcting = false }
                // Preserve the current vertical position, including user drags.
                scrollView.setContentOffset(CGPoint(x: x, y: current.y), animated: false)
            }
        }
    }
#endif
