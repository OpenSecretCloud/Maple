import Foundation

/// Deterministic packing only; measurement and placement never publish view state.
struct ComposerFlowPlan {
    let frames: [CGRect?]
    let height: CGFloat

    init(sizes: [CGSize], width: CGFloat, spacing: CGFloat = 8, trailingIndices: Set<Int> = []) {
        let width = max(0, width)
        var frames: [CGRect?] = []
        var x: CGFloat = 0, y: CGFloat = 0, rowHeight: CGFloat = 0
        let trailingStart = sizes.indices.first { trailingIndices.contains($0) && sizes[$0].width > 0 }
        for (index, size) in sizes.enumerated() {
            guard size.width > 0, size.height > 0 else { frames.append(nil); continue }
            if index == trailingStart, x > 0 {
                let group = sizes.indices.filter { $0 >= index && trailingIndices.contains($0) && sizes[$0].width > 0 }
                let groupWidth =
                    group.reduce(CGFloat.zero) { $0 + sizes[$1].width }
                    + CGFloat(max(0, group.count - 1)) * spacing
                if groupWidth <= width, x + groupWidth > width {
                    x = 0; y += rowHeight + spacing; rowHeight = 0
                }
            }
            if x > 0, x + size.width > width {
                x = 0; y += rowHeight + spacing; rowHeight = 0
            }
            frames.append(CGRect(origin: CGPoint(x: x, y: y), size: size))
            x += size.width + spacing
            rowHeight = max(rowHeight, size.height)
        }
        // The trailing cluster stays together when it fits a row. Move it as a
        // unit so project, microphone and the primary action share the same edge.
        if let index = frames.lastIndex(where: { $0 != nil }), trailingIndices.contains(index),
            let frame = frames[index]
        {
            let shift = max(0, width - frame.maxX)
            for candidate in frames.indices where trailingIndices.contains(candidate) {
                guard var trailing = frames[candidate], trailing.minY == frame.minY else { continue }
                trailing.origin.x += shift; frames[candidate] = trailing
            }
        }
        self.frames = frames
        height = y + rowHeight
    }
}

/// One editor is measured in either an inline pill or above the same controls.
/// The plan never mutates focus, the draft, or a presentation's identity.
struct ComposerInputPlan {
    let frames: [CGRect?]
    let height: CGFloat

    init(
        sizes: [CGSize], editorIndex: Int, width: CGFloat, expanded: Bool,
        hiddenIndices: Set<Int> = [], trailingIndices: Set<Int> = [], spacing: CGFloat = 4
    ) {
        let width = width.isFinite ? max(0, width) : 0
        guard sizes.indices.contains(editorIndex) else { frames = []; height = 0; return }
        let visible = sizes.indices.filter {
            !hiddenIndices.contains($0) && sizes[$0].width > 0 && sizes[$0].height > 0
        }
        let controls = visible.filter { $0 != editorIndex }
        let controlWidth = controls.reduce(CGFloat.zero) { $0 + sizes[$1].width }
        let inlineWidth = width - controlWidth - CGFloat(max(0, visible.count - 1)) * spacing
        var frames = Array<CGRect?>(repeating: nil, count: sizes.count)
        if !expanded, inlineWidth >= 44 {
            let rowHeight = visible.map { sizes[$0].height }.max() ?? 0
            var x: CGFloat = 0
            for index in visible {
                let size = CGSize(
                    width: index == editorIndex ? inlineWidth : sizes[index].width, height: sizes[index].height)
                frames[index] = CGRect(x: x, y: (rowHeight - size.height) / 2, width: size.width, height: size.height)
                x += size.width + spacing
            }
            self.frames = frames; height = rowHeight
        } else {
            let controlSizes = sizes.indices.map { index in
                index == editorIndex || hiddenIndices.contains(index) ? CGSize.zero : sizes[index]
            }
            let flow = ComposerFlowPlan(
                sizes: controlSizes, width: width, spacing: spacing, trailingIndices: trailingIndices)
            let editorHeight = sizes[editorIndex].height
            let gap: CGFloat = flow.height > 0 ? 4 : 0
            frames[editorIndex] = CGRect(x: 0, y: 0, width: width, height: editorHeight)
            for index in flow.frames.indices {
                frames[index] =
                    index == editorIndex ? frames[index] : flow.frames[index]?.offsetBy(dx: 0, dy: editorHeight + gap)
            }
            self.frames = frames; height = editorHeight + gap + flow.height
        }
    }
}

