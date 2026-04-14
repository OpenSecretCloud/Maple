# Maple Product Redesign Reimplementation Spec

> Source reference: GitHub PR #465 (`feature/frontend-ui-marketing-updates`)
>
> Purpose: extract the designer's authenticated product redesign into an engineering-ready spec for a clean new implementation.
>
> This document is intentionally **not** a request to merge or cherry-pick PR #465. It translates the visual wins from that PR into a product-only, standards-aligned plan that preserves existing Maple functionality.
>
> Important: PR #465 was produced from an older fork. Current `master` is the canonical source of truth for functionality, routes, copy, and any product additions made after that fork. The redesign must be reapplied onto current `master`, not the other way around.

## Design-Only Goals and Non-Goals

This redesign effort is **design-only**. It is not a product rewrite, feature rewrite, or behavior rewrite.

### Goals

- Recreate the strongest authenticated-product visual ideas from PR #465 on top of current `master`.
- Improve the visual design of the logged-in product experience through typography, spacing, color, hierarchy, radius, iconography, theming, and component/dialog chrome.
- Reimplement those visual improvements using Maple's normal engineering standards, shared tokens, shared primitives, and existing architectural patterns.
- Keep the work tightly focused on the authenticated product surfaces and the dialogs/components that are reachable from them.

### Non-Goals

- No feature changes.
- No product behavior changes.
- No route, navigation, or query-param contract changes.
- No event contract changes.
- No modal/dialog open, close, trigger, or priority logic changes.
- No billing, account, team, API, auth, persistence, or platform-behavior changes.
- No addition, removal, or simplification of existing capabilities just because the designer PR made them look quieter.
- No marketing, logged-out, pricing, helper, or other public-page redesign in this effort.
- No logic refactors unless they are strictly required to support the visual implementation and preserve behavior exactly.

If a proposed change cannot be justified as a pure design/chrome/presentation improvement while preserving current behavior, it is out of scope for this redesign.

---

## 1. Working Rules

1. Do **not** copy PR #465 wholesale.
2. Do **not** bring marketing, pricing, proof, downloads, solutions, or helper pages into the new product redesign PR.
3. Do preserve the visual ideas that make the designer PR good.
4. Do preserve existing product capabilities, behavior, and handler logic exactly.
5. When the raw PR and this spec disagree, follow this spec.
6. Prefer Maple's existing frontend patterns: React function components, shadcn/Radix primitives, Tailwind tokens, `@/` imports, TanStack Router, React Query, and current state/event contracts.
7. Treat current `master` as canonical for product behavior, routes, copy, and newer additions. Reapply the designer's visual changes onto `master`.
8. Do not let older-fork PR content overwrite newer `master` behavior/content. Example: if privacy and terms already exist on `master`, they stay exactly as `master` defines them.

---

## 2. What Was Reviewed

### 2.1 Designer PR inputs

- PR metadata and full changed-file list for PR #465
- PR diffs for:
  - `frontend/src/index.css`
  - `frontend/tailwind.config.js`
  - `frontend/index.html`
  - `frontend/src/app.tsx`
  - `frontend/src/contexts/ThemeContext.tsx`
  - `frontend/src/components/Sidebar.tsx`
  - `frontend/src/components/ChatHistoryList.tsx`
  - `frontend/src/components/UnifiedChat.tsx`
  - `frontend/src/components/AccountMenu.tsx`
  - `frontend/src/components/AccountDialog.tsx`
  - `frontend/src/components/CreditUsage.tsx`
  - `frontend/src/components/ModelSelector.tsx`
  - `frontend/src/components/markdown.tsx`
  - product-reachable dialogs and account/team/API dashboard surfaces
  - `frontend/src/routes/_auth.chat.$chatId.tsx`

### 2.2 Current product inputs

- `frontend/src/routes/index.tsx`
- `frontend/src/components/ProjectDetailView.tsx`
- `frontend/src/state/LocalStateContext.tsx`
- `frontend/src/state/LocalStateContextDef.ts`
- current implementations of the same product-side files above

---

## 3. Scope

## 3.1 Primary in-scope surfaces

These are the surfaces the redesign PR should actively target.

- Authenticated home shell in `frontend/src/routes/index.tsx`
- `frontend/src/components/UnifiedChat.tsx`
- `frontend/src/components/Sidebar.tsx`
- `frontend/src/components/ChatHistoryList.tsx`
- `frontend/src/components/AccountMenu.tsx`
- `frontend/src/components/AccountDialog.tsx`
- `frontend/src/components/CreditUsage.tsx`
- `frontend/src/components/ModelSelector.tsx`
- `frontend/src/components/markdown.tsx`
- `frontend/src/routes/_auth.chat.$chatId.tsx` (archived chat viewer)
- Shared product foundations used by those surfaces:
  - `frontend/src/index.css`
  - `frontend/src/chat.css`
  - `frontend/tailwind.config.js`
  - `frontend/index.html`
  - `frontend/src/app.tsx`
  - `frontend/src/contexts/ThemeContext.tsx`
  - relevant shadcn primitives

## 3.2 Product-reachable dialogs and secondary surfaces in scope

These are reachable from the logged-in product and should receive redesign polish where the PR provides direction.

- `DocumentPlatformDialog`
- `ContextLimitDialog`
- `DeleteChatDialog`
- `BulkDeleteDialog`
- `WebSearchInfoDialog`
- `TTSDownloadDialog`
- `UpgradePromptDialog`
- `PromoDialog`
- `VerificationModal`
- `GuestPaymentWarningDialog`
- `RecordingOverlay`
- Team/API/account dashboards reachable from the account menu:
  - `frontend/src/components/apikeys/ApiCreditsSection.tsx`
  - `frontend/src/components/apikeys/ApiKeyDashboard.tsx`
  - `frontend/src/components/apikeys/ApiKeysList.tsx`
  - `frontend/src/components/apikeys/CreateApiKeyDialog.tsx`
  - `frontend/src/components/apikeys/ProxyConfigSection.tsx`
  - `frontend/src/components/team/TeamDashboard.tsx`
  - `frontend/src/components/team/TeamInviteDialog.tsx`
  - `frontend/src/components/team/TeamMembersList.tsx`

## 3.3 Compatibility surfaces

These are part of the authenticated product flow and must not regress, but the designer PR does **not** provide enough direct design direction to justify a full rewrite.

- `frontend/src/components/ProjectDetailView.tsx`
- `frontend/src/components/ConversationProjectPicker.tsx`
- Project creation/rename/delete/move dialogs
- Existing project-focused route behavior (`project_id` search param flow)

Default rule for these surfaces:

- Keep current structure and behavior.
- Let shared token, typography, radius, button, and dialog improvements bring them closer to the new system.
- Do not invent a new project UX unless separately specified.

## 3.4 Explicitly out of scope

Ignore these PR #465 areas when building the new product redesign PR.

### Marketing / logged-out shell

- `frontend/src/components/Marketing.tsx`
- `frontend/src/components/MarketingSiteHome.tsx`
- `frontend/src/components/TopNav.tsx`
- `frontend/src/components/Footer.tsx`
- `frontend/src/components/SimplifiedFooter.tsx`
- `frontend/src/components/ComparisonChart.tsx`
- `frontend/src/components/VerticalLandingMock.tsx`
- `frontend/src/components/Explainer.tsx`

### Marketing/helper/public routes

