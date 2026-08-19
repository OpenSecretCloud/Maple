# Agent Mode v1 paging and synchronization

Status: selected v1 design. Implementation and validation are in progress.

## Decision

Agent Mode v1 uses count-bounded, cursor-based pages of Goose's native
persisted `Message` records.

- One page record is one Goose message row: its storage key, role, optional
  logical message ID, creation time, projection metadata, and complete
  content-block array.
- A record can contain text, thinking, tool requests, tool responses,
  permissions, notices, and errors. Those blocks are not split into synthetic
  storage rows for paging.
- The default page size is 25 records and the v1 maximum is 50 records.
- Pages walk from the newest history toward older history. The frontend keeps
  the accumulated records in chronological order.
- Session summaries are paged independently with the same count-first model.

This deliberately copies Maple Chat's proven scrollback behavior without
copying its storage schema. Chat stores several kinds of flat conversation
items; Goose stores a richer role message whose content array can contain
several presentation blocks. Both are legitimate page units for their own
authoritative stores.

There is no complete-turn page rule, no client-provided byte budget, and no
Maple-owned duplicate history journal. Transport frame limits and per-record
projection limits remain mandatory safety boundaries, but they are not the
product pagination model.

## Authority and shared path

Goose remains the sole durable authority for committed Agent history. The host
Maple installation remains the sole execution and storage authority.

```text
Goose SQLite message pager
          |
          v
Maple Agent history service
   |                    |
   v                    v
local Tauri adapter     authenticated Iroh adapter
   |                    |
   +---------+----------+
             v
       shared Agent UI
```

The embedded and remote adapters must call the same host-side history service.
The remote implementation must not load the whole conversation and slice it,
and Maple must not query Goose's private SQLite schema directly.

## Goose message page

Goose should expose a public storage-native API equivalent to:

```rust
pub struct ConversationMessagePageQuery {
    pub before: Option<ConversationMessageCursor>,
    pub page_size: usize,
}

pub struct ConversationMessagePage {
    /// Newest first, matching the storage query and Maple Chat's API.
    pub records: Vec<ConversationMessageRecord>,
    pub next_cursor: Option<ConversationMessageCursor>,
    pub history_revision: u64,
}

pub struct ConversationMessageRecord {
    /// The private SQLite row tie-breaker used to derive an opaque Maple record
    /// identity. This is distinct from Message::id, which is a logical ID.
    pub row_id: i64,
    pub message: Message,
}

impl SessionManager {
    pub async fn get_conversation_message_page(
        &self,
        session_id: &str,
        query: ConversationMessagePageQuery,
    ) -> Result<ConversationMessagePage>;
}
```

The storage query is a reverse keyset read over the total order
`(created_timestamp, row_id)`:

```sql
SELECT ...
FROM messages
WHERE session_id = ?
  AND (
    created_timestamp < ?
    OR (created_timestamp = ? AND id < ?)
  )
ORDER BY created_timestamp DESC, id DESC
LIMIT page_size + 1
```

The implementation returns the selected rows newest first and deserializes no
more than `page_size + 1` rows. The client reverses each page before prepending
it to its chronological in-memory window, matching Maple Chat.

The cursor is typed inside Goose and opaque outside the host. It binds at least:

- session identity;
- history revision;
- `created_timestamp`; and
- the row-ID tie breaker.

Appending a new message does not invalidate an older-history cursor.
Replacement, truncation, deletion, or in-place mutation advances the history
revision in the same SQLite transaction and makes an old cursor fail with a
typed cursor-invalidated result. `replace_conversation` must never silently
reuse a pre-rewrite cursor just because replacement rows happen to have the
same message IDs.

The messages table needs a composite index on
`(session_id, created_timestamp DESC, id DESC)`.

## Maple wire projection

Maple must not expose arbitrary provider metadata, raw image bytes, credentials,
or unbounded tool results merely because they exist inside a Goose message.
The host projects each selected Goose row into one safe Maple record:

