# Agent Mode flow checklist

This checklist lists the user-visible behavior of Maple Desktop Agent Mode.
Use it to confirm that the native gpui app replicates the flow end to end.

Sources (in the Maple repo):

- `docs/agent-mode-acp.md`
- `docs/tool-call-immediate-rendering.md`
- `docs/unified-chat-refactor.md`
- `frontend/src/components/AgentMode.tsx`
- `frontend/src/services/agentRuntimeService.ts`

Tags:

- `[core]` — the flow is broken without it.
- `[polish]` — nice-to-have for a first native milestone.

Items are in user-journey order.

## 1. Login

- [ ] [core] A signed-out user sees the app entry (login) page instead of Agent Mode.
- [ ] [core] A signed-in user opens Agent Mode and the view is keyed to that user; a different account gets a fresh view.
- [ ] [core] Logout or an account change stops the agent runtime and clears in-memory agent state before the next user is admitted.
- [ ] [polish] The window title reads "Maple Agent Mode" when signed in.

## 2. Runtime start

- [ ] [core] On open, the app loads runtime status, config, recent project roots, and MCP servers, and shows an initializing state until they arrive.
- [ ] [core] If the runtime is not running and a default project root exists, the app starts the runtime automatically.
- [ ] [core] While the runtime starts, the send button shows a spinner and the composer settings are locked.
- [ ] [core] A runtime start failure shows a red error banner above the timeline with the error text.
- [ ] [core] A `runtimeStatus` event updates the running flag, project root, model, mode, and active runs.
- [ ] [core] Restart of the runtime (for example, a project root change) goes through a restart command, not a second start.
- [ ] [polish] Runtime stop on app exit or update restart attempts a clean shutdown before the core runtime stops.

## 3. Project root and trust

- [ ] [core] The composer has a project folder selector with a "New project…" item and the list of recent roots by display name.
- [ ] [core] "New project…" opens a native folder picker; the chosen folder is saved as a recent root and becomes the active root.
- [ ] [core] Sending with no project root fails with "Select a project folder first" and no task is created.
- [ ] [core] The sidebar groups tasks under their project root; each root can collapse or expand.
- [ ] [core] Each project has a menu with rename, remove, "New task", and "Project Settings".
- [ ] [core] Remove project shows a confirm dialog "Remove <name>?"; a failure shows the error and offers "Retry".
- [ ] [core] When a selected project contains a real `.agents/skills/**/SKILL.md` and has no saved trust decision, a modal "Trust this project?" appears; Escape does not close it.
- [ ] [core] The trust dialog offers "Keep disabled" (cancel) and "Trust this project"; the decision persists per project and per account.
- [ ] [core] The trust check runs again at each task boundary (new or selected task), not by polling.
- [ ] [core] "Project Settings" shows a durable trust switch; a change is rejected while tasks in that project are running.
- [ ] [polish] Sidebar projects can be reordered by drag and drop, and the order persists.
- [ ] [polish] A removed project root is hidden but not deleted; its tasks stay in history.

## 4. New task

- [ ] [core] The sidebar has a "New task in <project>" button per project and a global new-task action.
- [ ] [core] A new task shows the empty state: "Work on anything..." heading, the composer, and "Encrypted and private at every step".
- [ ] [core] The composer placeholder reads "Ask Maple to work in this folder...".
- [ ] [core] The task session is created lazily on the first send, with the selected project root, model, mode, and enabled MCP servers.
- [ ] [core] The new task appears in the sidebar at once (`sessionCreated` event) and becomes the active task.
- [ ] [core] A project with no tasks shows "No tasks yet".
- [ ] [polish] The composer keeps a separate draft per task and restores it on switch.
- [ ] [polish] The "Add images" button attaches JPEG, PNG, or WebP files; a non-eligible plan opens an upgrade dialog.
- [ ] [polish] An MCP menu in the composer lists servers with enable toggles and a "Manage" action that opens the MCP servers dialog.

## 5. Send message

- [ ] [core] Enter sends; Shift+Enter inserts a newline.
- [ ] [core] The send button is disabled when the text is empty, the runtime is starting, or the composer is locked.
- [ ] [core] On send, the user message appears at once as a user turn and the composer clears.
- [ ] [core] After send, a pending indicator shows under the user turn until the first assistant item arrives.
- [ ] [core] A send failure appends an error item with status "failed" to the timeline and re-enables the composer.
- [ ] [core] The model and mode of the composer are sent with the message.
- [ ] [polish] The composer has an expand/fullscreen toggle.
- [ ] [polish] Up/Down arrow in an empty composer navigates prompt history for the task.

