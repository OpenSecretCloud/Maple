# Mobile Navigation Plan

## Status

Core implementation, the iOS edge-swipe stretch goal, and the compact main-menu sizing pass are
complete on the `mobile-navigation` branch. Navigation/history, stream-disconnect, gesture, and
compact Settings decisions have focused automated coverage. The full-screen menu now uses
phone-scale controls while retaining the same menu implementation and the original 296-pixel
desktop sidebar presentation. Unchecked acceptance items below still require authenticated
interactive browser or physical iOS/Android validation.

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

### Mobile main-menu sizing audit — implemented

The initial full-screen menu reused the desktop sidebar's dimensions as well as its content. That
kept the codebase simple, but it meant the content grew from a roughly 296-pixel-wide rail to a
roughly 390–430-pixel-wide phone surface while most vertical dimensions remained desktop-dense.

The pre-sizing audit measured:

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

The implemented pass scopes proportional sizing to `MainMenu`'s page presentation and passes that
presentation state through the existing shared history, account, and usage components. It does not
add a second menu or change the responsive breakpoints.

- The full-screen header, New Chat, Search, New Project, project rows, chat rows, overflow buttons,
  Settings, selection actions, selection hit areas, and context-menu actions use a 44-pixel minimum
  target.
- Primary and list labels are 16 pixels with a 24-pixel line height; their primary icons,
  disclosure icons, pinned indicators, and overflow icons remain 20 pixels. Section headings remain
  12 pixels, with 6 pixels of additional separation before the first row in each section.
- The search field is 44 pixels tall with 48 pixels of trailing clearance, and its clear action has
  a real 44-by-44-pixel hit area.
- Chat and project title regions end at the enlarged 44-pixel overflow control and fade over their
  final 16 pixels, so long titles cannot sit beneath the control or reappear beyond it.
- The page header is at least 44 pixels tall and uses a 20-pixel-high wordmark.
- Page-level horizontal insets are a symmetric 20 pixels. The desktop-only workspace-mode switch is
  not rendered in page mode.
- Mobile usage copy is 12 pixels, with 11-pixel plan/API labels. The account row aligns that card
  with an exact 44-by-44-pixel Settings control.
- Portaled menu content is constrained to the viewport, or to the native iOS safe-content frame
  when the page is full bleed. It scrolls vertically when necessary, uses 16-pixel collision
  padding, and gives its menu items 44-pixel targets with 16-pixel labels and 20-pixel icons.
- The footer uses the greater of 16 pixels or the device bottom safe-area inset. In short landscape,
  the full menu surface becomes the vertical scroller and the decorative history tail/fade
  contracts, so every section remains reachable.

All original desktop sizing branches remain in place: the 296-pixel sidebar retains its 14-pixel
labels, 16-pixel icons, dense rows, 36-pixel Settings control, original asymmetric history padding,
and existing overflow behavior.

Validation for this pass:

- The complete frontend suite passes with 712 tests, including focused mobile-navigation,
  response-lifecycle, Settings-navigation, swipe, and history-pagination coverage.
- TypeScript, formatting, lint, and the production build pass. Lint remains at the existing
  13-warning/zero-error baseline.
- The production CSS contains the 44-pixel height/width/min-height utilities, mobile title clipping
  and fade, viewport-limited dropdown height, and bottom safe-area expression used by page mode.
- A source-level branch audit confirmed that the new dimensions are reachable only through
  `presentation="page"` / `pagePresentation`; the desktop sidebar retains its original class
  branches. The desktop-only workspace-mode switch is omitted from page mode.
- Authenticated interactive viewport validation was not available in this workspace:
  `opensecret-workspaces` is not installed, the assigned OpenSecret/Billing/Maple ports had no
  listeners, and the remote branch preview required a Cloudflare Access sign-in. The representative
  portrait, short-landscape, breakpoint, light/dark, safe-area, and physical-device checks therefore
  remain unchecked below rather than being claimed from source inspection.

### Unsent composer state

