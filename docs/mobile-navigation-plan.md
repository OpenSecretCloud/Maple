# Mobile Navigation Plan

## Status

Core implementation is complete on the `mobile-navigation` branch. The navigation/history and
stream-disconnect behavior have focused automated coverage, and the repository's format, lint,
typecheck, test, and production-build checks pass. The unchecked acceptance items below require
interactive browser or physical iOS/Android validation and remain the final release-validation
pass.

## Objective

Give compact/mobile layouts a traditional page hierarchy while leaving the existing desktop-width experience unchanged:

- The existing menu becomes a full-screen mobile main menu.
- Opening a chat or project pushes a detail page over its parent page.
- Detail pages have a top-left back button.
- A chat is unmounted after it leaves the screen.
- Desktop app windows and desktop-width web browsers retain the existing sidebar-and-content layout.

The implementation should be as small as practical, avoid duplicate menu implementations, and preserve the existing URL scheme.

## Definitions

This plan uses the existing responsive layout rules:

- **Compact/mobile layout:** the current viewport-width and short-landscape checks used by `useIsMobile()` and `useIsLandscapeMobile()`.
- **Desktop layout:** the Tauri desktop app and web browsers at desktop width.

The breakpoints and compact-layout detection logic are not changing. Larger tablets that currently receive the desktop layout will continue to receive it.

## Agreed Product Behavior

### Desktop

- Preserve the current sidebar-and-chat layout.
- Preserve the current sidebar open/close behavior.
- Preserve the current chat and project headers.
- Treat a desktop-width web browser the same as the desktop app.

### Mobile main menu

- The menu is a full-screen root page, not a partial-width drawer.
- It uses the same menu content and behavior as the desktop sidebar.
- Menu items do not receive mobile-specific behavior changes.
- Projects continue to expand and collapse inline.
- The existing **View Project** action continues to open project detail.
- New Chat, Search, projects, pinned chats, recents, selection actions, pull-to-refresh, and account controls retain their current behavior.
- Existing platform and feature-flag visibility rules, including the desktop-only Agent Mode entry, remain unchanged.
- The Maple wordmark remains in the header.
- The desktop sidebar collapse control is not shown because the mobile main menu is the root page.

### Mobile chat and new-chat pages

- Opening a chat pushes a full-screen chat detail page.
- Opening New Chat pushes a transient new-chat page.
- The mobile menu/hamburger control becomes a top-left back arrow.
- Existing conversation headers retain the wordmark, conversation title, and New Chat action.
- The empty new-chat page has a back arrow but does not show a redundant New Chat action.
- Portrait and short-landscape mobile headers follow the same navigation rules.

### Mobile project detail

- Project detail is a pushed page.
- Its mobile menu/hamburger control becomes a back arrow.
- Opening a chat from project detail pushes the chat over project detail.
- Back from that chat returns to project detail; back from project detail returns to the main menu.

## Shared Menu Architecture

There must be one menu implementation.

The current `Sidebar` combines two responsibilities:

1. Sidebar-specific layout, sizing, visibility, outside-click, and collapse behavior.
2. The actual menu UI and behavior.

Refactor this boundary without redesigning the menu:

- Extract the inner menu into a shared `MainMenu` component.
- Keep `Sidebar` as a thin desktop wrapper around `MainMenu`.
- Render the same `MainMenu` as the full-screen mobile root page.
- Continue using the existing `ChatHistoryList` for projects, pinned chats, recents, selection, and list actions.

Changes made later to shared menu content must appear in both the desktop sidebar and the mobile main menu automatically.

## Mobile Navigation Stack

Use a small mobile navigation stack rather than introducing a second route hierarchy.

- Keep parent pages mounted while a child page is visible.
- Hide and make covered parent pages non-interactive and inaccessible to assistive technology.
- Preserve main-menu scroll position, expanded projects, search state, and selection state while a child page is open.
- Preserve project-detail state while a chat opened from that project is visible.
- Unmount a chat after its back transition completes.
- If one chat is replaced by New Chat or another chat, unmount the chat that left the screen.

Keeping the parent page mounted is intentional and matches a native navigation stack. It does not keep a popped chat loaded.