- `frontend/src/routes/about.tsx`
- `frontend/src/routes/agent.tsx`
- `frontend/src/routes/downloads.tsx`
- `frontend/src/routes/pricing.tsx`
- `frontend/src/routes/proof.tsx`
- `frontend/src/routes/redeem.tsx`
- `frontend/src/routes/research.tsx`
- all `frontend/src/routes/solutions*.tsx`
- `frontend/src/routes/teams.tsx`
- `frontend/src/routes/team.invite.$inviteId.tsx`
- `frontend/src/config/pricingConfig.tsx`

### Helper/signup surfaces outside the authenticated product flow

- `frontend/src/components/GuestCredentialsDialog.tsx`
- `frontend/src/components/GuestSignupWarningDialog.tsx`

### Generated / debug / not to be hand-copied

- `frontend/src/routeTree.gen.ts`
- `frontend/src/components/BillingDebugger.tsx`

---

## 4. Existing Product Invariants That Must Not Regress

This is the most important engineering section in this document.

The designer PR is visually valuable, but the current Maple product already supports more behavior than the redesign diff directly talks about. The new implementation must preserve those behaviors.

## 4.1 Routing and URL contracts

Keep the current authenticated routing model.

- `routes/index.tsx` still decides between:
  - marketing for logged-out users
  - `ProjectDetailView` when `project_id` is present without `conversation_id`
  - `UnifiedChat` otherwise
- `conversation_id` and `project_id` remain part of the app contract.
- The archived read-only route `/_auth.chat.$chatId` stays intact.

## 4.2 Event contracts

Preserve the existing custom/window event coordination model unless deliberately replaced everywhere.

Examples already used in the app:

- `newchat`
- `conversationselected`
- `projectselected`
- `conversationcreated`
- bulk dialog open events

A product redesign is not permission to casually break those event flows.

## 4.3 Sidebar/history capabilities to preserve

Do **not** remove these without explicit product approval.

- Projects/folders
- Pinned chats
- Recent chats
- Archived chats
- Search
- Bulk select
- Bulk delete
- Bulk move
- Long-press selection on mobile
- Pull-to-refresh
- Infinite scroll/pagination
- Per-item rename/delete/project actions

Important: PR #465 only visibly restyles parts of the history list. It does **not** provide justification for flattening or removing the current project/pin model.

## 4.4 Composer/chat capabilities to preserve

The new chat UI must still support:

- Streaming assistant responses
- Reasoning/thinking blocks
- Tool call rendering
- Web search status rendering
- Model gating / upgrade paths
- Project picker behavior where currently supported
- Image attachments
- Document attachments
- Desktop/Tauri PDF support behavior
- Voice recording + transcription
- TTS playback/download behavior
- Cancel generation
- Fullscreen composer mode
- Pagination/loading older messages
- Conversation title refresh/update behavior

## 4.5 Account/billing/team/API capabilities to preserve

The account redesign must keep:

- Plan visibility
- Credit usage visibility
- Manage subscription
- Team management entry points
- API management entry points
- Profile/email verification
- Preferences/default system prompt flows
- Change password / delete account
- Delete history
- Support/privacy/terms/about links as approved

## 4.6 Platform behavior to preserve

Do not regress platform-specific handling already present in the app.

- Desktop vs mobile layout differences
- Tauri vs web link opening
- iOS-specific billing/API gating
- Tauri-only features such as local PDF/TTS flows

## 4.7 Master-first reconciliation rule

PR #465 is a redesign reference, not a competing source of truth.

- If `master` and the designer PR disagree on behavior, routing, copy, or content, `master` wins.
- Reimplementation should start from current `master` and layer the redesign on top.
- Anything added to `master` after the designer fork should be preserved unless there is an explicit product decision to replace it.
- Example: privacy/terms behavior and destinations already present on `master` remain canonical and must not be overwritten by the designer branch.

---

## 5. What Makes the Designer PR Good

These are the design qualities worth preserving.

## 5.1 Quieter product chrome

- The product feels less noisy.
- The sidebar looks less like a stack of controls and more like product navigation.
- Secondary controls become calmer and more intentional.

## 5.2 Softer geometry

- Rounder search fields
- Rounder composer shell
- Rounder message/tool/result cards
- Rounder account trigger and menu chrome

## 5.3 Stronger brand presence without looking like marketing

- Manrope gives the product a more designed, modern feel.
- The inline Maple wordmark is cleaner than image swapping.
- The coral/pebble palette feels branded but restrained.
- The `m-avatar.svg` assistant avatar is small but high-value.

## 5.4 Better visual hierarchy in chat

- Empty state is simpler and more confident.
- The composer looks like the primary object on the page.
- User and assistant messages are easier to scan.
- Tool states and web-search states look more intentional.

## 5.5 Better density in account/billing surfaces

- The ring-style credit meter is more compact and more premium.
- The plan badge and account entry point feel more polished.
- Dialogs benefit from more cohesive semantic color usage.

## 5.6 Better content readability

- Markdown links are calmer.
- Tables behave better on mobile.
- Thinking blocks are less visually heavy.
- Code/table corner radii are more consistent with the rest of the redesign.

---

## 6. Engineering Standards For The Reimplementation

## 6.1 Do not treat PR #465 code as production-ready source

Use the designer PR as a **visual reference**, not as a code-quality standard.

## 6.2 Prefer tokenized and reusable styling

If the same style pattern appears 2+ times, extract it.

Examples worth extracting during implementation:

- sidebar title fade constant
- sidebar ellipsis button constant
- product composer shell class set
- product icon-button class set
- assistant message shell
- user message bubble shell
- plan badge variant or helper

## 6.3 Keep behavior separate from chrome

Especially in `UnifiedChat` and `ChatHistoryList`:

- do not mix event/state/business changes with visual refactors unless necessary
- avoid deleting working product logic just because the PR diff did not touch it

## 6.4 Avoid making monoliths worse

Current files are already large.

During implementation, prefer extracting presentational pieces such as:

- `MapleChatAvatar`
- `AssistantMessage`
- `UserMessageBubble`
- `ComposerShell`
- `SidebarHeader`
- `SidebarHistoryRow`
- `AccountMenuTrigger`

## 6.5 Prefer shared primitives over raw markup, except where custom controls are justified

Use shadcn/Radix primitives by default.

Reasonable exceptions from the designer PR:

- the circular gradient send button
- the chromeless sidebar toggle/close controls

## 6.6 No raw color drift

Do not introduce fresh `text-green-*`, `text-red-*`, `text-blue-*`, `text-purple-*`, `bg-*` product styling if a semantic Maple token exists.

---

## 7. Foundation Design System Requirements

## 7.1 Theme architecture

Adopt class-based theming.

### Required changes

- `tailwind.config.js`: add `darkMode: "class"`
- create `ThemeProvider`
- store theme in `localStorage` under `maple-theme`
- support `light | dark | system`
- apply/remove `.dark` on `document.documentElement`
- set `document.documentElement.style.colorScheme`

### Required fixes beyond the raw PR

The PR direction is correct, but the implementation needs two fixes.

1. **Prevent FOUC.** Add an inline script in `frontend/index.html` `<head>` (before any stylesheets) that synchronously applies the theme class:

```html
<script>
  (function () {
    var stored = localStorage.getItem("maple-theme");
    var isDark =
      stored === "dark" ||
      (stored !== "light" &&
        window.matchMedia("(prefers-color-scheme: dark)").matches);
    if (isDark) {
      document.documentElement.classList.add("dark");
      document.documentElement.style.colorScheme = "dark";
    } else {
      document.documentElement.style.colorScheme = "light";
    }
  })();
</script>
```

