# Frontend correctness review

Scope: `app/src/ui/chat.rs`, `app/src/ui/login.rs`, `app/src/ui/text_input.rs`,
`app/src/ui/markdown.rs`, `app/src/main.rs`.

Reference: `docs/agent-flow-checklist.md` ([core] items), Maple
`frontend/src/components/AgentMode.tsx` (`mergeTimelineItem` at line 6097,
`handleAgentEvent` at line 3110, `loadSession` at line 2327), and the backend
item builders in `crates/maple-agent/src/agent.rs`.

Severity: BLOCKER = the flow is wrong or data is lost. SHOULD-FIX = wrong in a
reachable case, or a [core] checklist gap. NIT = cosmetic or defensive.

## BLOCKER

1. **BLOCKER** `app/src/ui/chat.rs:327` — A `replace` merge overwrites the
   whole item. Maple keeps `title`, `status`, `input`, `output`, and `text`
   from the previous item when the incoming field is `null`
   (`AgentMode.tsx:6112-6120`). The backend sends tool completions with
   `input: None` and `text: None` (`agent.rs:7488-7512`, `tool_response_item`)
   and permission decisions as a status-only row. Result: when a tool
   finishes, its arguments and chain summary vanish from the card; a
   `load_skill` response with `title: None` blanks the title.
   Fix: replace the `else` branch with a field merge:
   ```rust
   let AgentTimelineItem { title, text, status, input, output, .. } = item;
   existing.title = title.or(existing.title.take());
   existing.text = text.or(existing.text.take());
   existing.status = status.or(existing.status.take());
   existing.input = input.or(existing.input.take());
   existing.output = output.or(existing.output.take());
   existing.merge = item.merge; existing.role = item.role.or(existing.role.take());
   ```
   Keep `created_ms` and `item_type` from the incoming item (Maple spreads
   `incoming` over `previous`). The `append` branch is correct: Maple appends
   only for `message`/`thinking` with non-empty text and otherwise falls
   through to the field merge, which this code also does.

2. **BLOCKER** `app/src/ui/chat.rs:362` — `AgentServiceEvent::TimelineItem`
   drops `session_id` and applies the item to the visible timeline. The
   backend emits this event for permission-status rows with `run_id: None`
   for any session (`agent.rs:5530`, `5702`). A decision made in a background
   task inserts a foreign permission row into the active task. Maple gates on
   `activeSessionIdRef.current === sessionId` (`AgentMode.tsx:1384`).
   Fix: `if Some(session_id.as_str()) == self.selected_session.as_deref()`
   before `apply_timeline_item`; otherwise ignore (or store per session).

3. **BLOCKER** `app/src/ui/chat.rs:414-417` and `:212-224` —
   `HistoryReplaced` calls `select_session`, and `set_active_session` resets
   `active_run = None` and `pending_permission = None`. The backend publishes
   `HistoryReplaced` in the middle of a live run (`agent.rs:6404-6412`, after
   Goose compaction). After it, the Stop button disappears, `stop()` is a
   no-op, and any pending permission card is lost while the run still waits
   for an answer. `Started` does not fire again. Maple only replaces the
   timeline and keeps run state (`AgentMode.tsx:3244-3255`).
   Fix: add a `reload_timeline(session_id)` path that replaces
   `self.timeline` only, and keep `active_run`/`pending_permission`.
   `set_active_session` must be used only for a user-initiated switch.

4. **BLOCKER** `app/src/ui/chat.rs:162-181` — `select_session` has no
   ownership token. Two clicks in a row (A then B) race: if A's
   `load_session` resolves last, `set_active_session(A)` wins and the sidebar
   highlight and transcript flip back to A. Also, any `TimelineItem` that
   streams in between the backend read and the callback is overwritten by
   the stale snapshot (Maple guards with `sessionSelectionGenerationRef` and a
   per-session `timelineRevision`, `AgentMode.tsx:2327-2361`).
   Fix: add `selection_generation: u64` to `ChatScreen`; increment in
   `select_session` and capture it; in the callback, return early if it
   changed. Bump a per-session revision counter on every applied timeline
   event and drop the snapshot if the revision moved during the load.

