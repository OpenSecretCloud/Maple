# Maple: Migration Plan to OpenAI Conversations + Responses

This plan completes Maple’s end-to-end migration from the legacy “threads” model to OpenAI-compatible Conversations + Responses APIs. It aligns Maple with the implementation in `../opensecret/src/web/conversations.rs` and patterns from `../openai-responses-poc`.

Outcome goals:
- Legacy chats still load (KV storage), but are read-only (no new messages).
- All new chats are first-class Conversations; message sends use Responses API scoped to a Conversation.
- Frontend no longer owns chat history persistence; state is server-managed and retrieved via Conversations endpoints.
- Robust streaming with correct event handling; unknown/unsupported events are logged for diagnosis.

Decisions confirmed:
- URL redirects: Provide a minimal, centralized redirect for old URLs. If we cannot map a historical Response ID to a Conversation ID via API, redirect to home with a notice. Keep logic in one place with a TODO to enhance backend (`GET /responses/{id}` include conversation_id) for perfect mapping.
- Titles: Backend populates metadata; show placeholder “New Conversation” while awaiting server title.
- Rename/Delete: Implement fully for Conversation-backed chats now (`conversations.update/delete`).
- Legacy migration: Show a simple read-only banner; migration UX is future work.
- History pagination: Future enhancement; for now, load all items.
- Web search/tools: Add TODO + console logging hooks referencing POC locations for follow-up.
- Images: Future enhancement; first pass is text-only.

References:
- Backend Conversations: `../opensecret/src/web/conversations.rs`
- Backend Responses: `../opensecret/src/web/responses.rs` (expects `conversation`, deprecates `previous_response_id`)
- POC patterns: `../openai-responses-poc/frontend/src/contexts/ConversationContext.tsx`, `../openai-responses-poc/frontend/src/lib/openai-client.ts`, `../openai-responses-poc/frontend/src/lib/streaming.ts`
- OpenSecret SDK: `../OpenSecret-SDK/src/lib/api.ts` (`listConversations`, conversations items helpers)

—

## Architecture Changes

- Chat identity: `chatId` becomes the Conversation ID (UUID). We do NOT use Response IDs for routing.
- Continuation: pass `conversation: <conversationId>` to `openai.responses.create(...)` rather than `previous_response_id`.
- History retrieval: load messages via `GET /v1/conversations/:id/items` (OpenAI-compatible) instead of storing client-side.
- Conversation list: sidebar populated from the custom `GET /v1/conversations` endpoint exposed via the OpenSecret SDK `listConversations()`.
- Legacy chats: detected from KV; render-only. UI prevents sending and shows a read-only banner.

—

## Step-by-Step Plan

1) Wire Conversation List (Sidebar)
- Replace usage of `fetchResponsesList` with `listConversations({ limit, after, before })` in `frontend/src/state/LocalStateContext.tsx`.
- Map item title from `conversation.metadata?.title ?? "New Conversation"` (backend stores title in metadata).
- Merge legacy history (KV) with Conversations list for a unified sidebar view, de-duped by `id`.
- Keep sort by `updated_at`/`created_at` as today; Conversations provide `created_at`. If `updated_at` is needed, approximate with last item timestamp (follow-up).

2) Load Conversation Items for Chat View
- Update `getChatById(id)` in `LocalStateContext.tsx`:
  - If KV entry exists → legacy chat (read-only), return as-is.
  - Else, treat `id` as Conversation ID: fetch items via `openai.conversations.items.list(id, { order: 'asc' })`.
  - Map items to Maple `ChatMessage[]`:
    - `type === 'message'` with `role in {user,assistant}` → extract text from content array.
    - Ignore or log other item types for now (e.g., tool calls); preserve for future UI.
  - Return `{ id, title, model?, messages }` without persisting to KV.
- Remove localStorage fallback `responses_chat_*` writes and reads.

3) Create + Continue Chats Using Conversations
- Update `useChatSession` to adopt Conversation-first flow:
  - NEW chat:
    - Call `openai.conversations.create({})` immediately.
    - Navigate/replace URL to `/chat/<conversationId>` when created.
    - Then call `openai.responses.create({ model, conversation: <conversationId>, input: <string>, stream: true, store: true })`.
  - EXISTING chat:
    - Use `conversation: chatId` on all subsequent `responses.create` calls.
  - Remove `previous_response_id` entirely (backend ignores it and prefers `conversation`).