The current runtime store retains a scope-specific, offscreen New Chat draft in memory, including
its composer resources, and resumes it when that scope is opened again. The mobile navigation shell
reuses that current-master behavior; it does not add persistent, cross-process draft storage or a
new draft model.

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

### Physical iPhone follow-up — August 14, 2026

A physical-iPhone recording exposed two implementation defects before final device sign-off:

- Nine deliberate edge swipes—including fast flicks, slow drags well beyond the 35-percent
  threshold, and a drag held at the fully revealed position—tracked the correct menu destination
  but returned to the chat on release.
- A native development launch could render the main menu because the one-process New Chat claim was
  mutated during React render. Strict Mode or another abandoned render could consume that claim
  before the committed navigation shell initialized.

The follow-up implementation now:

- Treats an iOS `pointercancel` after horizontal lock as release at the last position shown to the
  user, completing only when that visible distance crossed the normal threshold. Unexpected capture
  loss still cancels, while ordinary pointer-up uses both distance and recent velocity from the last
  reliable sample instead of a potentially reset WebKit release coordinate.
- Applies vertical-pan ownership throughout the active native navigation surface, while code blocks,
  tables, and range controls that intentionally use horizontal gestures opt out. Portaled controls
  outside that surface are rejected before gesture capture.
- Updates interactive transforms through hook-scoped CSS properties instead of rerendering the full
  chat or Settings tree for every pointer sample.
- Removes completed push-animation classes after 320 milliseconds and prevents a swipe during the
  active entrance, so canceling a later gesture cannot restart the original push animation.
- Peeks at the fresh-native-launch claim during render and commits it only from the mounted layout
  effect, preserving New Chat across Strict Mode and abandoned-render retries.

Focused launch/gesture tests and the complete frontend test suite cover the new launch gate and
interrupted-settlement rules. A subsequent physical-iPhone build confirmed smooth tracking and
completion for full-distance drags and fast hamburger-state flicks. Short cancellation, vertical
edge scrolling, Back-button taps, project-backed destinations, and both Settings levels still need
device verification.

### Physical iPhone menu follow-up — August 14, 2026

The first physical-menu screenshots exposed three additional issues:

- Compared with the 402-by-874-point reference, Maple's 16-pixel labels and 16-pixel page insets
  remained visually too close to its desktop-sidebar proportions. The existing approximately
  52-pixel row cadence already matched the reference and did not need to grow.
- The iPhone's top and bottom safe-area bands used the chat document background instead of the menu
  background. Pixel inspection showed the exact dark-theme values: `#0A0A0A` in the safe areas and
  `#262626` on the menu.
- After a transient New Chat became a conversation, opening either global New Chat or New Chat in
  Project could select that conversation again. The pushed mobile history entry inherited the old
  draft key, which intentionally remained an alias to the newly created conversation.

The follow-up implementation:

- Uses 20-pixel labels and symmetric 20-pixel insets only in full-screen page presentation. It
  keeps the existing font family, 12-pixel section headings, icons, buttons, 44-pixel targets,
  overflow clearance, and row cadence unchanged.
- Lets the active or interactively revealed menu own the document canvas color, including the iOS
  safe-area bands. Chat, project, and Settings surfaces retain their existing background, and no
  global viewport or native-window configuration changed.
- Ignores an inherited browser-history draft key only when initializing a stack-owned transient
  mobile New Chat. The existing scope-aware draft selector then creates a fresh root/project draft
  after a conversation materializes or resumes a legitimate unsent draft. Desktop and direct URL
  restoration remain history-authoritative.

Source review, focused regressions, and the complete 705-test frontend suite passed at that stage.
The next physical build confirmed edge-to-edge menu painting and both global and project-scoped New
Chat entry points, while exposing the refinements below.

### Second physical iPhone menu follow-up — August 14, 2026

The next physical recording and screenshots established four remaining issues:

- The safe-area canvas switched to the destination color at transition start. During an edge swipe,
  the top and bottom bands changed from the chat background to the menu background one frame after
  horizontal lock, while the chat still covered almost the entire viewport.
