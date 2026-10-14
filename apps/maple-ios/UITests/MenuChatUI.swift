import Foundation
import XCTest

/// Resolves only the requested UUID through real Projects/Pinned/Recent rows.
/// Paging and disclosure stay in the menu; no API reads or row substitution.
@MainActor enum MenuChatUI {
    static func find(
        _ id: String, in app: XCUIApplication, projectIDs: [String]? = nil, pageLimit: Int
    ) throws -> XCUIElement? {
        guard UUID(uuidString: id) != nil, (1...24).contains(pageLimit) else {
            throw NativeTextEntryIssue.unavailable
        }
        let root = try rootList(in: app)
        try settle(root)
        // The menu retains its scroll position after chat selection. Start each
        // exact lookup at the top before scanning its existing page owners.
        try reveal(root.buttons["library.new-chat"], in: app, owner: root, towardTop: true, limit: pageLimit)
        try settle(root)
        if let row = try scan(
            id, in: app, owner: root,
            pageControls: ["library.pinned.load-more", "library.recent.load-more"], limit: pageLimit)
        {
            return row
        }
        let candidates: [String]
        if let projectIDs {
            guard projectIDs.count <= 16, projectIDs.allSatisfy({ UUID(uuidString: $0) != nil }) else {
                throw NativeTextEntryIssue.unavailable
            }
            candidates = Array(Set(projectIDs)).sorted()
        } else {
            // Projects may load after the first root snapshot. Discover only
            // after the root scan and a real bounded return to the top.
            try reveal(root.buttons["library.new-chat"], in: app, owner: root, towardTop: true, limit: pageLimit)
            try settle(root)
            var found = Set<String>(), previous: Set<String>?, stable = 0, reachedEnd = false
            for _ in 0..<pageLimit {
                let folders = root.descendants(matching: .any).matching(
                    NSPredicate(format: "identifier BEGINSWITH %@", "library.project.")
                ).allElementsBoundByIndex
                for folder in folders {
                    let value = String(folder.identifier.dropFirst("library.project.".count))
                    if UUID(uuidString: value) != nil { found.insert(value) }
                }
                guard found.count <= 16 else { throw NativeTextEntryIssue.unavailable }
                let visible = visibleIDs(root, in: app)
                stable = visible == previous ? stable + 1 : 0
                if stable >= 2 { reachedEnd = true; break }
                previous = visible
                try scroll(in: app, owner: root, towardTop: false)
                try settle(root)
            }
            guard reachedEnd else { throw NativeTextEntryIssue.unavailable }
            candidates = found.sorted()
        }
        for projectID in candidates {
            try expand(projectID, in: app, pageLimit: pageLimit)
            if let row = try scan(
                id, in: app, owner: root,
                pageControls: ["library.project.load-more." + projectID], limit: pageLimit)
            {
                return row
            }
        }
        return nil
    }

    static func expand(_ projectID: String, in app: XCUIApplication, pageLimit: Int) throws {
        guard UUID(uuidString: projectID) != nil, (1...24).contains(pageLimit) else {
            throw NativeTextEntryIssue.unavailable
        }
        let root = try rootList(in: app)
        try reveal(root.buttons["library.new-chat"], in: app, owner: root, towardTop: true, limit: pageLimit)
        try settle(root)
        let folder = root.buttons.matching(identifier: "library.project." + projectID).firstMatch
        try reveal(folder, in: app, owner: root, towardTop: false, limit: pageLimit)
        guard let state = folder.value as? String, state == "Expanded" || state == "Collapsed" else {
            throw NativeTextEntryIssue.unavailable
        }
        var remainingScrolls = pageLimit
        if state == "Collapsed" {
            // Require the whole header and child margin inside the same clear band
            // used by the capped gesture before deciding to toggle the disclosure.
            var childRoom = false
            for _ in 0..<pageLimit {
                let viewport = try childRoomRegion(in: app, owner: root)
                var towardTop = false
                if folder.exists {
                    let frame = folder.frame
                    guard [frame.minX, frame.minY, frame.maxX, frame.maxY].allSatisfy({ $0.isFinite }),
                        frame.width > 0, frame.height > 0
                    else { throw NativeTextEntryIssue.unavailable }
                    if folder.isHittable, viewport.contains(frame), frame.maxY + 44 <= viewport.maxY {
                        childRoom = true; break
                    }
                    towardTop = frame.minY < viewport.minY
                }
                remainingScrolls -= 1
                try scroll(in: app, owner: root, towardTop: towardTop, maximumDistance: 44)
                try settle(root)
            }
            guard childRoom else { throw NativeTextEntryIssue.unavailable }
            folder.tap()
        }
        let expanded = XCTNSPredicateExpectation(
            predicate: NSPredicate { _, _ in folder.exists && folder.value as? String == "Expanded" }, object: folder)
        guard XCTWaiter.wait(for: [expanded], timeout: 15) == .completed else {
            throw NativeTextEntryIssue.unavailable
        }
        try settle(root)
        // Position the expanded children using the remaining disclosure budget.
        // The header is the stable anchor; Manage now lives in its context menu.
        // Empty projects remain valid; no conversation row is required here.
        for _ in 0...remainingScrolls {
            let viewport = try childRoomRegion(in: app, owner: root)
            guard folder.exists, folder.value as? String == "Expanded" else {
                throw NativeTextEntryIssue.unavailable
            }
            let frame = folder.frame
            guard [frame.minX, frame.minY, frame.maxX, frame.maxY].allSatisfy({ $0.isFinite }),
                frame.width > 0, frame.height > 0
            else { throw NativeTextEntryIssue.unavailable }
            if folder.isHittable, viewport.contains(frame), frame.minY <= viewport.minY + 44 {
                return
            }
            guard remainingScrolls > 0 else { throw NativeTextEntryIssue.unavailable }
            remainingScrolls -= 1
            try scroll(in: app, owner: root, towardTop: frame.minY < viewport.minY, maximumDistance: 44)
            try settle(root)
        }
    }

