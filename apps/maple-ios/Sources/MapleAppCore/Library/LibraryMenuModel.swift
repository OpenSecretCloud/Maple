import Foundation
import Observation

/// Account-owned menu snapshots are loaded before the chat scene is published.
/// Recreating a compact or split-view menu never creates a second set of pages.
@MainActor @Observable public final class LibraryMenuModel {
    public let root: LibraryModel
    public let scope: AccountScope
    public let pinned: LibraryModel
    public let recent: LibraryModel
    public private(set) var projects: [String: LibraryModel] = [:]
    public var expandedProjects: Set<String> = []
    public private(set) var isPrepared = false
    public private(set) var searchText = ""
    public var onModelCreated: (@MainActor (LibraryModel) -> Void)?
    @ObservationIgnored private let makeLibrary: @MainActor () -> LibraryModel
    @ObservationIgnored private var active = true
    @ObservationIgnored private var refreshTask: Task<Void, Never>?
    @ObservationIgnored private var searchTask: Task<Void, Never>?
    @ObservationIgnored private var searchActive = false

    public init(root: LibraryModel, makeLibrary: @escaping @MainActor () -> LibraryModel) {
        self.root = root; scope = root.scope; self.makeLibrary = makeLibrary
        pinned = makeLibrary(); recent = makeLibrary()
    }

    public func project(_ id: String) -> LibraryModel {
        if let current = projects[id] { return current }
        let model = makeLibrary()
        model.searchText = searchText
        projects[id] = model; onModelCreated?(model)
        if !active { model.invalidate() }
        return model
    }

    /// Only restored open folders are fetched at launch. Closed folders acquire
    /// their first page when opened or when the user searches the menu.
    public func prepare(isExpanded: @escaping @MainActor (String) -> Bool) async {
        guard active, !isPrepared else { return }
        await refresh(isExpanded: isExpanded)
    }

    /// One shared refresh keeps the previous complete snapshots visible and
    /// prevents overlapping cursor work from independently recreated views.
    public func refresh(isExpanded: @escaping @MainActor (String) -> Bool) async {
        guard active else { return }
        if let refreshTask { await refreshTask.value; return }
        let task = Task { @MainActor [weak self] in
            guard let self, active else { return }
            async let folders: Void = root.loadProjects()
            async let pins: Void = pinned.load(.pinned)
            async let latest: Void = recent.load(.recent)
            _ = await (folders, pins, latest)
            await searchTask?.value
            guard active, !Task.isCancelled else { return }
            let validIDs = Set(root.projects.map(\.id))
            for id in Array(projects.keys) where !validIDs.contains(id) {
                projects.removeValue(forKey: id)?.invalidate()
            }
            expandedProjects.formIntersection(validIDs)
            for item in root.projects {
                let model = project(item.id)
                if model.hasLoadedConversations || isExpanded(item.id) || (searchActive && !searchText.isEmpty) {
                    await model.load(.project(item.id))
                    guard active, !Task.isCancelled else { return }
                }
            }
            isPrepared = true
        }
        refreshTask = task
        await task.value
        refreshTask = nil
    }

    public func setSearch(_ value: String) {
        guard active else { return }
        searchText = value; pinned.searchText = value; recent.searchText = value
        searchActive = !value.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
        for model in projects.values { model.searchText = value }
        let prior = searchTask
        prior?.cancel()
        searchTask = Task { @MainActor [weak self] in
            await prior?.value
            guard let self, active else { return }
            guard !Task.isCancelled, searchActive else { return }
            for item in root.projects {
                guard active, !Task.isCancelled else { return }
                let model = project(item.id)
                if !model.hasLoadedConversations { await model.load(.project(item.id)) }
            }
        }
    }

    /// Hiding the menu stops speculative folder reads without losing its
    /// query, loaded rows or cursors. The next visible refresh resumes search.
    public func cancelSearchLoading() {
        searchActive = false; searchTask?.cancel()
    }

    public func invalidate() {
        active = false; refreshTask?.cancel(); searchTask?.cancel()
        refreshTask = nil; searchTask = nil; onModelCreated = nil
        pinned.invalidate(); recent.invalidate()
        for model in projects.values { model.invalidate() }
        projects.removeAll(); expandedProjects.removeAll(); searchText = ""; isPrepared = false; searchActive = false
    }
}
