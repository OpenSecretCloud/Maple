/// Session-owned status shown by library rows. Server history loading does
/// not create an unread flag; a retained run's successful completion does.
public enum ConversationActivity: Sendable, Equatable {
    case idle, running, unread
}