    private static func rootList(in app: XCUIApplication) throws -> XCUIElement {
        let root = app.collectionViews["library.sidebar-list"]
        guard root.waitForExistence(timeout: 15), app.windows.firstMatch.exists,
            root.frame.intersection(app.windows.firstMatch.frame).width >= 100,
            root.frame.intersection(app.windows.firstMatch.frame).height >= 100
        else { throw NativeTextEntryIssue.unavailable }
        return root
    }

    private static func settle(_ owner: XCUIElement) throws {
        let loaded = XCTNSPredicateExpectation(
            predicate: NSPredicate { _, _ in
                owner.exists && owner.activityIndicators.allElementsBoundByIndex.allSatisfy { !$0.exists }
            }, object: owner)
        guard XCTWaiter.wait(for: [loaded], timeout: 15) == .completed else {
            throw NativeTextEntryIssue.unavailable
        }
    }

    private static func visibleIDs(_ owner: XCUIElement, in app: XCUIApplication) -> Set<String> {
        let region = owner.frame.intersection(app.windows.firstMatch.frame)
        return Set(
            owner.descendants(matching: .any).allElementsBoundByIndex.compactMap { item in
                item.exists && item.isHittable && item.frame.intersects(region) && !item.identifier.isEmpty
                    ? item.identifier : nil
            })
    }