The current root shell already keeps `AuthenticatedHomeContent` mounted and inert behind dedicated settings routes. Reuse that established mounted-surface pattern where practical rather than creating a competing persistence mechanism. Mobile navigation changes must not break the existing return-from-settings behavior managed by `PersistentHomeNavigationProvider`.

## URL and History Rules

Do not add a new route or query parameter for this feature.

Continue using the current URLs:

- `/` remains the root URL.
- `/?conversation_id=<id>` remains a chat detail URL.
- `/?project_id=<id>` remains a project detail URL.

### Root URL

- On compact/mobile layouts, a fresh load of `/` shows the mobile main menu.
- On desktop layouts, `/` keeps its existing new-chat behavior with the sidebar visible.

### Existing chats and projects

- Selecting an existing chat continues to push its `conversation_id` into browser history.
- Opening project detail continues to use `project_id`.
- Web reloads always load the current URL, regardless of viewport width.
- A web reload on a chat URL reloads that chat.
- A web reload on a project URL reloads that project.

### New Chat

New Chat has no durable URL until a conversation exists:

- Push transient in-memory browser history state without changing `/`.
- Do not reconstruct the transient new-chat screen from that history state during a full document reload.
- On the first successful send, continue replacing the current URL with the newly created `conversation_id`.
- Back before the first send returns to the prior in-app page.
- Reloading `/` on mobile returns to the main menu.

The exact internal `history.state` shape is an implementation detail and should be centralized rather than spread across components.

## Back Navigation

All mobile surfaces use one shared back-navigation flow. Do not design separate flows for mobile web, iOS, or Android.

- The top-left back arrow returns to the previous in-app page.
- A chat opened from the main menu returns to the main menu.
- A chat opened from project detail returns to project detail.
- A new chat opened from an existing chat may return to that previous chat; the previous chat is reloaded when remounted.
- A chat loaded directly from a URL with no in-app parent returns to the mobile main menu instead of sending the user out of Maple.
- Browser back/forward navigation and the in-app back button must resolve through the same centralized navigation state.
- Do not plan Android-specific native navigation handling. Verify the shared browser-history behavior on Android and address only demonstrated platform bugs.

## App Lifecycle

### Web

- The URL is authoritative at every viewport width.
- Refreshing or reopening the current web URL loads the page represented by that URL.

### iOS and Android apps

- If the app process remains in memory, preserve the current page through backgrounding and foregrounding.
- If the app process launches fresh, start on the mobile main menu.
- Do not persist the active navigation page across native process restarts.
- Existing non-navigation deep-link handling remains outside this feature's scope.

## Chat Unmounting and Catch-Up

Maple/OpenSecret already continues processing a submitted chat after the client disconnects or the app exits. This is existing system behavior, not new backend work.

When a chat leaves the mobile navigation stack:

- Complete its exit transition.
- Disconnect its local streaming reader without invoking the user-facing cancel-response operation.
- Clear its component-local UI state by unmounting it.

When that chat is opened again:

- Mount a fresh chat component.
- Load the stored conversation and items using the existing conversation-loading flow.
- Use the existing polling/catch-up behavior to reach the current processing state or completed result.
- Do not resubmit the user's prompt.

This behavior must be covered by regression testing, including leaving during an active response and reopening before and after completion.

## Transitions

The core implementation includes a simple page transition:

- Forward navigation slides the child page in from the right.
- Back navigation slides the child page out to the right and reveals its parent.
- The popped page unmounts after its exit transition completes.
- Respect `prefers-reduced-motion` by removing or minimizing nonessential animation.
- Keep transition state centralized in the mobile navigation shell.

## Implementation Sequence

### Phase 1: Shared menu boundary

1. Extract the existing inner menu UI into `MainMenu`.
2. Keep menu data, actions, dialogs, search, selection, and list behavior unchanged.
3. Convert `Sidebar` into a thin desktop layout/collapse wrapper.
4. Confirm the desktop sidebar is visually and behaviorally unchanged before adding mobile navigation.

### Phase 2: Mobile navigation state