2. **Keep `resolvedTheme` reactive.** The PR computes `resolvedTheme` as a derived value during render, which doesn't trigger re-renders when the OS theme changes in "system" mode. Fix: store `resolvedTheme` in `useState` and call `setResolvedTheme()` from the media query change handler.

Also remove `style="color-scheme: light dark"` from the `<html>` tag in `index.html` -- ThemeContext manages this now.

## 7.2 Typography

### Primary product font

- Use `Manrope` as the primary font for the product body UI.
- Keep existing decorative fonts like Mondwest available if already used elsewhere.

### CSS application

In `index.css`, add to the `body` rule inside `@layer base`:

```css
body {
  font-family: "Manrope", sans-serif;
  font-size: 14px;
}
```

### Font loading

Final implementation should **self-host `Manrope`** and serve it from Maple's own app/domain so Cloudflare can cache it at the edge.

Recommended production approach:

- add checked-in font assets (prefer WOFF2) under `frontend/public/fonts/` or equivalent
- load them with `@font-face` in `index.css`
- optionally preload the most important weights in `index.html`
- use `font-display: swap`

The Google Fonts snippet in PR #465 is useful as a reference for the intended family/weight range, but it should **not** be the final production dependency for the product UI.

## 7.3 Core color tokens

### Existing shadcn token changes (light mode)

These are the **existing** CSS variables that change value. The implementor must update these in the `:root` block of `index.css`.

| Token                  | Old Value              | New Value                   | Notes                          |
| ---------------------- | ---------------------- | --------------------------- | ------------------------------ |
| `--background`         | `40 30% 96%`           | `0 0% 98%`                  | Warm off-white -> pure neutral |
| `--foreground`         | `0 0% 12%`             | `0 0% 15%`                  | Slightly lighter body text     |
| `--card`               | `40 30% 96%`           | `0 0% 100%`                 | White cards                    |
| `--card-foreground`    | `0 0% 12%`             | `0 0% 15%`                  | Match foreground               |
| `--popover`            | `40 30% 96%`           | `0 0% 100%`                 | White popovers                 |
| `--popover-foreground` | `0 0% 12%`             | `0 0% 15%`                  | Match foreground               |
| `--primary`            | `0 0% 12%`             | `0 0% 9%`                   | Darker primary                 |
| `--primary-foreground` | `40 30% 96%`           | `0 0% 98%`                  | Pure neutral                   |
| `--secondary`          | `264 89% 69%` (purple) | `17 100% 72%` (coral)       | **Major**                      |
| `--accent`             | `264 89% 69%` (purple) | `17 100% 94%` (light coral) | **Major**                      |
| `--muted`              | `40 20% 90%`           | `0 0% 96%`                  | Pure neutral                   |
| `--muted-foreground`   | `0 0% 40%`             | `0 0% 45%`                  | Slightly lighter               |
| `--destructive`        | `0 80% 37%`            | `12 60% 54%`                | Matches maple-error            |
| `--border`             | `40 15% 85%`           | `0 0% 90%`                  | Pure neutral                   |
| `--input`              | `40 15% 85%`           | `0 0% 90%`                  | Match border                   |
| `--ring`               | `264 89% 69%` (purple) | `17 100% 72%` (coral)       | **Major**                      |

New token: `--destructive-on-filled: 0 0% 100%` (white text on filled destructive buttons).

### Neutral scale (new)

| Token           |        HSL |     Hex | Notes                             |
| --------------- | ---------: | ------: | --------------------------------- |
| `--neutral-50`  | `0 0% 98%` | #FAFAFA | default light page bg             |
| `--neutral-100` | `0 0% 96%` | #F5F5F5 | muted light fill                  |
| `--neutral-200` | `0 0% 90%` | #E5E5E5 | borders/inputs                    |
| `--neutral-300` | `0 0% 83%` | #D4D4D4 | subtle borders                    |
| `--neutral-400` | `0 0% 64%` | #A3A3A3 | dark muted text                   |
| `--neutral-500` | `0 0% 45%` | #737373 | light muted text                  |
| `--neutral-600` | `0 0% 32%` | #525252 | secondary text                    |
| `--neutral-700` | `0 0% 25%` | #404040 | dark chrome                       |
| `--neutral-800` | `0 0% 15%` | #262626 | light body text / dark sidebar bg |
| `--neutral-900` |  `0 0% 9%` | #171717 | dark cards                        |
| `--neutral-950` |  `0 0% 4%` | #0A0A0A | dark page bg                      |

### Maple semantic palette (new)

