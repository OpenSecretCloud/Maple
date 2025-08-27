## Update Plan: Migrate Frontend to Responses API

### Goals
- Step 1: Send new chats using the Responses API (not Chat Completions)
- Step 2: Stream/subscribe to the correct response via SSE and render deltas
- Step 3: Add new chats to the sidebar using the response ID and server‑generated title
- Future Work: Loader/migration for old chats into Responses
- Future Work: Unify chat UX (start on home, push `chatId` into URL, no full page swap)

### References
- Backend implementation notes: `opensecret/docs/responses-implementation.md`
- OpenAPI surface: `opensecret/docs/openapi.yml`
- OpenAI migration guide: `opensecret/docs/openai-migrate-doc.md`

### Decisions (from you)
- System prompts: keep as‑is. We will map the existing `systemPrompt` to the Responses request without changing the UI surface (use `instructions` when calling Responses, logic stays the same).
- Titles: rely on the Responses list endpoint to return server‑generated titles; fallback to "New Chat" if undefined.
- Vision/images: defer for first pass (text‑only).
- Base URL/auth: same base and middleware; continue using `aiCustomFetch`.
- Idempotency: defer; add a TODO for sending `Idempotency-Key` in a later pass.

---

## High‑Level Approach
1. Leverage the OpenAI client (already configured with `aiCustomFetch`) and the OpenSecret SDK:
   - Use `openai.responses.create({ model, input, instructions?, stream: true })` to start a new chat
   - Consume the returned async iterator of events (SDK emits OpenAI‑compatible events)
2. Update chat creation flow to detect “new chat” and route through Responses API:
   - Build request payload from current UI state (model, input, optional system prompt, metadata)
   - For brand‑new threads, omit `previous_response_id`
   - Receive the response ID from `response.created` and treat it as the `chatId`
3. UI streaming:
   - Replace `openai.beta.chat.completions.stream` with `openai.responses.create(..., stream:true)`
   - Accumulate deltas into `currentStreamingMessage`
   - Handle abort, transient errors, and server error events
4. Sidebar & titles:
   - On `response.completed`, invalidate the history query and fetch via the SDK `fetchResponsesList` which includes titles
   - Use server‑generated titles; remove client title generation for Responses‑backed chats
5. Backward compatibility:
   - Maintain legacy path for old chats (flag per chat: `storageMode: 'responses' | 'legacy'`)
   - Keep existing persistence for legacy chats until migration

---

## Detailed Plan by Step

### Step 1 — Send new chats to Responses API
- Detection: A “new chat” means no prior messages stored for the current `chatId`.
- Build request (aligned to Responses spec in backend doc):
  - `model`: selected model
  - `input`: user text (text‑only for first pass)
  - `instructions`: pass current `systemPrompt` when set (keeps system prompts as‑is from UI)
  - `stream`: true
  - `store`: true (server persists)
  - `previous_response_id`: null (new thread)
  - `metadata`: optional; omit for now
- Call via SDK: `const stream = await openai.responses.create({...})`
- Begin consuming events immediately (see Step 2).

Code changes (initial):
- `frontend/src/hooks/useChatSession.ts`
  - Branch in `appendUserMessage` for new chat: call `openai.responses.create`
  - Remove local title generation for Responses mode
  - Use `AbortController` to cancel the stream

### Step 2 — View the incoming SSE stream
- Parse and handle events based on backend spec (see `responses-implementation.md`):
  - `response.created`: capture `response.id` and set it as current `chatId` if needed (navigate early if appropriate)
  - `response.in_progress`: optional UI signal
  - `response.output_item.added` / `response.content_part.added`: initialize structures for deltas
  - `response.output_text.delta`: append to `currentStreamingMessage`
  - `response.output_text.done` / `response.output_item.done`: finalize message block
  - `response.completed`: close stream, trigger invalidations (history, specific chat)
- Error frames: surface to UI and end stream properly; keep DB write robustness server‑side.
- Abort: if user navigates away or starts another send, abort controller cancels the stream.

Code changes (initial):
- `frontend/src/hooks/useChatSession.ts`
  - Replace the streaming loop to drive off events from `openai.responses.create` iterator
- Minor adjustments in `frontend/src/routes/_auth.chat.$chatId.tsx` to treat `currentStreamingMessage` as the single assistant delta buffer