- Mobile title generation reused the desktop title-flash animation, which painted a translucent
  background, padding, and scale behind the changing title.
- Selecting an uncached historical conversation after creating a chat could render the just-created
  runtime. First-mount projection fell back to the store's previously active key before the requested
  conversation had been created in the runtime cache.
- The 20-pixel page labels measured substantially larger than the reference's list typography, and a
  long title could continue beneath the overflow button and visibly reappear on its far side.

The follow-up implementation:

- Uses the settled/source page as the document-canvas owner until a push, pop, or interactive swipe
  commits. Menu and Settings-root surfaces use the sidebar canvas color; Chat, New Chat, project, and
  Settings-detail surfaces retain the application background. This keeps each visible screen's top
  and bottom safe areas consistent throughout the transition.
- Uses a mobile-only title text animation without the desktop background, padding, radius, or scale.
  Desktop title animation remains unchanged.
- Projects and creates a requested conversation runtime on a chat page's first mount. The existing
  selected-runtime fallback remains available only after that page has committed, preserving the
  synchronous deletion/replacement guard.
- Returns full-screen page labels to 16 pixels with a 24-pixel line height while retaining the
  20-pixel page insets, 44-pixel controls and rows, 20-pixel icons, 12-pixel headings, and existing
  vertical cadence. Mobile project and chat titles now clip at the 44-pixel overflow-control
  boundary and fade over their final 16 pixels; their complete accessible labels remain available.
  Desktop typography, spacing, title clipping, overflow presentation, and sidebar dimensions are
  unchanged.

Focused canvas-ownership, Settings-navigation, and runtime-projection tests pass, and the complete
frontend suite passes with 712 tests. TypeScript, formatting, the PR-configured production web build,
and the pinned iOS simulator bundle build pass. Lint remains at the existing 13-warning/zero-error
baseline, with no warning in a touched file. A physical-device rerun of the revised canvas timing,
title animation, 16-pixel typography, long-title fade, historical-chat load, short cancellation, and
Settings gestures remains pending.

### Third physical iPhone navigation follow-up — August 14, 2026

The next two physical recordings showed that matching the document canvas to the current page was
not sufficient. WKWebView still laid out every moving React surface only inside the safe-content
rectangle, so the page boundary moved between the status bar and home indicator while the top and
bottom bands stayed fixed and changed color only after the transition committed. The same build
also exposed a second settle during Settings entry and a Settings Back race that could briefly
reveal the menu before returning to the previously viewed chat.

The revised native-iOS compact presentation now:

- Adds `viewport-fit=cover` dynamically after exact native-iOS detection and before React renders,
  using the existing compact width and short-landscape predicates. Mobile web, Android, wide iPad,
  and desktop layouts keep their original viewport metadata and breakpoints.
- Gives every transformed menu, chat, New Chat, project, Settings-menu, and Settings-detail layer a
  full physical-screen background. Content is independently inset by all four iOS safe-area values,
  so the page boundary travels through the status and home-indicator regions without moving controls
  into them. Existing footer/composer safe padding is neutralized only inside this native frame to
  avoid double bottom insets.
- Keeps native-reachable static auth, reset, verification, callback, fallback, and Agent screens,
  fixed notifications, the marketing header, and team alerts inside the same safe-content bounds.
  Portaled menus and selects collide against the actual safe frame rather than the physical screen.
- Disables the base transform transition only while the native Settings push keyframe owns entry,
  eliminating the WebKit class-removal rebound while preserving pop, interactive swipe, reduced
  motion, mobile-web, and Android behavior.
- Animates a Settings Back-button close before changing history. It returns through the marked menu
  entry with one blocker-free Back, replaces a markerless/direct entry with `/`, and commits only if
  the raw browser URL and history-entry key still match the initiating Settings page. A concurrent
  browser Back, deep link, duplicate close, or unmount therefore cannot perform a late second Back;
  an invalidated close also restores the Settings shell instead of leaving it popped or inert.