    private static func scan(
        _ id: String, in app: XCUIApplication, owner: XCUIElement, pageControls: [String], limit: Int
    ) throws -> XCUIElement? {
        func visibleRow() -> XCUIElement? {
            owner.buttons.matching(identifier: "library.chat." + id).allElementsBoundByIndex.first {
                $0.exists && $0.isHittable && $0.isEnabled
                    && $0.frame.intersects(owner.frame.intersection(app.windows.firstMatch.frame))
            }
        }
        for _ in 0..<limit {
            if let row = visibleRow() { return row }
            if app.staticTexts["library.error"].exists { throw NativeTextEntryIssue.unavailable }
            if let more = pageControls.map({ owner.buttons[$0].firstMatch }).first(where: {
                $0.exists && $0.isHittable && $0.isEnabled
            }) {
                let before = Set(
                    owner.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "library.chat."))
                        .allElementsBoundByIndex.map(\.identifier))
                more.tap()
                let loaded = XCTNSPredicateExpectation(
                    predicate: NSPredicate { _, _ in
                        !more.exists || app.staticTexts["library.error"].exists
                            || Set(
                                owner.buttons.matching(NSPredicate(format: "identifier BEGINSWITH %@", "library.chat."))
                                    .allElementsBoundByIndex.map(\.identifier)) != before
                    }, object: owner)
                guard XCTWaiter.wait(for: [loaded], timeout: 15) == .completed else {
                    throw NativeTextEntryIssue.unavailable
                }
                try settle(owner)
            } else {
                try scroll(in: app, owner: owner, towardTop: false)
                try settle(owner)
            }
        }
        return visibleRow()
    }

    private static func reveal(
        _ element: XCUIElement, in app: XCUIApplication, owner: XCUIElement, towardTop: Bool, limit: Int
    ) throws {
        for _ in 0..<limit {
            if element.exists, element.isHittable { return }
            try scroll(in: app, owner: owner, towardTop: towardTop)
        }
        guard element.exists, element.isHittable else { throw NativeTextEntryIssue.unavailable }
    }

    private static func scroll(
        in app: XCUIApplication, owner: XCUIElement, towardTop: Bool, maximumDistance: CGFloat? = nil
    ) throws {
        let window = app.windows.firstMatch
        guard owner.exists, window.exists else { throw NativeTextEntryIssue.unavailable }
        if let maximumDistance {
            guard maximumDistance.isFinite, maximumDistance > 0 else { throw NativeTextEntryIssue.unavailable }
            let region = try childRoomRegion(in: app, owner: owner)
            let top = CGPoint(x: region.midX, y: region.minY + region.height * 0.15)
            let bottom = CGPoint(x: region.midX, y: region.maxY - region.height * 0.15)
            let distance = min(maximumDistance, bottom.y - top.y)
            let start = towardTop ? top : bottom
            let end = CGPoint(x: start.x, y: start.y + (towardTop ? distance : -distance))
            let origin = window.coordinate(withNormalizedOffset: .zero)
            let startPoint = origin.withOffset(
                CGVector(dx: start.x - window.frame.minX, dy: start.y - window.frame.minY))
            let endPoint = origin.withOffset(
                CGVector(dx: end.x - window.frame.minX, dy: end.y - window.frame.minY))
            startPoint.press(
                forDuration: 0.05, thenDragTo: endPoint,
                withVelocity: XCUIGestureVelocity(rawValue: 100), thenHoldForDuration: 0.1)
            return
        }
        var region = owner.frame.intersection(window.frame).insetBy(dx: 16, dy: 24)
        let search = app.textFields["library.search"]
        if search.exists, search.frame.intersects(region) {
            region.size.height = max(0, search.frame.minY - region.minY - 8)
        }
        for keyboard in app.keyboards.allElementsBoundByIndex where keyboard.exists {
            let overlap = keyboard.frame.intersection(region)
            if !overlap.isEmpty { region.size.height = max(0, overlap.minY - region.minY - 16) }
        }
        guard !region.isNull, !region.isInfinite, region.width >= 44, region.height >= 100 else {
            throw NativeTextEntryIssue.unavailable
        }
        let top = CGPoint(x: region.midX, y: region.minY + region.height * 0.15)
        let bottom = CGPoint(x: region.midX, y: region.maxY - region.height * 0.15)
        let origin = window.coordinate(withNormalizedOffset: .zero)
        func point(_ p: CGPoint) -> XCUICoordinate {
            origin.withOffset(CGVector(dx: p.x - window.frame.minX, dy: p.y - window.frame.minY))
        }
        point(towardTop ? top : bottom).press(forDuration: 0.05, thenDragTo: point(towardTop ? bottom : top))
    }

    private static func childRoomRegion(in app: XCUIApplication, owner: XCUIElement) throws -> CGRect {
        let window = app.windows.firstMatch
        guard owner.exists, window.exists else { throw NativeTextEntryIssue.unavailable }
        var region = owner.frame.intersection(window.frame).insetBy(dx: 16, dy: 24)
        guard [region.minX, region.minY, region.maxX, region.maxY].allSatisfy({ $0.isFinite }),
            !region.isNull, !region.isInfinite
        else { throw NativeTextEntryIssue.unavailable }
        let initialRegion = region
        let overlays = app.navigationBars.allElementsBoundByIndex
        for overlay in overlays where overlay.exists {
            let frame = overlay.frame
            guard [frame.minX, frame.minY, frame.maxX, frame.maxY].allSatisfy({ $0.isFinite }) else {
                throw NativeTextEntryIssue.unavailable
            }
            if frame.intersects(initialRegion), !frame.isEmpty {
                let minY = max(region.minY, frame.maxY + 8)
                region.size.height = max(0, region.maxY - minY)
                region.origin.y = minY
            }
        }
        let search = app.textFields["library.search"]
        if search.exists {
            let frame = search.frame
            guard [frame.minX, frame.minY, frame.maxX, frame.maxY].allSatisfy({ $0.isFinite }) else {
                throw NativeTextEntryIssue.unavailable
            }
            if frame.intersects(region), !frame.isEmpty {
                region.size.height = max(0, frame.minY - region.minY - 8)
            }
        }
        for keyboard in app.keyboards.allElementsBoundByIndex where keyboard.exists {
            let frame = keyboard.frame
            guard [frame.minX, frame.minY, frame.maxX, frame.maxY].allSatisfy({ $0.isFinite }) else {
                throw NativeTextEntryIssue.unavailable
            }
            let overlap = frame.intersection(region)
            if !overlap.isEmpty { region.size.height = max(0, overlap.minY - region.minY - 16) }
        }
        guard !region.isNull, !region.isInfinite, region.width >= 44, region.height >= 100 else {
            throw NativeTextEntryIssue.unavailable
        }
        return region
    }
}
