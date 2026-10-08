import Foundation
import Testing
@testable import MapleAppUI

@Suite struct ComposerFlowTests {
    @Test func compactActionsWrapWithoutOverlapOrDroppingIdentity() {
        let widths: [CGFloat] = [44, 128, 44, 44, 44, 82, 44]
        let sizes = widths.map { CGSize(width: $0, height: 44) }
        let plan = ComposerFlowPlan(sizes: sizes, width: 350)
        #expect(plan.frames.count == sizes.count)
        #expect(plan.frames.compactMap { $0?.minY } == [0, 0, 0, 0, 0, 52, 52])
        #expect(plan.height == 96)
        let frames = plan.frames.compactMap { $0 }
        for (index, frame) in frames.enumerated() {
            #expect(frame.minX >= 0 && frame.maxX <= 350)
            for next in frames.dropFirst(index + 1) { #expect(!frame.intersects(next)) }
        }
    }

    @Test func accessibilityHeightAndEmptyConditionalDoNotCreatePhantomRows() {
        let plan = ComposerFlowPlan(
            sizes: [.init(width: 44, height: 44), .init(width: 300, height: 112), .zero, .init(width: 44, height: 44)],
            width: 300)
        #expect(plan.frames[0]?.origin == .zero)
        #expect(plan.frames[1]?.minY == 52)
        #expect(plan.frames[2] == nil)
        #expect(plan.frames[3]?.minY == 172)
        #expect(plan.height == 216)
        #expect(ComposerFlowPlan(sizes: [.zero], width: 300).height == 0)
    }

    @Test func exactFitAndWideTabletKeepOneRow() {
        let sizes = [CGSize(width: 44, height: 44), CGSize(width: 44, height: 44)]
        #expect(ComposerFlowPlan(sizes: sizes, width: 96).height == 44)
        #expect(ComposerFlowPlan(sizes: sizes, width: 95).height == 96)
        #expect(ComposerFlowPlan(sizes: Array(repeating: sizes[0], count: 7), width: 850).height == 44)
    }

    @Test func sendUsesTrailingRowSpaceWithoutMovingOrOverlappingOtherControls() {
        let sizes = [CGSize(width: 44, height: 44), CGSize(width: 128, height: 44), CGSize(width: 44, height: 44)]
        let oneRow = ComposerFlowPlan(sizes: sizes, width: 350, trailingIndices: [2])
        #expect(oneRow.frames[0]?.minX == 0 && oneRow.frames[1]?.minX == 52)
        #expect(oneRow.frames[2]?.maxX == 350)
        let wrapped = ComposerFlowPlan(sizes: sizes, width: 200, trailingIndices: [2])
        #expect(wrapped.frames[2]?.minY == 52 && wrapped.frames[2]?.maxX == 200)
        #expect(wrapped.height == 96)
    }

}
