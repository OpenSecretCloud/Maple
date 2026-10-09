import Foundation

/// Physical-left navigation uses the captured scene width throughout a contact.
enum CompactMenuGesture {
    static func progress(translationX: CGFloat, width: CGFloat) -> CGFloat {
        guard translationX.isFinite, width.isFinite, width > 0 else { return 0 }
        return min(1, max(0, translationX / width))
    }

    static func shouldFinish(translationX: CGFloat, velocityX: CGFloat, width: CGFloat) -> Bool {
        guard translationX.isFinite, velocityX.isFinite, width.isFinite, width > 0,
            translationX > 65
        else { return false }
        // A deliberate reversal wins over the distance already travelled.
        if velocityX < -200 { return false }
        if velocityX > 500 { return true }
        let projectedX = translationX + velocityX * 0.2
        return progress(translationX: projectedX, width: width) >= 0.5
    }
}