#if os(iOS)
    import SwiftUI

    struct ComposerEditorLayoutKey: LayoutValueKey {
        static let defaultValue = false
    }

    enum ComposerInputRole { case attachment, editor, model, project, microphone, action }
    struct ComposerInputRoleKey: LayoutValueKey { static let defaultValue = ComposerInputRole.attachment }

    /// Changes positions and finite editor proposals while retaining the exact
    /// native input surface through expansion and keyboard dismissal.
    struct ComposerInputLayout: Layout {
        var expanded: Bool
        var layoutDirection: LayoutDirection = .leftToRight

        func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
            let width = proposal.width.flatMap { $0.isFinite ? max(0, $0) : nil } ?? 320
            let plan = measuredPlan(width: width, height: proposal.height, subviews: subviews)
            return CGSize(width: width, height: plan.height)
        }

        func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
            let plan = measuredPlan(width: bounds.width, height: bounds.height, subviews: subviews)
            for index in subviews.indices {
                guard plan.frames.indices.contains(index), let frame = plan.frames[index] else {
                    subviews[index].place(at: bounds.origin, proposal: .zero)
                    continue
                }
                subviews[index].place(
                    at: CGPoint(
                        x: layoutDirection == .rightToLeft ? bounds.maxX - frame.minX : bounds.minX + frame.minX,
                        y: bounds.minY + frame.minY),
                    anchor: layoutDirection == .rightToLeft ? .topTrailing : .topLeading,
                    proposal: ProposedViewSize(frame.size))
            }
        }

        private func measuredPlan(width: CGFloat, height: CGFloat?, subviews: Subviews) -> ComposerInputPlan {
            guard let editor = subviews.indices.first(where: { subviews[$0][ComposerInputRoleKey.self] == .editor })
            else {
                return ComposerInputPlan(sizes: [], editorIndex: 0, width: width, expanded: expanded)
            }
            let hidden = Set(
                subviews.indices.filter {
                    !expanded && [.model, .project].contains(subviews[$0][ComposerInputRoleKey.self])
                })
            let trailing = Set(
                subviews.indices.filter {
                    [.project, .microphone, .action].contains(subviews[$0][ComposerInputRoleKey.self])
                })
            var sizes = subviews.indices.map { index -> CGSize in
                guard index != editor, !hidden.contains(index) else { return .zero }
                let natural = subviews[index].sizeThatFits(.unspecified)
                guard natural.width > 0, natural.height > 0 else { return .zero }
                let controlWidth = min(width, max(44, natural.width))
                let measured = subviews[index].sizeThatFits(ProposedViewSize(width: controlWidth, height: nil))
                return CGSize(width: controlWidth, height: max(44, measured.height))
            }
            let controls = sizes.filter { $0.width > 0 && $0.height > 0 }
            let inlineWidth = width - controls.reduce(0) { $0 + $1.width } - CGFloat(controls.count) * 4
            let inline = !expanded && inlineWidth >= 44
            let chrome = ComposerFlowPlan(sizes: sizes, width: width, spacing: 4, trailingIndices: trailing).height
            let editorHeight = height.flatMap { $0.isFinite ? max(0, $0 - (inline ? 0 : chrome + 4)) : nil }
            sizes[editor] = subviews[editor].sizeThatFits(
                ProposedViewSize(width: inline ? inlineWidth : width, height: editorHeight))
            return ComposerInputPlan(
                sizes: sizes, editorIndex: editor, width: width, expanded: expanded,
                hiddenIndices: hidden, trailingIndices: trailing)
        }
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