5. **BLOCKER** `app/src/ui/login.rs:23-24`, `app/src/ui/chat.rs:37` — No
   `on_enter` is wired on any input. Enter does nothing on the login form and
   in the composer. Checklist 5 [core]: "Enter sends".
   Fix: in `LoginScreen::new`, build the inputs with
   `.on_enter(move |window, cx| ...)` that reads the parent entity via a
   `WeakEntity<LoginScreen>` captured before `cx.new`, and call a
   click-free `submit_inner(cx)`. Same for the composer with a
   `send_inner(cx)`. Do not call `self.composer.update` from inside the
   handler (see NIT 22).

6. **BLOCKER** `app/src/ui/chat.rs:254-256` — A `send_message` error is
   dropped. The composer was already cleared at line 244, so the user's text
   is gone and nothing is shown. Checklist 5 [core]: a send failure appends an
   error item with status `failed` and re-enables the composer.
   Fix: on `Err(message)`, push
   `AgentTimelineItem { id: format!("send-error-{ts}"), item_type: "error",
   title: Some("Send failed"), text: Some(message), status: Some("failed"),
   merge: "replace", .. }` and restore the draft with `composer.set_text`.

## SHOULD-FIX

7. **SHOULD-FIX** `app/src/ui/chat.rs:421-424` — `Finished` ignores
   `run_id`. A late `Finished` for an older run (a cancelled run whose task is
   still draining, `agent.rs:6420`) clears `active_run` of the newer run and
   drops its pending permission. Maple calls `clearActiveRun(sessionId,
   runId)` and only clears when the ids match.
   Fix: `if self.active_run.as_deref() == Some(run_id) { ... }`; clear
   `pending_permission` only if `permission.run_id == run_id`.

8. **SHOULD-FIX** `app/src/ui/chat.rs:349-356` — `RuntimeStatus` with
   `running: true` sets `notice = None`, which wipes a `SetupWarning`
   (`:411`), a `list_sessions` error, or any other notice. `running: false`
   overwrites the detailed start error from `start()` (`:83-86`) with the
   generic "Agent runtime stopped". The event also does not update
   `active_run` from `status.active_runs`, so a session that was running when
   the app loaded (or that ran in the background) never shows Stop.
   Fix: keep a separate `runtime_running: bool`; derive the banner from it
   plus a `runtime_error: Option<String>`; on every status set
   `self.active_run = status.active_runs.get(selected).cloned()`.

9. **SHOULD-FIX** `app/src/ui/chat.rs:383-390` — Run events for a
   non-selected session are dropped except `SessionUpdated`. `Started` and
   `Finished` are lost, so the sidebar cannot show the running or unread
   state (checklist 8 [core]) and, on switch, `set_active_session` sets
   `active_run = None` for a task that is still running (the backend does
   not re-send `Started`).
   Fix: keep `active_runs: HashMap<String, String>` keyed by session, update
   it for every `Started`/`Finished` regardless of selection, and read it in
   `set_active_session`. Also seed it from `RuntimeStatus.active_runs`.

10. **SHOULD-FIX** `app/src/ui/chat.rs:401-410` — `PermissionRequested`
    discards the `item` payload. The permission row never enters the
    timeline, so after a decision there is no "Allowed once"/"Denied" row
    (checklist 6 [core]); the later status-only `TimelineItem` for that id
    then arrives with no match and is pushed as a new bare row rendered by
    the `_` arm.
    Fix: `self.apply_timeline_item(item)` in that arm, and add a
    `"permission"` arm in `render_timeline_item` that shows the shield card
    inline with the status text. The floating card can stay for pending.

11. **SHOULD-FIX** `app/src/ui/chat.rs:277-300` — `respond_permission`
    clears `pending_permission` before the backend confirms and ignores the
    result. If the run finished first (`Finished` cleared it) or the request
    id is unknown, the error is silent and the card shows nothing. Also the
    "Cancel" decision (`AgentPermissionDecision::Cancel`) is not exposed.
    Fix: keep the permission in a `responding` state until `Ok`; on `Err`,
    show the message in `notice`. Add a third button that sends `"cancel"`.