```rust
pub struct AgentHistoryRecord {
    /// Stable for this history revision and storage row; not a claim that the
    /// optional Goose logical message ID is database-unique.
    pub record_id: String,
    pub role: AgentHistoryRole,
    pub created_ms: u64,
    /// Complete safe presentation projection for this one Goose record.
    pub items: Vec<AgentTimelineItem>,
}

pub struct AgentHistoryPage {
    pub records: Vec<AgentHistoryRecord>,
    pub next_cursor: Option<String>,
    pub history_revision: u64,
    /// Present only with an authoritative absolute live suffix and matching
    /// journal cut established by the attach coordinator.
    pub live_items: Option<Vec<AgentTimelineItem>>,
    pub through_event_cursor: Option<AgentEventCursor>,
}
```

One `AgentHistoryRecord` consumes one requested record even when it contains
several timeline items. The response validator therefore bounds `records.len()`
against the request limit rather than bounding the number of projected cards.

The cursor is not an authorization capability. Goose binds it to the session
and history revision; Maple resolves the account-scoped store and execution
target from the authenticated handle and request envelope before passing it to
Goose. Cursor fields can only narrow the explicitly authorized session query
and can never override that account, target, operation, or session scope.

The Iroh operation uses the bulk lane. Every request and response remains bound
to the authenticated endpoint, exact pairing incarnation, execution target,
and current connection stamp. The host revalidates that admission immediately
before storage access and before disclosure.

The existing one-MiB frame ceiling remains. A single safe record that cannot
fit returns a typed `HistoryRecordTooLarge`; the host never fetches all history,
silently truncates a tool result, or loops with an unchanged cursor.

## Page composition

Arbitrary message boundaries are legal.

- A tool request can be on an older page and its response on a newer page.
  Their stable tool ID lets the accumulated projection enrich one card without
  duplicating it.
- A page can begin in the middle of an assistant turn. Prepending older records
  repairs turn grouping without changing the stable render identity of already
  visible cards.
- A permission request and its resolving response can cross pages. Historical
  correlation improves its displayed state, while the host's live pending-
  permission registry remains the sole authority for whether an action is
  currently answerable.
- Repeated reasoning stored across split provider messages must retain today's
  safe content and stable identities. It may temporarily group differently at
  an arbitrary page boundary and settle when older records are prepended. That
  does not justify loading a complete turn, adding unbounded projection state
  to the cursor, or splitting a native Goose message into new storage records.
- Agent-only content stays filtered at the host authority.

Concatenating all pages must preserve every eligible safe content block in
storage order with stable record and tool identities. Cross-record card
enrichment should converge as older pages arrive. Byte-for-byte reproduction
of every whole-conversation coalescing heuristic is not a v1 requirement when
it would require a different storage schema or synthetic page units.

## Live synchronization is a separate cursor

History paging and live resume solve different problems and must not share one
cursor.

Committed history uses the Goose history cursor. Live events use a host-owned,
account-and-execution-target-scoped journal epoch and one monotonically
increasing sequence across that stream. The sequence is deliberately not
per-session: events for tasks A and B can interleave without creating false
gaps.

A plain history page does not claim a live checkpoint. A synchronized attach
uses the account-scoped event coordinator instead:

1. The coordinator drains all earlier publishes, captures an absolute safe live
   suffix for the selected session at journal cut C0, and registers a paused,
   bounded subscriber at C0.
2. The host reads one bounded Goose head page outside the event actor.
3. It replays account events `(C0, C1]` to a second coordinator barrier C1.
4. The client installs `head -> absolute live suffix at C0 -> replay to C1`,
   then resumes the already-registered subscriber.

The page exposes `live_items` and `through_event_cursor` only as a pair for that
completed protocol. `Some([])` is an authoritative empty live suffix; both
fields absent means ordinary committed-history paging. A paused-buffer overflow,
journal gap, owner change, or failed revalidation returns `HeadReloadRequired`
without ever falling back to a whole-conversation snapshot.

The host keeps a bounded durable event/outcome journal so a phone can resume
after ordinary backgrounding without loading history again:

1. reconnect and present the last event cursor;
2. replay missed events in sequence;
3. continue the live subscription; and
4. leave previously loaded history pages untouched.

If the cursor is from an old host epoch or older than retained replay, the host
returns `HeadReloadRequired`. The client reloads only the newest bounded message
page and resumes events from its new watermark. It never falls back to a whole-
conversation snapshot.

Event sequences deduplicate replayed append deltas. Stable timeline item IDs
alone are insufficient because applying the same `merge: append` delta twice
would duplicate text.

`HistoryReplaced` advances the Goose history revision, invalidates committed
pages for that session, and triggers one bounded head reload. It does not erase
or regress the current live suffix.

## Frontend behavior copied from Maple Chat

Agent Mode reuses the mature Chat scrollback mechanics:

- a persistent top sentinel with a 100-pixel loading margin;
- explicit upward wheel, touch, scrollbar, or keyboard intent before loading;
- one page per gesture and at most one queued follow-up;
- no mount-time or momentum-driven page cascade;
- capture of the first visible semantic anchor before prepend;
- anchor-and-offset restoration with a scroll-height fallback;
- stable ID deduplication and cursor-progress checks; and
- projection/session leases so delayed A -> B -> A work cannot mutate the
  wrong timeline.

State is maintained per Agent session for loaded records, the next older cursor,
history revision, load generation, and live suffix. The journal cursor belongs
to the authenticated account-and-target subscription and is shared across its
session routes. It survives a same-owner connection-generation refresh and is
reset when the account or execution-target lineage changes. Inactive sessions
can therefore keep receiving bounded live updates without forcing the selected
session to reload.

Session selection loads one newest page. Run completion and `HistoryReplaced`
reconcile one newest page. The product UI stops using unbounded `loadSession`
and `listSessions`; those may remain temporarily only as internal compatibility
methods while migration tests are being converted.

## Required proof

### Goose storage

- 101 rows with the same timestamp page 25 at a time without gaps or duplicates.
- A multi-content message remains one indivisible record and consumes one slot.
- Insert-at-head between older-page requests does not invalidate or duplicate.
- Replace, truncate, delete, and in-place mutation atomically invalidate an old
  cursor.
- Wrong-session and malformed cursors fail before returning data.
- Query-plan and instrumentation prove at most `limit + 1` rows are fetched and
  the composite index is used.
- A 10,000-message session has bounded decoded rows and peak memory.

### Maple projection and transport

- Plain text, multiple blocks in one row, and hidden content.
- Tool request/response split exactly at a page boundary.
- Reasoning replay and usage boundaries split across pages without lost safe
  content or synthetic storage records.
- Permission request/resolution and stopped notices split across pages.
- Concatenated paged projection preserves eligible safe content and canonical
  storage order; cross-page tool and permission enrichment converges.
- Request limit, cursor, target, account, pairing incarnation, and connection
  generation are all enforced.
- One oversized record fails explicitly without blocking control/cancel traffic.
- Embedded and Iroh adapters return the same serialized page for the same host
  state.

### Frontend and mobile resume

- Four or more history pages prepend chronologically with no duplicate cards.
- A visible expanded tool card and keyboard focus survive prepend/regrouping.
- Live terminal or resolved state wins over an older paged snapshot.
- A replayed append event is applied once by event sequence.
- `HistoryReplaced` rejects a stale in-flight page and reloads only the head.
- Phone background/foreground resumes events without a history fetch while the
  event cursor is retained.
- Replay overflow reloads one bounded head, never the whole conversation.
- Exact-app macOS testing proves local Agent scrollback is not degraded; iOS
  simulator/device testing proves repeated background/resume behavior.

## Deliberately deferred

- Merging Chat and Agent presentation components.
- Complete-turn pagination.
- Client-selected byte budgets.
- A second durable copy of committed Agent history in Maple.
- Multi-controller leases or arbitrary observer fan-out beyond the explicitly
  paired v1 controller.

These are not prerequisites for native Goose-message paging and can be revisited
only if measured product behavior requires them.