## 6. Streaming render

- [ ] [core] `runStarted` marks the task as running in the sidebar and in the composer (stop button visible).
- [ ] [core] Each `timelineItem` event renders at once; items with `merge: "replace"` update in place by id, `append` adds text.
- [ ] [core] Assistant text renders as Markdown and streams as deltas arrive.
- [ ] [core] Thinking items render as a collapsible thinking block; the active block shows a "thinking" state while the run is live.
- [ ] [core] Adjacent thinking items merge into one block.
- [ ] [core] A tool call renders as soon as it starts, before any assistant text, with a kind icon (shell, file read, file write, web, MCP, generic), a title, and a status label.
- [ ] [core] Tool status shows a spinner for running/in progress/pending/queued, a green check for completed, and a red X for failed or error.
- [ ] [core] A tool with input, output, or a summary is expandable; a failed tool starts expanded.
- [ ] [core] A permission item renders as a highlighted card with a shield icon, title ("Permission requested" by default), description, and input details.
- [ ] [core] A pending permission card has "Allow once", "Deny", and "Cancel" buttons; a click sends the decision for that exact run.
- [ ] [core] After a decision the card loses its highlight and shows the result: "Allowed once", "Denied", or "Cancelled".
- [ ] [core] System and error items render as a muted row; error rows use the destructive style.
- [ ] [core] `runFinished` clears the running state; a `historyReplaced` event reloads the full session timeline.
- [ ] [core] The timeline auto-scrolls when the user is within the bottom threshold and stops when the user scrolls up.
- [ ] [polish] Thinking blocks get a generated short label per phase.
- [ ] [polish] Assistant and user turns have a copy button; the copy button is hidden on the live last turn.
- [ ] [polish] Consecutive user turns stack visually.

## 7. Queue, steer, and stop

- [ ] [core] While a run is active, the composer shows a red stop button next to send.
- [ ] [core] Stop cancels the active run; the composer shows a stopping state until `runFinished` arrives.
- [ ] [core] Send while a run is active queues the message; it appears as a chip above the composer.
- [ ] [core] When the run finishes, the next queued message is promoted and sent automatically (`queuePromoted`).
- [ ] [core] Each queued chip has remove, edit, and "Send into the current turn" (steer) actions.
- [ ] [core] Cmd/Ctrl+Enter steers: the composer text is sent into the current turn instead of the queue.
- [ ] [core] Edit chip loads the chip text into the composer with placeholder "Edit the queued message, then send to keep its place..."; send updates the chip in place; "Discard" restores the draft.
- [ ] [core] Queue state per task survives task switches and comes back from `queueChanged` events.
- [ ] [polish] The edit lock on a chip is released if the task is switched or the composer is discarded.

## 8. Session switch, rename, delete

- [ ] [core] A click on a sidebar task loads its full timeline, queue, and MCP errors and makes it active.
- [ ] [core] Events for a non-active task update that task's state without changing the visible timeline.
- [ ] [core] A task that finishes while not active shows an unread indicator; the indicator clears when the task is opened.
- [ ] [core] A running task shows a running indicator in the sidebar; a collapsed project shows an aggregate indicator.
- [ ] [core] Each task row has a menu with rename and delete.
- [ ] [core] Rename opens a dialog; the new title shows in the sidebar at once (`sessionUpdated`).
- [ ] [core] Delete opens a confirm dialog; the task disappears from the sidebar and later events for that id are ignored.
- [ ] [core] If the active task is deleted, the view returns to the new-task empty state.
- [ ] [core] Task list is per account and sorted newest first.
- [ ] [polish] The sidebar collapses and expands with a transition and remembers its preference.
- [ ] [polish] Task rows support keyboard focus and open the menu with the keyboard.

## 9. Model switching

- [ ] [core] The composer has a model selector with a "Quick" default and the authenticated model catalog.
- [ ] [core] The model can be changed only before the first message; after the task has history or a run is active, the selector is disabled.
- [ ] [core] A session that loads with a persisted model shows that model.
- [ ] [core] The mode selector offers "Read only" (smart approve) and "Allow all" (auto) with descriptions; a change calls the backend for the active task.
- [ ] [core] A switch to "Read only" shows the new value only after the backend confirms it; a switch to "Allow all" shows at once.
- [ ] [polish] The model selector remembers the last chosen model as the default for new tasks.
- [ ] [polish] A model that needs a higher plan opens an upgrade dialog.
- [ ] [polish] An "advanced" toggle in the model selector reveals the full catalog.