1. Extend the current authenticated-home shell with a compact-layout navigation stack that always provides the mobile main-menu parent page.
2. Centralize interpretation of the existing URL plus transient in-memory history state.
3. Represent main menu, new chat, existing chat, and project detail without adding routes or query parameters.
4. Keep parent pages mounted and mark covered pages inert/hidden appropriately.
5. Preserve the existing persistent-home URL capture and return flow used by settings routes.

### Phase 3: Page headers and back flow

1. Replace the mobile chat hamburger with the shared back button.
2. Add the back button to the empty new-chat header.
3. Replace the mobile project-detail hamburger with the same back button.
4. Implement the direct-URL fallback to the main menu.
5. Synchronize header back, browser history back/forward, and transient new-chat history.

### Phase 4: Unmount lifecycle

1. Unmount popped chat pages after their exit transition.
2. Disconnect local streaming work without canceling server-side processing.
3. Confirm reopening uses the existing load-and-poll flow.
4. Prevent duplicate prompt submission or duplicate conversation creation during back/forward transitions.

### Phase 5: Motion and accessibility

1. Add the forward and backward slide transitions.
2. Add reduced-motion behavior.
3. Move focus to the pushed page when navigation completes.
4. Restore sensible focus when returning to the parent.
5. Ensure covered pages cannot receive pointer, keyboard, or assistive-technology interaction.

### Phase 6: Platform lifecycle and regression validation

1. Ensure web reloads honor the current URL.
2. Ensure native resume preserves the in-memory page.
3. Ensure a fresh native launch starts at the main menu.
4. Validate the existing compact breakpoint and short-landscape behavior.
5. Complete desktop and menu-behavior regression testing.

## Likely Files Involved

This is a planning estimate, not a requirement to modify every file listed.

- `frontend/src/components/Sidebar.tsx`
- `frontend/src/components/ChatHistoryList.tsx`
- `frontend/src/components/UnifiedChat.tsx`
- `frontend/src/components/ProjectDetailView.tsx`
- `frontend/src/components/AuthenticatedHomeContent.tsx`
- `frontend/src/contexts/PersistentHomeNavigationContext.ts`
- `frontend/src/routes/__root.tsx` only if the existing mounted-home wrapper needs a small integration change
- `frontend/src/routes/index.tsx` only if authenticated-root coordination requires it
- `frontend/src/utils/utils.ts` only if a shared navigation helper belongs there; breakpoint behavior must not change
- A small new shared menu and/or mobile navigation-shell component
- Focused tests for navigation-state resolution and history behavior

No backend, database, or OpenSecret API change is expected.

## Gap Analysis

### Unsent composer state

Behavior is intentionally undecided when a chat or transient new-chat page is unmounted with unsent content.

Relevant transient state includes:

- Typed but unsent text
- Selected images
- Selected documents
- Composer-specific UI state

An in-memory `draftMessages` mechanism exists in local state, but `UnifiedChat` does not currently use it. Do not silently connect, remove, or redesign that mechanism as part of this navigation work.

Before implementation is finalized, record the actual resulting behavior and decide whether preserving unsent text should become a separate follow-up. Draft persistence is not currently part of the feature's definition of done.

The implemented core behavior is that unsent composer text and attachments are discarded when a
chat or transient New Chat page is popped and unmounted. Preserving them should be considered as a
separate follow-up; this implementation does not connect or change the existing unused
`draftMessages` state.

## Stretch Goal: iOS Edge-Swipe Back

After the core feature is complete, add an interactive left-edge swipe-back gesture for the iOS Tauri app.

Wry 0.55.1 does not expose built-in back/forward navigation gestures on iOS, so this requires an app-level gesture rather than a configuration switch.

Expected behavior:

- Begin only from the left screen edge.
- Track horizontal finger movement while rejecting primarily vertical gestures.
- Move the current page with the finger and reveal the previous mounted page underneath.
- Complete based on distance and/or velocity.
- Snap back cleanly when canceled.
- Use the same centralized back action as the header button.
- Unmount the popped chat after the completed gesture.
- Avoid intercepting controls or horizontally scrollable content away from the left-edge activation area.