- Input shape: keep simple `input: string` for now (backend supports; images later).

4) Streaming Event Handling (Responses API)
- Consume SSE frames compatible with backend events (see `responses.rs`):
  - `response.created` → no-op except ensure local state shows “streaming”.
  - `response.in_progress` → optional UI signal.
  - `response.output_item.added` → begin assistant message.
  - `response.content_part.added` → can be ignored for text-only, or prepare part indices.
  - `response.output_text.delta` → append to streaming buffer.
  - `response.output_text.done` → optional; finalization occurs on completed.
  - `response.completed` → finalize assistant message, invalidate conversation list.
  - `response.error` → present banner and end stream.
- For any unknown `event.type`, log to console with the raw frame. This unblocks future tool/web-search types.
- Abort handling: keep `AbortController` semantics; don’t surface abort as an error.

5) Make Legacy Chats Read-Only
- Detect legacy via `getChatById` (KV hit) and render a banner: “This is a legacy chat. You can read it but not reply.”
- Disable input submit for legacy chats (prop from route → `ChatBox`).
- Prevent `appendUserMessage` from sending on legacy chats (guard in `useChatSession`).

6) Titles & Rename/Delete Behavior
- Titles: default to metadata.title from `listConversations()`. If absent, show “New Conversation”.
- Option A (minimal): hide “Rename” for Conversation-backed chats until we wire `openai.conversations.update(id, { metadata: { title }})`.
- Option B (preferred): wire rename for Conversations now; keep KV rename for legacy.
- Delete behavior:
  - Conversations: call `openai.conversations.delete(id)` then invalidate list.
  - Legacy: current KV delete path.

7) Cleanup Frontend State
- Remove Responses-list history fetches and any writes to `localStorage` for Responses.
- Delete the “handoff” context state where possible (userPrompt/systemPrompt/image handoff), favoring in-place start on home and URL `replaceState` (future refactor matches POC).
- Keep `draftMessages` as-is if still used purely for input UX.

8) Error Handling & Observability
- Replace generic “Failed to create response stream” with actionable messages:
  - Auth missing (OpenSecret context unavailable)
  - Network/HTTP error with status + server error message
  - Unsupported event types (logged)
- Add `console.debug` around request payloads (without user content) to verify `conversation` is always sent.

9) Testing & Validation
- Manual scenarios:
  - New chat: create conversation → stream model deltas → title appears in sidebar.
  - Continue existing conversation (refresh and send again): no prior messages passed in request; conversation context is honored.
  - Legacy chat opens; input disabled; cannot send.
  - Abort mid-stream; no banner; can send again.
  - Error case: surface `response.error` frame.
- Optional: record one SSE stream and verify parser accumulates text correctly.

—

## File-Level Changes (Minimal, Surgical)

- `frontend/src/state/LocalStateContext.tsx`
  - Swap `fetchResponsesList` → `listConversations` for history.
  - In `getChatById`, add Conversations branch: fetch items via `openai.conversations.items.list` and map to `ChatMessage[]`.
  - Remove localStorage `responses_chat_*` reads/writes.
  - Gate `renameChat`/`deleteChat` by storage mode (KV vs Conversations). Optionally add new helpers using OpenAI client for Conversations.

- `frontend/src/hooks/useChatSession.ts`
  - Introduce Conversation lifecycle:
    - On new chat: create conversation, set URL, then send response with `conversation`.
    - On existing: always send with `conversation: chatId`.
  - Remove `previous_response_id` usage.
  - Expand event handling; log unknown events.
  - Guard append on legacy (read-only) chats.
  - Centralize redirect handling for old URLs: detect non-conversation UUIDs; if not a KV chat and not a valid conversation, attempt `fetchResponse(id)`. If mapping to conversation is not possible (current backend), redirect to home and show a “Chat moved” notice. Add TODO to switch to perfect redirect when backend returns `conversation_id` on response retrieval.

- `frontend/src/routes/_auth.chat.$chatId.tsx`
  - Use new `useChatSession` semantics; disable input when chat is legacy.
  - Remove reliance on response IDs to mutate URL; now URL is set when the conversation is created.
  - Add minimal redirect surface (single place) that handles the old URL flow described above.