12. **SHOULD-FIX** `app/src/ui/chat.rs:364`, `:399`, `:222` —
    `scroll_to_bottom()` runs on every timeline event. The user cannot scroll
    up while a run streams (checklist 6 [core]: auto-scroll only inside the
    bottom threshold).
    Fix: check `self.scroll.offset()` against `max_offset()` in a
    `should_autoscroll()` helper and scroll only when the user is within
    ~48 px of the bottom, or after a send.

13. **SHOULD-FIX** `app/src/ui/chat.rs:769-773` — Tool status mapping treats
    every status other than `completed`/`failed` as `running`, so `error`,
    `cancelled`, `pending`, `controlled_externally`, and `queued` render as a
    live spinner label forever. Checklist 6 [core] lists the mapping.
    Fix: `"completed" => success`, `"failed" | "error" => error`,
    `"cancelled" | "controlled_externally" => muted`, everything else running.

14. **SHOULD-FIX** `app/src/ui/markdown.rs:184-197`, `:271-281` — Nested
    lists collapse into one line. For a tight list, `Tag::Item` pushes the
    bullet into the shared `paragraph`; a nested `Tag::List`/`Tag::Item`
    pushes a second bullet into the same buffer before the outer item is
    flushed at `TagEnd::Item`. Input `- a\n  - b` renders as `• a• b`. There
    is also no indentation per depth.
    Fix: flush `paragraph` at `Tag::Item` start when it is not empty; wrap
    each item in a `div().pl(px(16. * depth))` using `list_counters.len()`.

15. **SHOULD-FIX** `app/src/ui/markdown.rs:249-267` — A fenced code block
    inside a list item (or after inline text in a loose paragraph that has
    not ended) is emitted before the text that precedes it, because the
    paragraph buffer is flushed only at `TagEnd::Item`/`TagEnd::Paragraph`.
    `- run:\n  \`\`\`sh\n  ls\n  \`\`\`` renders the code block above "• run:".
    Fix: flush the pending paragraph at `Tag::CodeBlock` start (and at
    `Event::Rule`, which has the same ordering bug at `:305`).

16. **SHOULD-FIX** `app/src/ui/markdown.rs:146`, `app/src/ui/chat.rs:662` —
    Every frame re-parses and re-shapes every assistant message in the
    timeline. Cost per frame is O(total transcript bytes) for parsing plus a
    full `StyledText` shape per paragraph; during streaming each delta
    triggers `notify`, so a 50-message transcript with a 20 KB streaming
    reply re-parses ~all 50 messages per delta. This is pathological once
    a session has a few long answers: parse + shape time grows linearly
    with history, not with the delta.
    Fix: cache the rendered element tree per item keyed by
    `(item.id, item.text.len())` (or a hash) in `ChatScreen`, invalidate
    only the item that changed, and render the transcript with `gpui::list`
    so off-screen items are not laid out.

17. **SHOULD-FIX** `app/src/ui/text_input.rs:470-483` —
    `character_index_for_point` with `mask == true` takes the display index
    (one byte per `*`) and feeds it to `offset_to_utf16` as a content byte
    offset. For a password with any multi-byte character, the IME/lookup
    position is wrong (and can land inside a char boundary, which panics in
    `text_for_range` slicing). The `assert_eq!` at `:479` also panics in
    release builds if a layout from a previous frame is stale.
    Fix: map through the same char-index table as
    `index_for_mouse_position` (`:288-296`); replace the assert with an
    early `return None` when lengths differ.

18. **SHOULD-FIX** `app/src/main.rs:104-125` — The pump is started per
    login. `take_events` hands out the receiver once; a second
    `LoginSucceeded` (after a future logout) gets `None` and the new
    `ChatScreen` receives no events, while the first pump keeps forwarding
    into the dropped entity (`update(..).ok()` swallows it). The first pump
    is alive today only because `MapleApp` never re-enters login.
    Fix: start the pump once in `main` and route through a
    `Entity<MapleApp>`-level handler that forwards to the current
    `Screen::Chat`; or keep the receiver in `MapleApp` and re-attach it.

