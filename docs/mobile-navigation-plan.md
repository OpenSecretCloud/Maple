# Mobile Navigation Plan

## Status

Core implementation and the iOS edge-swipe stretch goal are complete on the `mobile-navigation`
branch. Navigation/history, stream-disconnect, and gesture decisions have focused automated
coverage, and the repository's format, lint, typecheck, test, and production-build checks pass. The
unchecked acceptance items below require interactive browser or physical iOS/Android validation and
remain the final release-validation pass. A mobile main-menu sizing audit is recorded below, but no
sizing changes are included pending a product decision.

## Objective

Give compact/mobile layouts a traditional page hierarchy while leaving the existing desktop-width experience unchanged:

- The existing menu becomes a full-screen mobile main menu.
- Opening a chat or project pushes a detail page over its parent page.
- New Chat and chats started from it retain a top-left menu button; destination-based detail pages
  use a top-left back button.
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
- A fresh iOS or Android app process starts on New Chat, matching the pre-navigation behavior.
- New Chat shows the top-left hamburger/menu button.
- After the first message turns New Chat into a conversation, that conversation retains the
  hamburger/menu button for that navigation entry.
- The hamburger button opens the full-screen main menu directly, even when New Chat was opened from
  another detail page.
- A chat selected from the main menu or project detail shows a top-left back arrow.
- A chat loaded directly from its URL also shows a back arrow, with the main menu as its in-app
  fallback.
- Existing conversation headers retain the wordmark, conversation title, and New Chat action.
- The empty new-chat page does not show a redundant New Chat action.
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

- Keep non-chat parent surfaces such as the main menu and project detail mounted while a child page
  is visible.
- Hide and make those covered parent surfaces non-interactive and inaccessible to assistive
  technology.
- Preserve main-menu scroll position, expanded projects, search state, and selection state while a child page is open.
- Preserve project-detail state while a chat opened from that project is visible.
- Do not keep a chat mounted merely because it is a parent navigation entry.
- Unmount a chat after its exit transition completes; if it is replaced by New Chat or another
  chat, unmount it when it leaves the visible flow.

Keeping non-chat parent surfaces mounted is intentional and matches the useful state-preservation
part of a native navigation stack. It does not keep a covered or popped chat loaded.

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
- A fresh native iOS or Android launch is the exception: it normalizes to `/` and opens transient
  New Chat above the main-menu root.

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
- The hamburger button opens the main menu before or after the first send.
- Reloading `/` on mobile returns to the main menu.

The exact internal `history.state` shape is an implementation detail and should be centralized rather than spread across components.

## Back Navigation

All mobile surfaces use one shared back-navigation flow. Do not design separate flows for mobile web, iOS, or Android.

- The top-left back arrow returns to the previous destination-based in-app page.
- The top-left hamburger button on New Chat and chats started from it opens the main menu directly.
- A chat opened from the main menu returns to the main menu.
- A chat opened from project detail returns to project detail.
- A chat loaded directly from a URL with no in-app parent returns to the mobile main menu instead of sending the user out of Maple.
- The iOS left-edge gesture follows the visible control: it pops to the previous page from a Back
  state and opens the main menu directly from a hamburger state.
- Browser back/forward navigation and the in-app back button must resolve through the same centralized navigation state.
- Do not plan Android-specific native navigation handling. Verify the shared browser-history behavior on Android and address only demonstrated platform bugs.

## App Lifecycle

### Web

- The URL is authoritative at every viewport width.
- Refreshing or reopening the current web URL loads the page represented by that URL.

### iOS and Android apps

- If the app process remains in memory, preserve the current page through backgrounding and foregrounding.
- If the app process launches fresh, start on New Chat above the mobile main-menu root.
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

The core implementation uses a paired page transition modeled on the standard iOS navigation
controller push/pop motion:

- Forward navigation slides the child page in from the right while shifting its parent partially
  off the left edge.
- Back navigation slides the child page out to the right while returning its parent from the left.
- The popped page unmounts after its exit transition completes.
- Respect `prefers-reduced-motion` by removing or minimizing nonessential animation.
- Keep transition state centralized in the mobile navigation shell.

## Compact Settings Navigation

Compact Settings follows the same root/detail hierarchy and paired motion:

- Opening Settings from the compact main menu pushes the entire Settings surface over the mounted
  main menu using the same paired parent/child motion.
- Back from the Settings menu pops the entire Settings surface and reveals the preserved main menu.
- `/settings` is the full-screen Settings menu rather than a drawer over Account settings.
- Selecting a category pushes its existing detail route over the mounted Settings menu.
- The detail header uses a top-left back arrow that returns to the Settings menu.
- Browser history back to the Settings menu uses the same paired pop animation.
- A directly loaded Settings detail URL falls back to `/settings` from the in-app back button.
- Existing nested category routes, navigation locks, sign-out behavior, and persistent return to the
  prior home surface remain unchanged.