- `frontend/src/components/ChatHistoryList.tsx`
  - Optionally hide Rename for Conversations until wired; otherwise call Conversations update.

- Delete/clean any code that references “Responses list” as a source of truth for chat history.

—

## Implementation Checklist

API-correctness first
- [x] Replace `previous_response_id` with `conversation` param in all sends
- [x] Create conversation on first send; update URL via `history.replaceState` (no navigation)
- [ ] Ensure Responses stream consumption handles event types per backend

Conversation list and loading
- [ ] Sidebar uses `listConversations()` from SDK (no responses list)
- [ ] Titles show placeholder “New Conversation” until backend metadata present
- [ ] `getChatById` loads items via `conversations.items.list(order: 'asc')`
- [ ] Map items to messages; log unsupported types (web search/tool) with TODO + POC references

Legacy handling
- [ ] Legacy KV chats open; input disabled; banner shown
- [ ] All new chats are Conversations (no KV writes for them)

Rename/Delete
- [ ] Implement `conversations.update(id, { metadata: { title } })`
- [ ] Implement `conversations.delete(id)` and invalidate list
- [ ] Keep KV rename/delete for legacy

Redirects
- [ ] Centralized redirect: detect old URLs; try conversation retrieve; fallback to response retrieve
- [ ] If cannot map Response → Conversation, redirect to home with notice
- [ ] TODO: remove workaround after backend adds `conversation_id` to `/responses/{id}`

Cleanup
- [ ] Remove localStorage `responses_chat_*` code paths
- [ ] Remove responses-list-based history paths
- [ ] Delete obsolete thread/previous_response_id logic

Testing
- [ ] New chat happy path (stream → complete → title appears)
- [ ] Continue existing conversation (no message history in request)
- [ ] Legacy chat read-only UX
- [ ] Abort mid-stream, no error
- [ ] Error frame surfaces to UI

—

—

## Acceptance Criteria

- New chats always create a Conversation first; route updates to `/chat/<conversationId>` before streaming.
- Sends use `openai.responses.create({ conversation: <id>, input: <string>, stream: true, store: true })`.
- Existing conversation sends do NOT include the full message history nor `previous_response_id`.
- Sidebar lists Conversations via `listConversations()` and legacy KV chats; titles come from conversation metadata or fallback.
- Legacy chats open but are read-only.
- Streaming works without “Failed to create response stream”; unknown event types are logged.

Note: We prioritize making the API usage correct first (conversation-scoped sends and creation order) to eliminate the current stream error, then complete sidebar/history and cleanup.

—

## Open Questions / Decisions Needed

1) URL semantics: Can we definitively change `chatId` to mean Conversation ID only? Should we add a compatibility redirect if a user opens a historical URL that contains a Response ID?
2) Titles: Do we want client-side title generation (as in POC) for brand-new conversations, or will the backend populate metadata.title (synchronously or via background job)?
3) Rename/Delete: Should we wire Conversations `update/delete` now, or temporarily hide rename/delete for Conversation-backed chats?
4) Legacy banner copy: Ok to show a non-intrusive banner (“Legacy chat. Read-only.”) and disable input? Any desire to expose a “Migrate this chat” action?
5) Pagination for conversation items: Is loading full history acceptable for now, or do we need paging/infinite scroll in the first pass?
6) Tool/web-search support: Should we log-only unknown items, or also render lightweight stubs (e.g., a small badge row) similar to the POC’s web search handling?
7) Vision/images: Confirm deferring for now. When we add it, we’ll switch `input` to content parts and support image inputs + conversation items mapping.

—

## Rollout

- Feature-flag: gate Conversations path for a small cohort; default on after sanity checks.
- Dogfood in development/staging; verify cross-device behavior (Conversations persist server-side).
- Remove legacy Responses-list dependencies after stable.

—

## Notes on Current Failure

- The “Failed to create response stream” likely comes from sending `previous_response_id` without a `conversation`. Backend `responses.rs` treats `previous_response_id` as deprecated and does not use it; without `conversation`, a new conversation is created for each send, and client treats Response IDs as thread identifiers. Switching to explicit `conversation` parameter aligns with backend and fixes continuity.
