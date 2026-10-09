#if os(iOS)
    import MapleAppCore
    import SwiftUI

    struct QueueReview: View {
        @Bindable var runtime: ChatRuntime
        let onEdit: () -> Void
        @Environment(\.dismiss) private var dismiss

        var body: some View {
            NavigationStack {
                List {
                    Section {
                        if runtime.isQueuePaused {
                            Text(MapleStrings.text("Queued messages are paused. Resume when you’re ready."))
                                .foregroundStyle(.secondary)
                            Button(MapleStrings.text("Resume queue")) { _ = runtime.resumeQueue() }
                                .disabled(!runtime.isOnline || runtime.isEditingQueue)
                                .accessibilityIdentifier("queue.resume")
                        } else {
                            Button(MapleStrings.text("Pause queue")) { runtime.pauseQueue() }.accessibilityIdentifier(
                                "queue.pause")
                        }
                    }
                    ForEach(runtime.queuedTurns) { turn in
                        VStack(alignment: .leading, spacing: 10) {
                            if !turn.draft.text.isEmpty { Text(turn.draft.text).lineLimit(3) }
                            if !turn.draft.images.isEmpty {
                                Text(MapleStrings.imageCount(turn.draft.images.count)).font(.footnote)
                            }
                            HStack {
                                Button(MapleStrings.text("Edit")) {
                                    if runtime.beginQueueEdit(id: turn.id) { dismiss(); onEdit() }
                                }.disabled(runtime.isEditingQueue).buttonStyle(.bordered)
                                Button(MapleStrings.text("Delete"), role: .destructive) {
                                    runtime.deleteQueuedTurn(id: turn.id)
                                }
                                .buttonStyle(.bordered)
                            }
                        }.padding(.vertical, 4)
                    }
                }
                .listStyle(.plain)
                .listRowBackground(Color.clear)
                .scrollContentBackground(.hidden)
                .background(MapleSurface.canvas)
                .navigationTitle(MapleStrings.text("Queued messages")).navigationBarTitleDisplayMode(.inline)
                .toolbar { ToolbarItem(placement: .topBarTrailing) { Button(MapleStrings.text("Done")) { dismiss() } } }
            }
        }
    }
#endif