Do not install this custom gesture in mobile Safari; Safari owns its browser navigation gesture. No Android-specific equivalent is planned.

This stretch goal does not block completion of the core feature.

## Non-Goals

- Redesigning or duplicating the menu
- Changing how menu items behave
- Changing project expand/collapse behavior
- Adding a new chat or project URL scheme
- Adding `new_chat=true` or a similar query parameter
- Replacing the current conversation/project query parameters with path routes
- Changing desktop app or desktop-width web navigation
- Changing the existing responsive breakpoint logic
- Adding Android-specific navigation behavior without a demonstrated platform bug
- Changing Maple/OpenSecret background-processing behavior
- Changing settings navigation or persistent return-to-home behavior
- Changing Agent Mode availability or navigation
- Adding draft persistence
- Adding the iOS edge-swipe gesture before the core feature is complete

## Definition of Done

The core feature is complete when every agreed non-stretch behavior is implemented and verified. No additional product scope is implied by this checklist.

### Shared menu and desktop preservation

- [ ] Desktop app navigation is visually and behaviorally unchanged.
- [ ] Desktop-width web navigation is visually and behaviorally unchanged.
- [x] Desktop and mobile render the same shared menu implementation.
- [x] A shared menu change appears on both desktop and mobile.
- [ ] Existing menu item behavior remains unchanged.
- [ ] Existing platform and feature-flag visibility rules remain unchanged.
- [ ] Projects, pinned chats, recents, search, selection, pull-to-refresh, and account controls still work.
- [ ] Leaving for settings and returning home still restores the correct home URL and surface.

### Mobile hierarchy

- [x] A fresh mobile root load shows the full-screen main menu.
- [x] The mobile main menu has no sidebar collapse control.
- [x] New Chat opens a transient full-screen new-chat page without changing the URL.
- [x] Existing chats open as full-screen detail pages using the existing `conversation_id` URL.
- [x] Project detail uses the existing `project_id` URL.
- [ ] Project rows still expand and collapse inline in the menu.
- [x] Parent-page menu and project state are preserved while a child page is visible.

### Back behavior

- [x] Mobile chat, new-chat, and project-detail pages show the agreed back button.
- [x] Back returns to the correct previous in-app page.
- [x] Directly loaded chat URLs fall back to the mobile main menu from the in-app back button.
- [x] Browser back and forward remain synchronized with the visible mobile page.
- [x] Mobile web, iOS, and Android use the same navigation logic.

### URL and lifecycle behavior

- [x] No new route or query parameter is introduced.
- [x] Web refresh reloads the current chat or project URL at every viewport width.
- [x] Refreshing mobile `/` shows the main menu rather than reconstructing transient New Chat.
- [x] Backgrounding and foregrounding an in-memory native app preserves its current page.
- [x] A fresh native app process starts at the main menu.

### Chat lifecycle

- [x] A chat unmounts after it leaves the screen.
- [x] Leaving a generating chat does not call the cancel-response operation.
- [x] Reopening a generating chat catches up without resubmitting the prompt.
- [x] Reopening after generation completes shows the completed result.
- [x] Switching between chat, New Chat, project detail, and the menu does not create duplicate conversations or messages.

### Motion and accessibility

- [ ] Forward and back slide transitions work in portrait and short landscape.
- [x] Reduced-motion users receive a suitable non-animated or minimized transition.
- [x] Covered parent pages are inert and hidden from assistive technology.
- [x] Focus moves predictably on push and pop.
- [ ] Safe areas and the mobile keyboard do not obscure navigation controls.

### Validation

- [ ] Validate just below and at the existing desktop breakpoint.
- [ ] Validate representative portrait and short-landscape phone viewports.
- [ ] Validate mobile web refresh and browser back/forward.
- [ ] Validate iOS suspend/resume and fresh launch.
- [ ] Validate Android suspend/resume and fresh launch.
- [ ] Validate navigation away from and back to an actively generating chat.
- [x] Run the repository's applicable format, lint, typecheck, test, and build checks.

The unresolved composer-draft gap and the iOS edge-swipe stretch goal do not block core completion.