| Token                         |                                HSL | Role                               |
| ----------------------------- | ---------------------------------: | ---------------------------------- |
| `--maple-primary`             |                      `17 100% 72%` | coral accent (#FF9771)             |
| `--maple-primary-strong`      |                       `17 78% 58%` | darker coral gradient stop         |
| `--maple-on-primary`          | `0 0% 100%` light / `0 0% 4%` dark | text on coral                      |
| `--maple-primary-container`   |                      `17 100% 94%` | subtle coral tint (#FFE8E0)        |
| `--maple-primary-rgb`         |                    `255, 151, 113` | for rgba() usage                   |
| `--maple-secondary`           |                       `237 8% 57%` | pebble muted accent (#8A8B9A)      |
| `--maple-secondary-700`       |                       `237 9% 38%` | darker pebble icon/text (#5A5B6A)  |
| `--maple-secondary-container` |                      `240 14% 92%` | secondary surface fill (#E8E8ED)   |
| `--maple-tertiary`            |                       `11 22% 51%` | earthy "bark/grove" tone (#9E7469) |
| `--maple-tertiary-container`  |                       `17 25% 88%` | soft tertiary fill (#EADED9)       |
| `--maple-success`             |                       `80 32% 42%` | success (#7B8F4A)                  |
| `--maple-warning`             |                       `36 57% 59%` | warning (#D4A35A)                  |
| `--maple-on-warning`          |                        `0 0% 100%` | text on warning                    |
| `--maple-error`               |                       `12 60% 54%` | destructive/error (#D05E41)        |
| `--maple-info`                |                      `213 15% 56%` | info (#7E8DA1)                     |
| `--maple-surface`             |                         `0 0% 98%` | branded surface                    |
| `--maple-surface-dim`         |                       `240 7% 78%` | dim surface                        |

### Product-specific chrome aliases (new)

```css
/* Light mode */
--sidebar-chrome: 0 0% 100%;
--sidebar-chrome-hover: 0 0% 96%;
--on-sidebar-chrome: 0 0% 15%;

/* Dark mode (.dark block) */
--sidebar: var(--neutral-800);
--sidebar-chrome: var(--neutral-700);
--sidebar-chrome-hover: var(--neutral-600);
--on-sidebar-chrome: 0 0% 98%;
```

### Dark mode variable overrides

In the `.dark` block, the key overrides (beyond the sidebar tokens above):

- `--background: var(--neutral-950)` (was `0 0% 7%`)
- `--foreground: 0 0% 98%` (was `0 0% 89%`)
- `--card: 0 0% 9%` (was `0 0% 10%`)
- `--primary: 0 0% 98%` (inverted from light)
- `--primary-foreground: 0 0% 4%`
- `--muted: 0 0% 15%` (was `0 0% 15%` -- unchanged)
- `--muted-foreground: 0 0% 64%` (was `0 0% 70%`)
- `--border: 0 0% 15%` (was `0 0% 20%`)
- `--maple-on-primary: 0 0% 4%` (dark text on coral in dark mode)
- `--maple-primary-container: 17 40% 18%` (darker container)

Refer to PR #465 `index.css` diff for the complete dark `.dark` block -- the pattern is the same as light but with adjusted values for each maple-\* token.

### Brand gradient

```css
--maple-brand-gradient-from: 240 7% 78%;
--maple-brand-gradient-to: 11 22% 51%;
```

Utility classes to add in `@layer utilities`:

- `.brand-gradient` -- `bg-gradient-to-r` with the from/to stops
- `.brand-gradient-text` -- same gradient with `bg-clip-text text-transparent`

### Global CSS additions in `@layer utilities`

```css
/* Maple primary colored caret for all text inputs */
textarea,
input[type="text"],
input[type="email"],
input[type="password"],
input[type="search"] {
  caret-color: hsl(var(--maple-primary));
}
```

Also update `.primary-gradient` to use `--maple-primary` instead of `--purple`.

## 7.4 Tailwind additions

Add semantic Tailwind mappings for:

- `maple.primary.*`
- `maple.secondary.*`
- `maple.tertiary.*`
- `maple.success`
- `maple.warning`
- `maple.onWarning`
- `maple.error`
- `maple.info`
- `maple.surface.*`
- `neutral.50` through `neutral.950`
- `destructive.onFilled`

## 7.5 Semantic color migration rules

Use the following migration logic across product surfaces.

| Replace                           | With                                                                                    |
| --------------------------------- | --------------------------------------------------------------------------------------- |
| `text-green-*`                    | `text-maple-success`                                                                    |
| `text-red-*`                      | `text-maple-error`                                                                      |
| `text-amber-*`, `text-yellow-*`   | `text-maple-warning`                                                                    |
| `text-blue-*`                     | `text-maple-info` or `text-[hsl(var(--maple-primary))]` when brand emphasis is intended |
| `text-purple-*`                   | `text-[hsl(var(--maple-primary))]`                                                      |
| `bg-green-*/10`                   | `bg-maple-success/10`                                                                   |
| `bg-red-*/10`                     | `bg-maple-error/10`                                                                     |
| `bg-amber-*/10`, `bg-yellow-*/10` | `bg-maple-warning/10`                                                                   |
| `bg-blue-*/10`                    | `bg-maple-info/10`                                                                      |
| `bg-purple-*/10`                  | `bg-[hsl(var(--maple-primary))]/10`                                                     |

## 7.6 Shared product assets

### Required

- `MapleWordmark.tsx` as an inline `currentColor` SVG component
- `public/m-avatar.svg` for the assistant avatar

### Optional / only if actually used by product surfaces

- additional wordmark SVG files
- raster branding assets added by PR #465

Do not carry unused branding files into the product-only PR just because they exist in the designer branch.

## 7.7 Primitive strategy

### Good ideas from the PR

- add a `primary` button variant
- softer radii
- better dropdown/dialog corners
- better semantic destructive text handling
- darker unified scrim for overlays

### Important cleanup rule

The PR uses `--marketing-hero-scrim` for product overlays. That value is the dark translucent backdrop behind dialogs, sheets, and alert dialogs. In the clean reimplementation, prefer a neutral/product-owned token name such as `--overlay-scrim` if the value is shared by dialogs/sheets/alert dialogs. Do not keep marketing-specific naming in core product primitives unless absolutely necessary.

### Blast-radius caution

Global primitive changes affect out-of-scope pages too.

Preferred approach:

- land global primitive changes only when they are broadly safe
- otherwise add product-specific variants/helpers instead of restyling every button/menu in the whole app by accident

### Primitive details worth preserving

#### Button (`button.tsx`)

**Base class changes:**

- Old: `hover:backdrop-blur-xs ... rounded-md ... disabled:opacity-50`
- New: `active:scale-[0.95] rounded-full ... transition-all duration-200 ease-out ... disabled:opacity-40`

**Variant exact classes from the PR** (refer to PR diff for the full strings; key mappings below):

| Variant       | Old summary                                                                   | New summary                                                                                                                                                                                       |
| ------------- | ----------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `default`     | `bg-primary text-primary-foreground hover:bg-primary/90`                      | Subtle gradient: `bg-gradient-to-b from-[hsl(var(--maple-tertiary-container)/0.5)] to-[hsl(var(--maple-tertiary-container)/0.25)] text-[hsl(var(--maple-secondary-700))]` with dark mode variants |
| `primary`     | **(NEW)**                                                                     | `bg-gradient-to-b from-[hsl(var(--maple-primary))] to-[hsl(var(--maple-primary-strong))] text-[hsl(var(--maple-on-primary))]/90 hover:brightness-110`                                             |
| `destructive` | `bg-destructive text-white`                                                   | `bg-gradient-to-b from-[hsl(var(--maple-error))] to-[hsl(var(--maple-error)/0.8)] text-destructive-onFilled hover:brightness-110`                                                                 |
| `outline`     | Purple/blue tinted border + hover glow                                        | `border-[hsl(var(--maple-secondary))]/30 hover:border-[hsl(var(--maple-primary))]/80 bg-transparent`                                                                                              |
| `secondary`   | `bg-secondary text-secondary-foreground`                                      | `bg-gradient-to-b from-[hsl(var(--maple-secondary-container))] to-[hsl(var(--maple-secondary-container)/0.6)] text-[hsl(var(--maple-secondary-700))]`                                             |
| `ghost`       | `hover:bg-accent dark:hover:bg-[hsl(var(--purple))]/20 dark:hover:text-white` | `text-foreground hover:bg-[hsl(var(--maple-secondary-container))]` (same both modes)                                                                                                              |
| `link`        | `text-primary underline-offset-4`                                             | `text-[hsl(var(--maple-primary))] rounded-none active:scale-100`                                                                                                                                  |

**Size changes:**

- `default`: `h-10 px-4 py-2` -> `h-10 px-5 py-2 text-sm`
- `sm`: `h-9 rounded-md px-3` -> `h-9 px-4 py-2 text-xs` (inherits rounded-full)
- `lg`: `h-11 rounded-md px-8` -> `h-11 px-7 py-2 text-base`

Recommended approach: definitely add `variant="primary"`. Audit `default` and `outline` changes before making them global since they affect marketing pages too.

#### Dialog (`dialog.tsx`)

- **Overlay:** `bg-black/80` -> `bg-[hsl(var(--overlay-scrim)/0.8)]` (this is the modal backdrop/scrim behind the dialog; use a neutral token name, not `--marketing-hero-scrim`)
- **Content:** Remove `border`. Add `dark:bg-muted`. Corner radius: `sm:rounded-lg` -> `sm:rounded-2xl`

#### Sheet (`sheet.tsx`)

- **Overlay:** Same scrim change as Dialog

#### Dropdown menu (`dropdown-menu.tsx`)

- **Content & SubContent:** `rounded-md` -> `rounded-xl`. Remove `border` from main content.
- **All item focus states:** `dark:focus:bg-[hsl(var(--purple))]/20 dark:focus:text-white` -> `dark:focus:bg-[hsl(var(--maple-primary))]/20 dark:focus:text-foreground`

#### Switch (`switch.tsx`)

- **Thumb:** `bg-black` -> `bg-foreground`

---

## 8. Surface-by-Surface Specification

## 8.1 Authenticated shell

### Keep

- current route branching in `routes/index.tsx`
- current modal/dialog wiring for verification, promo, guest payment, team management, API management

### Add

- `ThemeProvider` around the app in `app.tsx`
- closed-sidebar top-left control row: sidebar toggle + wordmark

### Do not do

- do not redesign the logged-out shell in the same PR
- do not move product logic into new routes unless needed

## 8.2 Sidebar

### Visual direction to preserve

- flatter sidebar container
- inline wordmark at top
- chromeless close control
- text-link style actions for `New Chat` and `Search`
- rounded search input
- separate subtle `History` label

### Target structure

1. Top wordmark row
   - left: `MapleWordmark`
   - right: close button using `ArrowLeftFromLine`
2. Action row(s)
   - `New Chat`
   - `Search`
3. Optional search input
4. `History` label row
5. history nav
6. account menu pinned to bottom

### Canonical chrome classes

```tsx
<div className="flex h-full w-[280px] flex-col items-stretch border-r border-border/20 bg-muted backdrop-blur-lg dark:bg-[hsl(var(--sidebar))]">
```

### Search input shape

```tsx
className = "pl-4 pr-8 h-9 rounded-full";
```

### Sidebar toggle

Use a chromeless button with `Menu` (hamburger) icon, not an outline `<Button>`.

### Icon changes

| Location              | Old                                               | New                        |
| --------------------- | ------------------------------------------------- | -------------------------- |
| Close sidebar         | `PanelRightOpen`                                  | `ArrowLeftFromLine`        |
| Open sidebar (toggle) | `PanelRightClose` in `<Button variant="outline">` | `Menu` in plain `<button>` |
| Send message          | `Send`                                            | `ArrowUp`                  |
| Fullscreen toggle     | `Maximize2` / `Minimize2`                         | `Expand` / `Shrink`        |

### Important product rule

The designer PR removes the visible bulk-move button from selection mode. The new product implementation must **not** lose bulk move capability. Keep bulk move either:

- as a visible second action, or
- as a compact overflow action,

but do not regress the feature.

## 8.3 Chat history list

### What to keep

- Projects
- Pinned chats
- Recents
- Archived chats
- selection mode
- context menus
- pagination
- pull-to-refresh
- search filtering

### What to take from the PR

- rounded row shells
- fade overlay at the right edge of long titles
- rounded floating ellipsis trigger
- hidden date text by default
- same treatment for archived rows

### Reusable constants

```tsx
const SIDEBAR_TITLE_FADE =
  "pointer-events-none absolute inset-y-0 right-0 z-[1] bg-gradient-to-l from-muted from-35% via-muted/85 to-transparent dark:from-[hsl(var(--sidebar))] dark:from-35% dark:via-[hsl(var(--sidebar)/0.85)] dark:to-transparent";

const SIDEBAR_ELLIPSIS_BTN =
  "z-20 shrink-0 rounded-full bg-muted/90 p-1.5 text-primary backdrop-blur-sm transition-opacity dark:bg-[hsl(var(--sidebar)/0.9)]";
```

### Row shell direction

```tsx
className =
  "group relative flex select-none items-center gap-0.5 rounded-2xl pr-1";
```

### Important implementation rule

Apply the new row chrome consistently to:

- recent chats
- pinned chats
- project conversation rows
- archived chat rows

Do not redesign only one subsection and leave the others visually stale.

## 8.4 UnifiedChat

## 8.4.1 Empty state

### Replace

- large logo swap
- `Private AI Chat`
- `How can I help you today?`

### With

- simple vertical spacer
- stronger empty-state heading
- brand-gradient headline text

### Canonical headline

```tsx
<h1 className="overflow-visible pb-1 text-4xl font-normal leading-relaxed brand-gradient-text mb-6">
  Research anything...
</h1>
```

## 8.4.2 Closed-sidebar top-left chrome

When the sidebar is closed and the product is not in the mobile chat-header state:

```tsx
<div className="fixed top-[9.5px] left-4 z-20 flex items-center gap-1.5">
  <SidebarToggle onToggle={toggleSidebar} />
  <MapleWordmark className="h-4 w-auto" aria-hidden />
</div>
```

## 8.4.3 Mobile conversation header

When on mobile, when messages exist, and when the sidebar is closed:

- use a compact two-row header
- row 1: sidebar toggle + wordmark on left, new-chat icon button on right
- row 2: centered conversation title

This is a meaningful polish item from the PR and worth preserving.

## 8.4.4 Message layout

### User message direction

Move away from a full-width labeled row.

Use a quieter, right-aligned bubble:

```tsx
<div className="max-w-[min(100%,42rem)] rounded-2xl border border-border bg-muted px-4 py-3 backdrop-blur-lg dark:bg-card">
```

### Assistant message direction

Use:

- branded `m-avatar.svg`
- name label as a small line item
- cleaner vertical rhythm
- no faux bot-icon chip

### Assistant avatar helper

```tsx
function MapleChatAvatar() {
  return (
    <img
      src="/m-avatar.svg"
      alt=""
      width={32}
      height={32}
      draggable={false}
      className="h-8 w-8 shrink-0 select-none"
    />
  );
}
```

### Messages area padding

- `p-6` -> `p-4 md:p-6` (tighter on mobile)

### Tool and search result shells

Adopt the larger rounded cards from the PR.

Examples:

- web search status: `rounded-2xl`
- tool result cards: `rounded-3xl`
- incomplete/canceled indicator: `rounded-2xl`

## 8.4.5 Composer shell

This is one of the most important visual upgrades.

### Canonical shell

```tsx
className =
  "relative overflow-hidden rounded-3xl border border-[hsl(var(--maple-secondary-container))] bg-background transition-colors focus-within:border-[hsl(var(--maple-primary))]";
```

### Fullscreen variant

Keep fullscreen behavior, but use the same shell language.

### Layout changes to preserve

- textarea and fullscreen button on the same top row
- thinner border than current `border-2`
- much rounder shell
- no top-right absolute icon floating over the textarea
- toolbar without a hard separator line

### Empty-state textarea direction

- `rows={1}`
- `min-h-[52px]`
- `max-h-[200px]`
- comfortable `leading-6`

## 8.4.6 Toolbar controls

Adopt the calmer pebble/coral icon treatment.

### Canonical icon-button classes

```tsx
className =
  "h-8 w-8 p-0 text-[hsl(var(--maple-secondary-700))] hover:text-[hsl(var(--maple-secondary-700))] hover:bg-[hsl(var(--maple-primary-container))]";
```

### Keep these controls functional

- model selector
- web search toggle
- attachment menu
- mic
- stop generation
- send
- project picker if currently shown in this flow

Important: this is a visual redesign, not a capability reduction.

## 8.4.7 Send button

Keep the PR's custom circular gradient send button.

### Canonical send button

```tsx
<button
  type="submit"
  disabled={!input.trim() && !draftImages.length && !documentText}
  className="flex h-9 w-9 items-center justify-center rounded-full bg-gradient-to-b from-[hsl(var(--maple-primary))] to-[hsl(var(--maple-primary-strong))] text-[hsl(var(--maple-on-primary))]/90 transition-all duration-200 ease-out active:scale-[0.95] disabled:pointer-events-none"
>
  <ArrowUp className="h-4 w-4" />
</button>
```

Use the `h-8 w-8` version for the bottom compact composer.

## 8.4.8 Voice / stop / attachment shells

Preserve the PR's softer geometry:

- mic: `rounded-xl`
- stop button: `rounded-xl`
- inner stop square: `rounded-md`
- image thumbs: `rounded-xl`
- document chip: `rounded-2xl`
- recording overlay: `rounded-3xl`

## 8.4.9 Color migrations in UnifiedChat

Throughout the chat interface, apply the semantic color migration:

- Error text: `text-red-500` -> `text-maple-error`
- Success icons in tool results: `text-green-600 dark:text-green-400` -> `text-maple-success`
- Warning dot (incomplete/canceled): `bg-yellow-500` -> `bg-maple-warning`
- Web search enabled icon: `text-blue-500` -> `text-[hsl(var(--maple-primary))]`
- Web search disabled icon: `text-muted-foreground` -> `text-[hsl(var(--maple-secondary-700))]`
- Toolbar icon buttons (globe, plus, mic): all use the pebble/coral treatment from Section 9.7

## 8.4.10 Footer copy

Use the stronger privacy microcopy from the PR.

```tsx
<p className="text-xs text-center text-muted-foreground/60 flex items-center justify-center gap-1">
  <LockKeyhole className="h-3 w-3" />
  Encrypted and private at every step
</p>
```

Bottom disclaimer: `text-sm` -> `text-[10px]`, `text-muted-foreground/60` -> `text-muted-foreground/50`, `mt-2` -> `mt-1 mb-2`.

## 8.4.11 Do not regress current UnifiedChat behavior

The following are visual-only or presentation-oriented changes. Do not use this redesign as an excuse to delete:

- pagination state
- model gating
- attachment validation
- project integration
- title refresh logic
- web search education flow
- TTS logic
- voice recording lifecycle

## 8.5 Archived chat route

Even though this is a compatibility surface, the PR includes useful polish here.

### Take from the PR

- user messages become the same right-aligned rounded bubble treatment
- assistant messages use the branded `m-avatar.svg`
- mobile new-chat button becomes borderless

### Keep

- read-only archived behavior
- current route semantics

## 8.6 Account menu

## 8.6.1 Visual direction

- centered plan badge
- compact ring-style credit meter below it
- small circular account trigger instead of a full-width `Account` button
- dropdown aligns to the sidebar edge, not to the tiny circular trigger center

## 8.6.2 Canonical trigger

```tsx
<button
  type="button"
  className="relative flex h-9 w-9 shrink-0 items-center justify-center rounded-full bg-[hsl(var(--sidebar-chrome))] text-[hsl(var(--on-sidebar-chrome))] shadow-none ring-0 transition-colors hover:bg-[hsl(var(--sidebar-chrome-hover))]"
>
  <User className="h-4 w-4" />
</button>
```

## 8.6.3 Canonical dropdown positioning

```tsx
<DropdownMenuContent
  className="w-[calc(280px-2rem)] max-w-[calc(100vw-2rem)] overflow-hidden dark:bg-[hsl(var(--sidebar-chrome))]"
  align="start"
  side="top"
  sideOffset={8}
>
```

## 8.6.4 Content and styling details

- Menu label: `Maple Research` instead of `Maple AI`
- Plan badge styling:
  - **Old:** `bg-[hsl(var(--primary))] text-[hsl(var(--primary-foreground))]` (black/white)
  - **New:** `bg-[hsl(var(--maple-tertiary-container))] text-[hsl(var(--maple-tertiary))] text-[10px]` (earthy tint)
- Team setup badge: `bg-amber-500 text-white` -> `bg-maple-warning text-maple-onWarning`
- Privacy/terms links: **ignore the PR's URL swap.** The designer branch came from an older fork. Current `master` privacy/terms pages, routes, and destinations are canonical and must remain exactly as `master` defines them.

## 8.6.5 Keep current account behavior

Do not regress:

- sign-out cache clearing behavior unless consciously reworked everywhere
- billing portal behavior
- external-link behavior in Tauri/web
- team/API modal flows

## 8.7 Account dialog

### Add

A `Theme` section with `Light`, `Dark`, and `System` controls.

### Visual direction

- `Sun`, `Moon`, `Monitor` icons
- use buttons that clearly show active selection

### Token cleanup

- verified email icon -> `text-maple-success`
- unverified email icon -> `text-maple-error`
- destructive action becomes text/ghost-style destructive action instead of a heavy outlined danger button

## 8.8 Credit usage

## 8.8.1 Keep both layouts

- existing bar layout remains useful outside the sidebar
- new ring layout is used in the redesigned account area

## 8.8.2 Ring layout to preserve

- Compact bordered card: `rounded-xl border border-[hsl(var(--sidebar-chrome))] bg-transparent p-3`
- Left side: status label ("Plan credits" / "Almost full" / "Limit reached") + reset date + optional extra credits text
- Right side: 32x32 SVG ring meter with 3.5px stroke
- Ring track: `stroke-[hsl(var(--sidebar-chrome))]`
- Ring fill: linear gradient from `hsl(var(--maple-primary))` to `hsl(var(--maple-primary-strong))`
- Animated: `transition-[stroke-dashoffset] duration-500 ease-out`
- Rotated -90deg so arc starts from top

Refer to PR #465 `CreditUsage.tsx` diff for the `RingMeter` SVG implementation -- it's ~40 lines of clean SVG component code worth carrying over directly.

## 8.8.3 Usage thresholds

| Threshold | Tone   | Color                       | Text class           |
| --------- | ------ | --------------------------- | -------------------- |
| `>= 90%`  | danger | `hsl(var(--maple-error))`   | `text-maple-error`   |
| `>= 75%`  | warn   | `hsl(var(--maple-warning))` | `text-maple-warning` |
| `< 75%`   | ok     | `hsl(var(--maple-success))` | `text-maple-success` |

Old bar variant used hardcoded Tailwind (red-500, amber-500, emerald-500) -- migrate to the same semantic tokens.

Status labels: `>= 100%` "Limit reached", `>= 90%` "Almost full", `< 90%` "Plan credits".

## 8.8.4 Dev-only mock support

The PR's dev-only mock scenarios are useful and can stay behind `import.meta.env.DEV`.

Supported scenarios from the PR:

- `demo`
- `full`
- `high`
- `warn`
- `ok`
- `off`

## 8.9 Model selector

### Take from the PR

- Trigger text size: `text-sm` -> `text-xs`
- Trigger button color: `text-[hsl(var(--maple-secondary-700))] hover:bg-[hsl(var(--maple-primary-container))]`
- Chevron: remove `opacity-50`
- Badge border radius: `rounded-sm` -> `rounded-md`
- Upgrade hover state: `hover:bg-purple-50 dark:hover:bg-purple-950/20` -> `hover:bg-[hsl(var(--maple-primary-container))] dark:hover:bg-[hsl(var(--maple-primary))]/10`

### Preserve

- all current gating logic
- image restrictions
- model availability logic
- current category/model selection behavior

### Badge mapping from the PR

| Badge         | Treatment                                    |
| ------------- | -------------------------------------------- |
| `Coming Soon` | `bg-muted text-muted-foreground`             |
| `Pro`         | coral-to-tertiary soft gradient + coral text |
| `Starter`     | `bg-maple-success/10 text-maple-success`     |
| `New`         | `bg-maple-info/10 text-maple-info`           |
| `Reasoning`   | `bg-maple-error/10 text-maple-error`         |
| `Beta`        | `bg-maple-warning/10 text-maple-warning`     |

## 8.10 Markdown rendering

## 8.10.1 Link behavior (chat.css)

Replace the current accent-colored, outline-ring link style with calmer foreground-based treatment:

```css
.markdown-body a {
  color: hsl(var(--foreground) / 0.72); /* subdued, not accent-colored */
  text-decoration: none;
  -webkit-tap-highlight-color: transparent;
}
.markdown-body a:hover {
  text-decoration: underline;
  text-underline-offset: 0.15em;
  color: hsl(var(--foreground)); /* full opacity on hover */
}
.markdown-body a:focus {
  outline: none;
  box-shadow: none;
}
.markdown-body a:focus-visible {
  outline: none;
  box-shadow: none;
  text-decoration: underline;
  text-underline-offset: 0.15em;
  color: hsl(var(--foreground));
}
```

Remove the old `.markdown-body a:focus`, `.markdown-body a:focus:not(:focus-visible)`, and `.markdown-body a:focus-visible` rules that used outline rings.

## 8.10.2 Tables

### Component change (markdown.tsx)

Replace the `ResponsiveTable` scroll-detection approach (removes `useState`, `useEffect`, `useRef` for scroll tracking, gradient fade indicators) with a simpler wrapper:

```tsx
<div className="my-4 w-full min-w-0 max-w-none self-stretch">
  <div className="block w-full min-w-0 overflow-x-auto overflow-y-visible overscroll-x-contain [-webkit-overflow-scrolling:touch]">
    <table className="markdown-table-maple w-full min-w-0">{children}</table>
  </div>
</div>
```

### CSS additions (chat.css)

Add a new `.markdown-table-maple` class with these traits:

- `display: table; table-layout: fixed; width: 100%`
- Transparent backgrounds (no alternating row stripes)
- Horizontal rules only: `border-bottom: 1px solid hsl(var(--maple-secondary) / 0.2)` for headers, `hsl(var(--maple-secondary-container))` for cells
- Last row: no bottom border
- First column flush left (`padding-left: 0`)
- Responsive first-column widths: 34% on mobile, 22% on desktop
- `overflow-wrap: break-word; word-break: break-word` on all cells

Remove `display: block; width: max-content; max-width: 100%; overflow: auto` from base `.markdown-body table`.

Refer to the PR diff for the exact CSS rules -- there are ~90 lines of table styling.

## 8.10.3 Typography sizes (chat.css)

Set explicit `font-size: 14px; line-height: 1.5` on:

- `.markdown-body p`
- `.markdown-body ol`, `.markdown-body ul`
- `.markdown-body li`, `.markdown-body li > p`
- `.markdown-body td`

Add mobile reading optimization:

```css
@media (max-width: 767px) {
  .markdown-body p {
    max-width: min(100%, 48ch);
  }
  .markdown-body td p,
  .markdown-body th p {
    max-width: none;
  }
}
```

## 8.10.4 Thinking blocks (markdown.tsx)

Replace the bordered card treatment:

- **Old:** `border border-gray-200 dark:border-gray-700 rounded-lg bg-gray-50 dark:bg-gray-900/50`
- **New:** No border, no background. Plain button + expandable content.
- Icon/text colors: `text-gray-500 dark:text-gray-400` -> `text-muted-foreground`
- Collapse chevron moves from left side to right side of the row
- Expanded content: no border-top, just `pb-1 pt-2`

## 8.10.5 Code and content radius consistency

All code-related `border-radius` values change from `6px` to `12px`:

- `.markdown-body kbd`
- `.markdown-body pre`
- `.markdown-body .mermaid`
- Footnote checkbox `::before`

## 8.10.6 Dark mode in chat.css

Remove the `@media (prefers-color-scheme: dark) { :root { ... } }` block from `chat.css` (the markdown-specific dark variable overrides). Dark mode is now handled by the `.dark` class in `index.css`.

## 8.11 Product dialogs and secondary surfaces

### Apply token/radius polish to product-reachable dialogs

Use PR #465 mostly as a semantic cleanup guide here.

#### High-priority dialogs

- `UpgradePromptDialog`
- `PromoDialog`
- `WebSearchInfoDialog`
- `DocumentPlatformDialog`
- `ContextLimitDialog`
- `DeleteChatDialog`
- `BulkDeleteDialog`
- `TTSDownloadDialog`
- `VerificationModal`
- `GuestPaymentWarningDialog`

#### Expected treatment

- semantic icon/text color migration (see Section 7.5 for the full mapping table)
- rounded container polish
- use `variant="primary"` for the strongest upgrade CTA when appropriate
- preserve existing logic and copy unless the PR provides a clear product-facing improvement

#### Specific dialog changes from PR #465 worth preserving

- **UpgradePromptDialog:** benefit check icons `text-green-500` -> `text-maple-success`; upgrade button uses `variant="primary"`
- **PromoDialog:** pink/orange gradients -> `from-[hsl(var(--maple-primary))] to-[hsl(var(--maple-primary-strong))]`; badge, benefit icons, privacy check all migrate to maple semantic colors
- **WebSearchInfoDialog:** info icon `bg-blue-500/10 text-blue-500` -> `bg-maple-info/10 text-maple-info`; feature checks -> `text-maple-info`
- **GuestPaymentWarningDialog:** warning colors -> `text-maple-warning`, `bg-maple-warning/10`
- **AccountDialog:** verified email `text-green-700` -> `text-maple-success`; unverified `text-red-700` -> `text-maple-error`; delete account button `variant="outline" border-destructive` -> `variant="ghost" text-destructive hover:bg-destructive/10`

## 8.12 Team/API/account dashboards

These are in scope only for polish, not for wholesale redesign.

### What to carry over

- semantic token cleanup
- softer badges/fills
- progress bars using Maple semantic colors
- consistent dialog/dropdown/button styling inherited from shared primitives

### What not to do

- do not restructure these dashboards just because product chrome changed elsewhere

## 8.13 Project surfaces

The designer PR does not directly redesign project mode, but the authenticated product still supports it.

### Required rule

The new redesign PR must leave project functionality intact.

### Minimum expectation

- shared theme/tokens should not make project mode look broken
- shared buttons/dialogs/sidebar chrome should feel consistent
- project flows remain operational

### Not required in the first redesign PR

- a bespoke new visual language for `ProjectDetailView`
- redesigning project instructions UX
- redesigning move/create/delete project dialogs beyond shared primitive polish

---

## 9. Exact High-Value Class Specs

These are the visual details most worth preserving verbatim or near-verbatim.

## 9.1 Sidebar history fade

```tsx
const SIDEBAR_TITLE_FADE =
  "pointer-events-none absolute inset-y-0 right-0 z-[1] bg-gradient-to-l from-muted from-35% via-muted/85 to-transparent dark:from-[hsl(var(--sidebar))] dark:from-35% dark:via-[hsl(var(--sidebar)/0.85)] dark:to-transparent";
```

## 9.2 Sidebar ellipsis button

```tsx
const SIDEBAR_ELLIPSIS_BTN =
  "z-20 shrink-0 rounded-full bg-muted/90 p-1.5 text-primary backdrop-blur-sm transition-opacity dark:bg-[hsl(var(--sidebar)/0.9)]";
```

## 9.3 Empty-state heading

```tsx
className =
  "overflow-visible pb-1 text-4xl font-normal leading-relaxed brand-gradient-text mb-6";
```

## 9.4 Composer shell

```tsx
className =
  "relative overflow-hidden rounded-3xl border border-[hsl(var(--maple-secondary-container))] bg-background transition-colors focus-within:border-[hsl(var(--maple-primary))]";
```

## 9.5 Composer top row

```tsx
className = "flex items-start gap-1 pl-4 pr-2 pt-2";
```

## 9.6 Fullscreen toggle button

```tsx
className =
  "mt-0.5 shrink-0 rounded-full p-1.5 text-muted-foreground/60 transition-colors hover:bg-muted/50 hover:text-foreground";
```

## 9.7 Toolbar icon button

```tsx
className =
  "h-8 w-8 p-0 text-[hsl(var(--maple-secondary-700))] hover:text-[hsl(var(--maple-secondary-700))] hover:bg-[hsl(var(--maple-primary-container))]";
```

## 9.8 User bubble

```tsx
className =
  "max-w-[min(100%,42rem)] rounded-2xl border border-border bg-muted px-4 py-3 backdrop-blur-lg dark:bg-card";
```

## 9.9 Assistant message shell

```tsx
<div className="mx-auto flex w-full max-w-4xl flex-col gap-2 md:flex-row md:items-start md:gap-3">
  <div className="flex h-8 shrink-0 items-center gap-2 px-0 md:h-auto md:flex-col md:items-start md:gap-3">
    <MapleChatAvatar />
  </div>
  <div className="flex min-w-0 w-full flex-1 flex-col overflow-hidden px-2 md:gap-2 md:px-0">
    <div className="hidden h-8 items-center md:flex">
      <div className="text-left text-sm font-semibold leading-none">Maple</div>
    </div>
  </div>
</div>
```

## 9.10 Account trigger

```tsx
className =
  "relative flex h-9 w-9 shrink-0 items-center justify-center rounded-full bg-[hsl(var(--sidebar-chrome))] text-[hsl(var(--on-sidebar-chrome))] shadow-none ring-0 transition-colors hover:bg-[hsl(var(--sidebar-chrome-hover))]";
```

## 9.11 Ring credit card

```tsx
<div className="w-full rounded-xl border border-[hsl(var(--sidebar-chrome))] bg-transparent p-3">
```

---

## 10. What Not To Copy Literally From PR #465

## 10.1 Do not copy any marketing/page work

Ignore those files entirely for the new product redesign PR.

## 10.2 Do not infer feature removals from visual simplifications

If the designer PR visually simplifies a control but the current product still supports that feature, preserve the feature.

Key examples:

- bulk move
- projects/project mode
- pinned chats
- project picker behavior
- current route/search-param contracts

## 10.3 Do not hand-edit generated files

- `routeTree.gen.ts`

## 10.4 Do not import unused branding assets

Only bring over assets actually needed by the product redesign.

## 10.5 Do not keep marketing-specific token names in shared product primitives if a neutral semantic alias is cleaner

Example: prefer `overlay-scrim` over `marketing-hero-scrim` for dialog overlays if touching shared primitives.

## 10.6 Do not land huge global primitive changes without auditing blast radius

Because marketing/helper pages are out of scope, avoid unintentionally redesigning them through careless primitive changes.

## 10.7 Do not overwrite newer master content with older-fork PR content

PR #465 started from an older fork, so some content/routing changes in that branch are stale by definition.

- keep newer `master` pages, routes, and copy
- only reapply the visual/product-chrome improvements from the designer branch
- do not let old-fork link destinations replace current `master` behavior
- example: privacy/terms on `master` stay exactly as they are

---

## 11. Implementation Phase Plan For The Future Product PR

## Phase 1: foundation

- add tokens to `index.css`
- add Tailwind semantic mappings
- add `ThemeProvider`
- add early-theme script
- self-host `Manrope` assets and wire `@font-face` / preload strategy
- add `MapleWordmark` + `m-avatar.svg`

## Phase 2: shared primitives

- button improvements
- dialog/dropdown/alert/sheet/switch polish
- only land safe global changes

## Phase 3: shell + sidebar

- sidebar shell/header
- sidebar toggle
- chat history row chrome
- account menu shell
- ring credit meter

## Phase 4: chat surface

- empty state
- mobile/desktop header behavior
- message shells
- composer shell
- send button
- tool/web search status cards
- footer/privacy microcopy

## Phase 5: markdown + secondary dialogs

- markdown link/table/thinking polish
- upgrade/search/promo/document dialogs
- account dialog theme picker
- product-reachable dashboard token cleanup

## Phase 6: compatibility pass

- archived route visual alignment
- project surfaces sanity pass
- ensure project mode does not clash with new shared foundation

## Phase 7: validation

For the future implementation PR, run at minimum:

```bash
just format
just lint
just build
```

If Rust is untouched, Rust validators are not required.

---

## 12. Acceptance Checklist For The Future Implementation PR

## 12.1 Theme and shell

- [ ] light mode looks correct
- [ ] dark mode looks correct
- [ ] `system` theme follows OS changes
- [ ] no initial theme flash on reload

## 12.2 Responsive behavior

- [ ] desktop closed-sidebar top-left wordmark row works
- [ ] mobile two-row chat header works
- [ ] sidebar open/close behavior still works correctly

## 12.3 Chat behavior

- [ ] empty state redesign is present
- [ ] active chat layout works
- [ ] streaming states look correct
- [ ] canceled/tool/web-search states look correct
- [ ] image/document attachments still work
- [ ] voice flow still works
- [ ] fullscreen composer still works

## 12.4 History behavior

- [ ] projects still work
- [ ] pinned chats still work
- [ ] archived chats still work
- [ ] bulk move still works
- [ ] bulk delete still works
- [ ] search still works
- [ ] pull-to-refresh still works on mobile

## 12.5 Account/billing behavior

- [ ] account menu redesign is present
- [ ] credit ring is present in sidebar
- [ ] theme picker works
- [ ] team/API dialogs still open and work
- [ ] billing/manage subscription still works

## 12.6 Compatibility surfaces

- [ ] archived route looks aligned with new system
- [ ] project mode still works and does not look broken

---

## 13. PR #465 File Map For The New Product PR

## 13.1 Carry over directly or near-directly

- `frontend/src/components/MapleWordmark.tsx`
- `frontend/public/m-avatar.svg`
- theme/token additions in `index.css`
- Tailwind semantic mappings in `tailwind.config.js`
- `ThemeProvider` concept in `src/contexts/ThemeContext.tsx` (with fixes)
- core product chrome direction in:
  - `Sidebar.tsx`
  - `ChatHistoryList.tsx`
  - `UnifiedChat.tsx`
  - `AccountMenu.tsx`
  - `CreditUsage.tsx`
  - `ModelSelector.tsx`
  - `markdown.tsx`
  - `AccountDialog.tsx`

## 13.2 Carry over as token/radius cleanup only

- product-reachable dialogs
- API dashboard surfaces
- team dashboard surfaces
- verification/promo/search/upgrade supporting surfaces

## 13.3 Ignore for the new product-only PR

- all marketing/public routes and components
- pricing config changes
- `routeTree.gen.ts`
- `BillingDebugger.tsx`
- extra marketing-only asset work

---

## 14. Bottom Line

The right implementation is **not** “merge the designer branch.”

The right implementation is:

- take the designer's product visual language,
- keep Maple's real product behavior,
- express the redesign through shared tokens, clean primitives, and reusable product chrome,
- and keep the rewrite tightly scoped to the authenticated product experience.