19. **SHOULD-FIX** `app/src/main.rs:120-121` — If `chat.update` fails
    (window closed, entity dropped) the event is discarded and the loop
    continues to drain the channel forever, so the runtime keeps running
    with no consumer. Not fatal today, but it hides every event after a
    window close and burns CPU on a busy run.
    Fix: on `Err`, `break` out of the loop and let the receiver drop; the
    backend already tolerates a closed channel (`backend.rs:55-60`).

20. **SHOULD-FIX** `app/src/ui/chat.rs:226-232` — `send` while `booting`
    returns silently; the Send button looks enabled and nothing happens.
    Checklist 2/5 [core]: spinner and disabled send while the runtime
    starts. Also send with `selected_session == None` returns silently
    instead of creating the session lazily (checklist 4 [core]).
    Fix: render the button disabled (no hover, muted bg) when
    `booting || text.is_empty()`; when there is no session, call
    `create_session` and then send in the callback.

21. **SHOULD-FIX** `app/src/ui/login.rs:51` and `backend.rs:174` — The
    `JoinError` and the `set_auth` error strings are shown verbatim. The
    OpenSecret errors are already sanitized at `backend.rs:157-161`, but
    `set_auth(..).await?` can surface internal validation text (URL, user
    id) to the login form.
    Fix: map both to a fixed message and `log::warn!` the detail.

## NIT

22. **NIT** `app/src/ui/text_input.rs:123-128` — `enter_pressed` takes the
    handler out, calls it, and puts it back. If the handler replaces the
    entity's `on_enter` (through `cx`) the new handler is overwritten by the
    old one. If the handler calls `.update()` on this same `TextInput`
    entity, gpui panics on the double lease. Document the contract or pass
    the handler a `WeakEntity<TextInput>` and drop the take/re-insert.

23. **NIT** `app/src/ui/text_input.rs:702` — The Enter check ignores
    modifiers and IME state. Enter during a marked (composing) range should
    commit the composition, not submit; Cmd/Ctrl+Enter should be free for
    the steer action (checklist 7 [core]). Check
    `event.keystroke.modifiers` and `self.marked_range.is_none()`.

24. **NIT** `app/src/ui/text_input.rs:45-50` — All bindings are registered
    with `context: None`, so `ctrl-a`, `ctrl-c`, `ctrl-x` are global. They
    only reach a `TextInput` because it is the only `on_action` handler, but
    any future app-level `ctrl-a`/`ctrl-c` action will collide. Register
    with `Some("TextInput")` to match the `key_context` at `:684`.

25. **NIT** `app/src/ui/markdown.rs:156`, `:175-177` — `heading` is set but
    never read; a heading inside a list item inherits the bullet in the
    paragraph buffer and renders as `• Title` in bold. Flush the buffer at
    `Tag::Heading` start and drop the variable.

26. **NIT** `app/src/ui/markdown.rs:314-321` — `InlineHtml` is pushed as
    plain text and block `Html` outside code is dropped. Streaming replies
    often contain `<br>` or `<details>`; showing the raw tag or nothing is
    inconsistent. Pick one (Maple renders HTML as text).

27. **NIT** `app/src/ui/chat.rs:828` — The `_` arm hides unknown item types
    with empty text as a zero-height `div()`, which still adds `gap_3`
    spacing. Return `None` from the iterator (`filter_map`) instead.

28. **NIT** `app/src/ui/chat.rs:212-224` — `set_active_session` does not
    reset the composer draft or the models menu. Checklist 4 [polish] keeps a
    draft per task; at minimum close `models_menu_open`.

29. **NIT** `app/src/ui/login.rs:34-48` — `busy` blocks a double submit, but
    the inputs stay editable while busy; a changed email during the request
    is ignored without feedback. Disable the inputs or show them muted.

## Counts

BLOCKER: 6. SHOULD-FIX: 15. NIT: 8.