Focused viewport, mobile-navigation, swipe, Settings-navigation, and runtime tests pass. The complete
local frontend suite passes with 723 tests; typecheck, formatting, the production build, and lint
also pass, with lint unchanged at 13 existing warnings and no errors. Source review confirms the
full-bleed rules require the exact native-iOS compact marker and do not alter the 296-pixel desktop
sidebar. A physical-iPhone rerun of edge-to-edge push/pop/swipe boundaries, Settings entry, the
chat-to-menu-to-Settings Back sequence, rotation/short landscape, portaled menus, and global overlays
remains pending.

### Fourth physical iPhone interaction follow-up — August 14, 2026

The full-bleed physical build confirmed that ordinary edge drags and flicks were smooth, but one
deliberately reversed gesture still exposed two completion discontinuities. The chat moved from 259
pixels revealed back to 29 pixels, then the final rightward flick reached 177 pixels before release.
The next frame jumped to 276 pixels, and after reaching the full 294-pixel width the menu briefly
fell back to 238 pixels before settling again. The same test pass also found that tapping another
row overflow button while a popover was open could leave both menus visible instead of transferring
the interaction in one tap.

The follow-up implementation now:

- Tracks the last swipe position actually written to CSS separately from the latest coalesced pointer
  sample. Completion still uses the latest sample for intent, but its duration starts from the
  position currently visible on screen, so an unpainted final flick cannot shorten and jump the
  settle animation.
- Keeps the hook-scoped swipe properties in place until React has removed their inline transform
  consumers. This prevents the revealed menu from falling back to its covered `-24%` transform
  between the interactive history commit and the final render.
- Makes chat/project overflow menus nonmodal only in full-screen page presentation and controls them
  with one shared exclusive key. An outside tap dismisses the current menu without blocking its
  target; tapping another overflow button transfers ownership immediately, and a delayed close from
  the old menu cannot close the new one. Desktop dropdown modality and uncontrolled state remain
  unchanged.

Focused hook and exclusive-menu regressions pass, and the complete frontend suite passes with 732
tests. TypeScript, touched-file formatting and lint, and diff checks pass. A physical-iPhone rerun of
the reversed-drag/final-flick sequence, ordinary cancel/complete gestures, outside popover dismissal,
chat-to-chat/project menu switching, nested submenus, and compact keyboard focus remains pending.

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

### Compact main-menu sizing

- [x] Page-mode primary, list, overflow, Settings, selection, and context-menu controls have
      44-pixel minimum targets.
- [x] Page-mode primary/list labels are 16 pixels and icons are 20 pixels; 12-pixel section headings
      retain their hierarchy.
- [x] The search clear action has a 44-by-44-pixel target and row titles clear enlarged overflow
      controls.
- [x] The page header is at least 44 pixels tall with a 20-pixel wordmark and symmetric 20-pixel
      horizontal insets.
- [x] Mobile usage copy is 11–12 pixels and aligns with an exact 44-by-44-pixel Settings control.
- [x] The menu footer accounts for the bottom safe area, and short-landscape page mode has a
      full-surface vertical-scroll fallback.
- [x] The 296-pixel desktop sidebar retains its existing sizing classes and behavior branches.
- [ ] Verify authenticated light/dark layouts, long titles, menus, selection, and safe areas on
      representative phone viewports and physical devices.

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
- [x] After a New Chat becomes a conversation, global New Chat and New Chat in Project select a
      blank or legitimately retained unsent draft instead of reopening that conversation.
- [x] Selecting an uncached historical chat after creating a conversation projects and loads the
      requested chat instead of retaining the just-created runtime.

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
- [x] A physical iPhone confirms smooth tracking and completion for full-distance and fast-flick
      hamburger-state gestures.
- [ ] Re-verify short cancellation, vertical-scroll rejection, Back-button taps, final canvas
      timing, project-backed destinations, and Settings gestures on a physical iPhone.

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

The unresolved composer-draft gap does not block completion. Post-fix physical iOS gesture and fresh
launch validation remain part of the final release-validation pass.
