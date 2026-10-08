#if os(iOS)
    import MapleAppCore
    import SwiftUI

    public struct CompactReplyPlayer: View {
        @Bindable private var session: ReplyListeningSession
        public init(session: ReplyListeningSession) { self.session = session }

        public var body: some View {
            if session.cleanupRequired && !session.isPresented {
                VStack {
                    SpeechIssueText(issue: .storage)
                    Button {
                        Task { try? await session.retryCleanup() }
                    } label: {
                        Text("Retry audio cleanup", tableName: "Audio", bundle: .module)
                    }
                }.font(.callout).padding()
            } else if session.isPresented {
                VStack(spacing: 8) {
                    HStack {
                        Text("Maple", tableName: "Audio", bundle: .module).font(.headline)
                        Spacer()
                        if session.phase == .preparing {
                            ProgressView().accessibilityLabel(
                                Text("Preparing reply audio", tableName: "Audio", bundle: .module))
                        }
                        if let chatID = session.current?.chatID ?? session.pending.first?.chatID {
                            Toggle(
                                isOn: Binding(
                                    get: { session.autoplayEnabled(chatID: chatID) },
                                    set: { session.setAutoplay($0, chatID: chatID) }
                                )
                            ) { Text("Autoplay replies", tableName: "Audio", bundle: .module) }
                            .labelsHidden().accessibilityLabel(
                                Text("Autoplay replies", tableName: "Audio", bundle: .module)
                            )
                            .accessibilityIdentifier("audio.autoplay")
                        }
                        Button {
                            session.close()
                        } label: {
                            Image(systemName: "xmark")
                        }
                        .frame(minWidth: 44, minHeight: 44)
                        .accessibilityLabel(Text("Close reply player", tableName: "Audio", bundle: .module))
                        .accessibilityIdentifier("audio.close")
                    }
                    ViewThatFits(in: .horizontal) {
                        HStack(spacing: 16) {
                            transportControls
                            Spacer(minLength: 0)
                            speedControl
                        }
                        VStack(spacing: 8) {
                            transportControls
                            speedControl
                        }
                    }
                    if session.duration > 0 {
                        ProgressView(value: session.position, total: session.duration)
                            .accessibilityLabel(Text("Reply audio progress", tableName: "Audio", bundle: .module))
                            .accessibilityValue(
                                Text(
                                    "\(Duration.seconds(session.position).formatted(.units(allowed: [.minutes, .seconds], width: .abbreviated))) elapsed, \(Duration.seconds(session.duration).formatted(.units(allowed: [.minutes, .seconds], width: .abbreviated))) total",
                                    tableName: "Audio", bundle: .module))
                    }
                    if let issue = session.issue { SpeechIssueText(issue: issue).font(.footnote) }
                }
                .padding(.horizontal, 12).padding(.vertical, 8)
                .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 20))
                .accessibilityElement(children: .contain).accessibilityIdentifier("audio.player")
            }
        }
        private var transportControls: some View {
            HStack(spacing: 16) {
                Button {
                    session.skip(seconds: -10)
                } label: {
                    Image(systemName: "gobackward.10")
                }
                .frame(minWidth: 44, minHeight: 44).disabled(session.duration <= 0)
                .accessibilityLabel(Text("Go back ten seconds", tableName: "Audio", bundle: .module))
                .accessibilityIdentifier("audio.back")
                Button {
                    if session.phase == .playing { session.pause() } else { session.play() }
                } label: {
                    Image(systemName: session.phase == .playing ? "pause.fill" : "play.fill")
                }
                .frame(minWidth: 44, minHeight: 44).disabled(!session.canPlay)
                .accessibilityLabel(playPauseLabel)
                .accessibilityIdentifier("audio.play-pause")
                Button {
                    session.skip(seconds: 10)
                } label: {
                    Image(systemName: "goforward.10")
                }
                .frame(minWidth: 44, minHeight: 44).disabled(session.duration <= 0)
                .accessibilityLabel(Text("Go forward ten seconds", tableName: "Audio", bundle: .module))
                .accessibilityIdentifier("audio.forward")
            }.fixedSize()
        }
        private var playPauseLabel: Text {
            if session.phase == .playing {
                Text("Pause reply audio", tableName: "Audio", bundle: .module)
            } else {
                Text("Play reply audio", tableName: "Audio", bundle: .module)
            }
        }
        private var speedControl: some View {
            Menu {
                Picker(selection: Binding(get: { session.speed }, set: { session.setSpeed($0) })) {
                    ForEach(MaplePlaybackSpeed.allCases, id: \.self) { speed in
                        Text(
                            verbatim:
                                "\(speed.rawValue.formatted(.number.precision(.fractionLength(0...2))))×"
                        ).tag(speed)
                    }
                } label: {
                    Text("Playback speed", tableName: "Audio", bundle: .module)
                }
            } label: {
                Text(
                    verbatim:
                        "\(session.speed.rawValue.formatted(.number.precision(.fractionLength(0...2))))×"
                )
                .font(.custom("GeistMono-Regular", size: 17, relativeTo: .body)).monospacedDigit().frame(
                    minWidth: 44, minHeight: 44)
            }
            .accessibilityLabel(Text("Playback speed", tableName: "Audio", bundle: .module))
            .accessibilityIdentifier("audio.speed")
        }

    }

    private struct SpeechIssueText: View {
        let issue: SpeechIssue
        var body: some View {
            switch issue {
            case .emptyText: Text("This reply has no text to read aloud.", tableName: "Audio", bundle: .module)
            case .capacity:
                Text(
                    "This audio request exceeds the player’s current limits. Your message text is unchanged.",
                    tableName: "Audio", bundle: .module)
            case .invalidAudio:
                Text("The reply audio could not be played. Try again.", tableName: "Audio", bundle: .module)
            case .unavailable:
                Text("Reply audio is temporarily unavailable. Try again.", tableName: "Audio", bundle: .module)
            case .storage:
                Text(
                    "Temporary reply audio could not be stored or cleared. Try again.", tableName: "Audio",
                    bundle: .module)
            case .retired: Text("This listening session has ended.", tableName: "Audio", bundle: .module)
            case .dictationActive:
                Text("Finish or cancel dictation before playing reply audio.", tableName: "Audio", bundle: .module)
            case .paidPlanRequired:
                Text("Read aloud requires an eligible paid plan.", tableName: "Audio", bundle: .module)
            case .usageLimit:
                Text("Your account’s current audio allowance has been reached.", tableName: "Audio", bundle: .module)
            }
        }
    }
#endif
