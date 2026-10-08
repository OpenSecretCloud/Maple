import Foundation

/// Deterministic packing only; measurement and placement never publish view state.
struct ComposerFlowPlan {
    let frames: [CGRect?]
    let height: CGFloat

    init(sizes: [CGSize], width: CGFloat, spacing: CGFloat = 8, trailingIndices: Set<Int> = []) {
        let width = max(0, width)
        var frames: [CGRect?] = []
        var x: CGFloat = 0, y: CGFloat = 0, rowHeight: CGFloat = 0
        for size in sizes {
            guard size.width > 0, size.height > 0 else { frames.append(nil); continue }
            if x > 0, x + size.width > width {
                x = 0; y += rowHeight + spacing; rowHeight = 0
            }
            frames.append(CGRect(origin: CGPoint(x: x, y: y), size: size))
            x += size.width + spacing
            rowHeight = max(rowHeight, size.height)
        }
        // Only the final visible action may consume the remainder of its row.
        // This preserves the preceding actions and logical order without overlap.
        if let index = frames.lastIndex(where: { $0 != nil }), trailingIndices.contains(index),
            var frame = frames[index]
        {
            frame.origin.x = max(frame.minX, width - frame.width)
            frames[index] = frame
        }
        self.frames = frames
        height = y + rowHeight
    }
}

#if os(iOS)
    import SwiftUI

    struct ComposerEditorLayoutKey: LayoutValueKey {
        static let defaultValue = false
    }

    /// Reserve the existing chrome at its measured size before giving the native
    /// scrolling editor its share of the keyboard-reduced viewport. No owners or
    /// view state change during measurement or placement.
    struct ComposerVerticalLayout: Layout {
        var maximumHeight: CGFloat
        var layoutDirection: LayoutDirection = .leftToRight
        var spacing: CGFloat = 10

        func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
            let width = availableWidth(proposal.width, subviews: subviews)
            let sizes = measuredSizes(width: width, height: heightLimit(proposal.height), subviews: subviews)
            let visible = sizes.filter { $0.height > 0 }
            return CGSize(
                width: width,
                height: visible.reduce(0) { $0 + $1.height } + CGFloat(max(0, visible.count - 1)) * spacing)
        }

        func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
            let sizes = measuredSizes(width: bounds.width, height: heightLimit(bounds.height), subviews: subviews)
            var y = bounds.minY, placedVisible = false
            for (index, size) in sizes.enumerated() {
                if size.height > 0, placedVisible { y += spacing }
                subviews[index].place(
                    at: CGPoint(x: layoutDirection == .rightToLeft ? bounds.maxX : bounds.minX, y: y),
                    anchor: layoutDirection == .rightToLeft ? .topTrailing : .topLeading,
                    proposal: ProposedViewSize(size))
                if size.height > 0 { y += size.height; placedVisible = true }
            }
        }

        private func measuredSizes(width: CGFloat, height: CGFloat, subviews: Subviews) -> [CGSize] {
            var sizes = subviews.map { $0.sizeThatFits(ProposedViewSize(width: width, height: nil)) }
            guard let editor = subviews.indices.first(where: { subviews[$0][ComposerEditorLayoutKey.self] }) else {
                return sizes
            }
            let visible = sizes.indices.filter { sizes[$0].height > 0 }
            let chrome = visible.filter { $0 != editor }.reduce(CGFloat.zero) { $0 + sizes[$1].height }
            let gaps = CGFloat(max(0, visible.count - 1)) * spacing
            // The editor subview includes its existing padding and border.
            sizes[editor] = subviews[editor].sizeThatFits(
                ProposedViewSize(width: width, height: max(0, height - chrome - gaps)))
            return sizes
        }

        private func heightLimit(_ proposed: CGFloat?) -> CGFloat {
            let limit = maximumHeight.isFinite ? max(0, maximumHeight) : 0
            guard let proposed, proposed.isFinite else { return limit }
            return min(limit, max(0, proposed))
        }

        private func availableWidth(_ proposed: CGFloat?, subviews: Subviews) -> CGFloat {
            if let proposed, proposed.isFinite { return max(0, proposed) }
            return subviews.map { $0.sizeThatFits(.unspecified).width }.filter { $0.isFinite }.max() ?? 0
        }
    }

    struct ComposerTrailingControlKey: LayoutValueKey {
        static let defaultValue = false
    }

    /// Each action has exactly one SwiftUI identity, including picker/presentation
    /// state. Width proposals wrap labels and rows without stateful layout feedback.
    struct ComposerControlFlow: Layout {
        var layoutDirection: LayoutDirection = .leftToRight

        func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
            let width = availableWidth(proposal.width, subviews: subviews)
            let plan = ComposerFlowPlan(
                sizes: sizes(width: width, subviews: subviews), width: width,
                trailingIndices: trailingIndices(subviews))
            return CGSize(width: width, height: plan.height)
        }

        func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
            let plan = ComposerFlowPlan(
                sizes: sizes(width: bounds.width, subviews: subviews), width: bounds.width,
                trailingIndices: trailingIndices(subviews))
            for (index, frame) in plan.frames.enumerated() {
                guard let frame else { continue }
                let x = layoutDirection == .rightToLeft ? bounds.maxX - frame.minX : bounds.minX + frame.minX
                subviews[index].place(
                    at: CGPoint(x: x, y: bounds.minY + frame.minY),
                    anchor: layoutDirection == .rightToLeft ? .topTrailing : .topLeading,
                    proposal: ProposedViewSize(frame.size))
            }
        }

        private func trailingIndices(_ subviews: Subviews) -> Set<Int> {
            Set(subviews.indices.filter { subviews[$0][ComposerTrailingControlKey.self] })
        }

        private func availableWidth(_ proposed: CGFloat?, subviews: Subviews) -> CGFloat {
            if let proposed, proposed.isFinite { return max(44, proposed) }
            let natural = subviews.map { $0.sizeThatFits(.unspecified) }.filter { $0.width > 0 && $0.height > 0 }
            return max(44, natural.reduce(0) { $0 + max(44, $1.width) } + CGFloat(max(0, natural.count - 1)) * 8)
        }

        private func sizes(width: CGFloat, subviews: Subviews) -> [CGSize] {
            subviews.map { view in
                let natural = view.sizeThatFits(.unspecified)
                guard natural.width > 0, natural.height > 0 else { return .zero }
                let proposed = min(max(44, width), max(44, natural.width))
                let measured = view.sizeThatFits(ProposedViewSize(width: proposed, height: nil))
                return CGSize(width: proposed, height: max(44, measured.height))
            }
        }
    }
#endif
