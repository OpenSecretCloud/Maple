#if os(iOS)
    import SwiftUI
    import UIKit

    /// Adds outside-tap dismissal to a native sheet without enabling the controls
    /// behind it. UIKit still owns the sheet's dimming, sizing and drag gestures.
    struct SheetOutsideTapDismissal: UIViewControllerRepresentable {
        let onDismiss: () -> Void

        func makeUIViewController(context: Context) -> OutsideTapController {
            OutsideTapController(onDismiss: onDismiss)
        }

        func updateUIViewController(_ controller: OutsideTapController, context: Context) {
            controller.onDismiss = onDismiss
            controller.installRecognizer()
        }

        static func dismantleUIViewController(_ controller: OutsideTapController, coordinator: ()) {
            controller.removeRecognizer()
        }

        final class OutsideTapController: UIViewController, UIGestureRecognizerDelegate {
            var onDismiss: () -> Void
            private weak var observedPresentation: UIPresentationController?
            private var outsideTap: UITapGestureRecognizer?

            init(onDismiss: @escaping () -> Void) {
                self.onDismiss = onDismiss
                super.init(nibName: nil, bundle: nil)
            }

            @available(*, unavailable)
            required init?(coder: NSCoder) { fatalError("init(coder:) is unavailable") }

            override func loadView() {
                let observer = UIView()
                observer.backgroundColor = .clear
                observer.isUserInteractionEnabled = false
                view = observer
            }

            override func viewDidAppear(_ animated: Bool) {
                super.viewDidAppear(animated)
                installRecognizer()
            }

            override func viewDidLayoutSubviews() {
                super.viewDidLayoutSubviews()
                installRecognizer()
            }

            override func viewWillDisappear(_ animated: Bool) {
                super.viewWillDisappear(animated)
                removeRecognizer()
            }

            func installRecognizer() {
                guard viewIfLoaded?.window != nil else { return }
                var candidate: UIViewController? = self
                while let controller = candidate {
                    if controller.presentingViewController != nil,
                        let presentation = controller.presentationController,
                        let container = presentation.containerView
                    {
                        if observedPresentation === presentation, outsideTap?.view === container { return }
                        removeRecognizer()
                        let tap = UITapGestureRecognizer(target: self, action: #selector(dismissFromOutside(_:)))
                        tap.delegate = self
                        tap.cancelsTouchesInView = false
                        container.addGestureRecognizer(tap)
                        observedPresentation = presentation
                        outsideTap = tap
                        return
                    }
                    candidate = controller.parent
                }
            }

            func removeRecognizer() {
                if let outsideTap { outsideTap.view?.removeGestureRecognizer(outsideTap) }
                outsideTap = nil
                observedPresentation = nil
            }

            func gestureRecognizer(_ gestureRecognizer: UIGestureRecognizer, shouldReceive touch: UITouch) -> Bool {
                guard let presentation = observedPresentation,
                    let container = presentation.containerView, let presented = presentation.presentedView,
                    presentation.presentedViewController.presentedViewController == nil,
                    !presentation.presentedViewController.isBeingDismissed
                else { return false }
                // Native sheet controls can extend beyond the content bounds.
                // Their view ownership still identifies them as sheet taps.
                if touch.view?.isDescendant(of: presented) == true { return false }
                let point = presented.convert(touch.location(in: container), from: container)
                return !presented.bounds.contains(point)
            }

            func gestureRecognizer(
                _ gestureRecognizer: UIGestureRecognizer,
                shouldRecognizeSimultaneouslyWith otherGestureRecognizer: UIGestureRecognizer
            ) -> Bool {
                true
            }

            @objc private func dismissFromOutside(_ gesture: UITapGestureRecognizer) {
                guard gesture.state == .ended,
                    let presentation = observedPresentation,
                    presentation.presentedViewController.presentedViewController == nil,
                    !presentation.presentedViewController.isBeingDismissed
                else { return }
                onDismiss()
            }
        }
    }
#endif