### Step 3 — Add new chats to the sidebar with correct ID and server title
- Use the response UUID as the `chatId` (per backend: first message UUID = thread UUID for new threads)
- Immediately navigate using that `chatId` once `response.created` arrives (optional; or navigate after completion)
- On `response.completed`, invalidate:
  - `['chatHistory']` so the sidebar refreshes and shows the decrypted, server‑generated title
  - `['chat', chatId]` if we add a chat details query endpoint
- UI should not generate titles client‑side for Responses chats; show a placeholder (e.g., “Generating title…”) until the list refresh returns the title

Code changes (initial):
- `frontend/src/routes/_auth.chat.$chatId.tsx`
  - When starting a new chat, do not call local `addChat(title)`; rely on server persistence
  - Ensure navigation uses the server `chatId` (from `response.created`)
- `frontend/src/components/Sidebar` (if needed)
  - Ensure `chatHistory` loader uses SDK `fetchResponsesList` for Responses mode

---

## Data & State Model
- New per‑chat flag: `storageMode`
  - `legacy`: existing KV‑style
  - `responses`: server‑managed threads/messages
- Local optimistic state still used to render the user message immediately and the streaming assistant message.
- Persistence for Responses chats is entirely server‑side; frontend only invalidates queries after completion.

---

## Networking & Auth
- Use existing authenticated fetch/session mechanism (same as other backend requests)
- SSE with `fetch` + `ReadableStream` (EventSource cannot POST)
- Timeouts/heartbeats: backend emits `: heartbeat` frames; keep connection alive

---

## Error Handling
- Map SSE errors to user‑visible banner (reuse existing `streamingError` pattern)
- On abort, suppress error banners
- Keep UI resilient if storage fails on server (server continues streaming per docs)

---

## Testing Plan
- Unit test SSE parser on recorded streams
- Manual test flows:
  - New chat happy path (delta stream → completed → title shows in sidebar)
  - Abort mid‑stream
  - Network hiccup during stream
  - Auth error
- Regression: legacy chats still work unchanged

---

## Rollout Plan
- Feature‑flag Responses path for new chats
- Dogfood with a subset of users
- Flip default to Responses for all new chats

---

## Future Work

### 1. Load and migrate legacy chats to Responses
- Provide a migration UI: select legacy thread → create new Responses thread → seed context
- Mark legacy items as migrated; keep references for back‑links

### 2. Improve Chat Handoff (Medium Refactor)
**Problem:** Currently when starting a chat from home route, we create a local chat ID, store prompt/images in context, navigate to chat route, then chat route reads the handoff data and starts streaming. This is clunky and causes unnecessary route transitions.

**Solution:** Make the home route (`/`) handle both empty and active chat states:
- When user sends first message from home:
  - Start streaming immediately in place (no navigation)
  - Get response ID from server via `response.created` event
  - Use `window.history.replaceState` to update URL to `/chat/[id]` without page reload
  - Continue streaming in the same component instance
- Benefits:
  - Delete handoff logic (userPrompt, systemPrompt, userImages context state)
  - Delete duplicate chat rendering logic between home and chat routes
  - Smoother UX with no visible navigation flash

### 3. Unified Chat Component (Larger Refactor)  
**Problem:** Duplicate logic between home and chat routes for rendering and managing chats.

**Solution:** Create a single `<ChatInterface>` component that works for both new and existing chats:
- Home route renders: `<ChatInterface chatId={null} />`
- Chat route renders: `<ChatInterface chatId={params.chatId} />`
- Component handles:
  - If `chatId` is null and message sent → create response, update URL via `replaceState`
  - If `chatId` exists → load from Responses API or legacy storage
  - All streaming, rendering, and state management in one place
- Benefits:
  - Single source of truth for chat UI logic
  - Delete all handoff/navigation code
  - Cleaner separation of concerns
  - With Responses API, we ALWAYS have server state - no need for complex local persistence

**Note:** Once fully on Responses API, we can delete the completions API code entirely. Every chat exists on the server, we only need to support loading (not appending to) legacy chats from storage.

---

## TODOs (non‑blocking)
- Add `Idempotency-Key` header generation for new chats (configurable expiry)
- Extend to vision/images when ready (map attachments to supported inputs)
- Reconcile any minor type differences for `aiCustomFetch` vs OpenAI client `fetch` signature if needed (adapter wrapper)