- Desktop-width Settings keeps its existing two-column navigation and detail layout.

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
4. Keep non-chat parent surfaces mounted and mark covered pages inert/hidden appropriately; do not
   retain covered chats.
5. Preserve the existing persistent-home URL capture and return flow used by settings routes.

### Phase 3: Page headers and back flow

1. Keep the hamburger/menu button on New Chat and conversations started from it.
2. Use the shared back button for chats selected from the menu or project detail.
3. Replace the mobile project-detail hamburger with the same back button.
4. Implement the direct-URL fallback to the main menu.
5. Synchronize header controls, browser history back/forward, and transient new-chat history.

### Phase 4: Unmount lifecycle

1. Unmount chat pages after they are popped or replaced by another chat/New Chat surface.
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
3. Ensure a fresh native launch starts on New Chat.
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

### Mobile main-menu sizing audit — decision pending

The full-screen menu currently reuses the desktop sidebar's dimensions as well as its content. That
keeps the codebase simple, but it means the content grows from a roughly 296-pixel-wide rail to a
roughly 390–430-pixel-wide phone surface while most vertical dimensions remain desktop-dense.

Measured from the current shared component styles:

- Base menu and list text is 14 pixels; section headings are 12 pixels.
- New Chat, Search, and New Project are approximately 32–33 pixels tall with 16-pixel icons.
- Project and chat rows are approximately 29 pixels tall.
- Visible mobile row-overflow controls are approximately 28 by 28 pixels.
- Search and Settings controls are 36 pixels tall; the search clear control has only a 16-pixel
  icon-sized hit area.
- Usage-card copy ranges from 9 to 10 pixels.
- The page-mode header loses the desktop-only 36-pixel close control, leaving its height largely
  defined by the 16-pixel wordmark and 12/8-pixel vertical padding.
- The history area uses 16 pixels of left inset and 8 pixels of right inset on mobile.

This supports the physical-device feedback that the full-screen menu feels too small: its available
width increases substantially, but its type, icons, row heights, and tap targets do not. Most of the
primary interactive targets are also below the familiar 44-point iOS touch-target convention.

If a sizing pass is approved, the smallest coherent change would use compact-layout responsive
classes inside the existing shared components rather than creating mobile-only menu markup:

1. Give primary actions, search, project/chat rows, overflow controls, and Settings a 44-pixel
   minimum touch target on compact layouts.
2. Raise primary and list labels to 16 pixels and their icons to roughly 18–20 pixels.
3. Give the search clear action its own full touch target and expand row title clearance alongside
   the larger overflow control.
4. Make the page-mode header at least 44 pixels tall, increase the wordmark modestly, and use
   symmetric 16–20-pixel horizontal insets.
5. Keep 12-pixel section headings, but raise usage-card copy to roughly 11–12 pixels.
6. Verify bottom safe-area spacing, short landscape, and accessibility text sizing on devices.

No recommendation above is implemented in the current branch. Desktop dimensions can remain
unchanged by scoping any approved sizing adjustments to the existing compact-layout presentation.

### Unsent composer state

The implemented behavior discards unsent composer state when a chat or transient New Chat page is
unmounted. Whether that state should be preserved is an unresolved follow-up, not part of this
feature's definition of done.

Relevant transient state includes:

- Typed but unsent text
- Selected images
- Selected documents
- Composer-specific UI state

An in-memory `draftMessages` mechanism exists in local state, but `UnifiedChat` does not currently
use it. This implementation does not connect, remove, or redesign that unused state. Preserving
unsent text or attachments should be considered separately.

## Stretch Goal: iOS Edge-Swipe Back

Implemented as an interactive left-edge swipe-back gesture for the iOS Tauri app. The same shared
gesture tracker is used for chat/project navigation, Settings detail-to-menu navigation, and
Settings menu-to-home navigation.

Wry 0.55.1 does not expose built-in back/forward navigation gestures on iOS, so this requires an app-level gesture rather than a configuration switch.

Expected behavior:

- Begin only from the left screen edge.
- Track horizontal finger movement while rejecting primarily vertical gestures.
- Move the current page with the finger and reveal its destination underneath.
- In a back-arrow state, reveal and pop to the previous mounted page.
- In a hamburger state, reveal and open the main menu directly, skipping intermediate detail
  layers.
- Complete based on distance and/or velocity.
- Snap back cleanly when canceled.
- Use the same centralized back action as the header button.
- Unmount the current chat after the completed gesture.
- Avoid intercepting controls or horizontally scrollable content away from the left-edge activation area.

Implementation details:

- The gesture activates only within the leftmost 28 pixels.
- It locks after 8 pixels of primarily rightward movement and yields to primarily vertical movement.
- It completes at 35% of the screen width or with sufficient rightward release velocity; otherwise,
  it animates back to the current page.
- It reuses the existing history/back destinations and skips a second non-interactive pop animation
  after the finger-driven transition completes.
- A hamburger-state gesture crosses any transient in-app history entries and normalizes the root
  destination to the main menu.
- A previous chat is mounted only when needed to reveal it during an interactive gesture. Canceling
  the gesture unmounts that preview; completing it leaves the popped chat unmounted.
- Navigation locks can opt a surface out of gesture capture, and other controls can use
  `data-swipe-back-ignore` if a future left-edge interaction needs priority.

Do not install this custom gesture in mobile Safari; Safari owns its browser navigation gesture. No Android-specific equivalent is planned.

Physical-device verification remains required because the gesture is intentionally disabled outside
the iOS Tauri runtime.

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
- Changing persistent return-to-home behavior
- Changing Agent Mode availability or navigation
- Adding draft persistence
- Implementing the mobile main-menu sizing recommendations before product approval

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

- [x] A fresh mobile web root load shows the full-screen main menu.
- [x] A fresh native app process starts on New Chat above the main-menu root.
- [x] The mobile main menu has no sidebar collapse control.
- [x] New Chat opens a transient full-screen new-chat page without changing the URL.
- [x] Existing chats open as full-screen detail pages using the existing `conversation_id` URL.
- [x] Project detail uses the existing `project_id` URL.
- [ ] Project rows still expand and collapse inline in the menu.
- [x] Parent-page menu and project state are preserved while a child page is visible.

### Back behavior

- [x] New Chat and chats started from it show the hamburger/menu button.
- [x] Chats selected from the menu or project detail and project-detail pages show the back button.
- [x] The hamburger opens the main menu directly, including from a project-scoped New Chat.
- [x] Back returns to the correct previous in-app page.
- [x] Directly loaded chat URLs fall back to the mobile main menu from the in-app back button.
- [x] Browser back and forward remain synchronized with the visible mobile page.
- [x] Mobile web, iOS, and Android use the same navigation logic.

### URL and lifecycle behavior

- [x] No new route or query parameter is introduced.
- [x] Web refresh reloads the current chat or project URL at every viewport width.
- [x] Refreshing mobile `/` shows the main menu rather than reconstructing transient New Chat.
- [x] Backgrounding and foregrounding an in-memory native app preserves its current page.
- [x] A fresh native app process starts on New Chat.

### Chat lifecycle

- [x] A chat unmounts after it leaves the screen.
- [x] Leaving a generating chat does not call the cancel-response operation.
- [x] Reopening a generating chat catches up without resubmitting the prompt.
- [x] Reopening after generation completes shows the completed result.
- [x] Switching between chat, New Chat, project detail, and the menu does not create duplicate conversations or messages.

### Motion and accessibility

- [ ] Forward and back slide transitions work in portrait and short landscape.
- [x] Parent menu/project surfaces shift left on push and return on pop.
- [x] Reduced-motion users receive a suitable non-animated or minimized transition.
- [x] Covered parent pages are inert and hidden from assistive technology.
- [x] Focus moves predictably on push and pop.
- [ ] Safe areas and the mobile keyboard do not obscure navigation controls.

### iOS edge-swipe back

- [x] The gesture is limited to the iOS Tauri app and begins only at the left edge.
- [x] Horizontal movement tracks the finger while primarily vertical movement is rejected.
- [x] Distance and velocity determine completion, and canceled gestures snap back.
- [x] Chat, project, Settings detail, and Settings-root flows use their existing back destinations.
- [x] Hamburger-state chat surfaces reveal and open the main menu directly.
- [x] A completed gesture does not replay the non-interactive pop animation.
- [x] Popped chats unmount after completion, and canceled chat previews unmount after snap-back.
- [ ] Verify tracking, completion, cancellation, and vertical-scroll rejection on a physical iPhone.

### Compact Settings

- [x] Opening and closing Settings uses the same paired push/pop treatment as chat navigation.
- [x] `/settings` renders the full-screen Settings menu on compact layouts.
- [x] Settings categories push existing detail routes over the mounted menu.
- [x] Settings detail back returns to the menu through shared browser history when available.
- [x] Directly loaded Settings detail routes fall back to the Settings menu.
- [x] Settings navigation locks continue to block unsafe navigation.
- [x] Desktop-width Settings retains its existing two-column layout.

### Validation

- [ ] Validate just below and at the existing desktop breakpoint.
- [ ] Validate representative portrait and short-landscape phone viewports.
- [ ] Validate mobile web refresh and browser back/forward.
- [ ] Validate iOS suspend/resume and fresh launch.
- [ ] Validate Android suspend/resume and fresh launch.
- [ ] Validate navigation away from and back to an actively generating chat.
- [x] Run the repository's applicable format, lint, typecheck, test, and build checks.

The unresolved composer-draft gap does not block completion. Physical iOS gesture validation remains
part of the final release-validation pass.
