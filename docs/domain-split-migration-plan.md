# Maple Marketing/App Domain Separation and App Host Migration Plan

> Status: proposed strategy and execution plan
>
> Last updated: 2026-04-14
>
> Purpose: define a deliberate three-phase plan for separating Maple's marketing and app surfaces without coupling this decision to the current redesign work, while still creating a clear long-term path to introduce a product-agnostic auth broker and later move the product from `https://trymaple.ai` to `https://app.trymaple.ai`.

This document captures the current architectural constraints in the Maple frontend, the major migration risks, the options considered, the path that was chosen, and a concrete rollout plan.

This is intentionally a planning document, not an implementation PR checklist for a single day. The goal is to avoid making a brittle short-term decision during the redesign while still preserving a clean long-term end state.

---

## 1. Executive Summary

Maple currently has one public host, `https://trymaple.ai`, serving two different concerns:

- the logged-out marketing experience
- the logged-in product experience

That coupling has leaked into:

- OAuth callback URLs
- billing and checkout return URLs
- desktop auth flows
- iOS/Android universal link and app link configuration
- hardcoded external links throughout the frontend
- SEO artifacts like `sitemap.xml`, `robots.txt`, `llms.txt`, and `llms-full.txt`

Because Maple already has production users across web, desktop, iOS, and Android, this cannot be treated like a simple hostname rename.

### Chosen direction

Maple should follow a three-phase strategy:

1. **Phase 1: marketing launch**

   - `https://trymaple.ai` remains the existing app host
   - `https://www.trymaple.ai` becomes the new marketing site
   - the app on apex removes marketing-heavy public pages from its codebase or flow, but critical app/auth/billing behavior stays on apex

2. **Phase 2: auth broker launch**

   - introduce a stable auth broker host such as `https://auth.trymaple.ai`
   - the broker is product-agnostic in design, but first ships for Maple only
   - the broker owns OAuth start/callback and native/desktop auth handoff concerns
   - broker plumbing should support future Maple products on other domains or native-only apps
   - billing callbacks and pricing return flows remain app-specific and are **not** part of broker v1

3. **Phase 3: app URL launch day**
   - `https://app.trymaple.ai` becomes the canonical app host
   - `https://trymaple.ai` becomes the canonical marketing host
   - `https://www.trymaple.ai` redirects to apex
   - every known integration, callback URL, external link, deep-link config, and product CTA is updated to the new app host or broker host as appropriate
   - older clients are expected to update or be blocked by minimum-version enforcement
   - Cloudflare compatibility rules may exist as a best-effort fallback, but the migration does **not** depend on them for success

### Core strategic principle

Do **not** try to solve the final app host migration inside the current redesign/rewrite effort.

Do the redesign and marketing split first.

Then introduce a real auth broker as separate shared infrastructure.

Only after that, on a later scheduled migration day, perform a deliberate public cutover to `app.trymaple.ai` with:

- pre-shipped client support
- broker-backed auth/callback infrastructure already in production
- provider configuration prepared in advance
- feature-flagged host configuration already in production
- a clear communications plan
- temporary fallback options, but no architectural dependence on permanent Cloudflare path rewriting

---

## 2. Goals

### 2.1 Primary goals

- Separate marketing and product into distinct codebases.
- Avoid breaking current users during the near-term redesign.
- Preserve SEO while giving marketing a clean home and clean routing.
- Avoid locking Maple into a permanent same-host path-routing architecture.
- Introduce a stable, product-agnostic auth surface that can outlive any single app host.
- Create a clear and explicit long-term end state:
  - `trymaple.ai` = marketing
  - `auth.trymaple.ai` = shared auth broker
  - `app.trymaple.ai` = app
- Reduce host-specific hardcoding in the codebase.
- Make future host migrations configurable rather than scattered through literals.

### 2.2 Secondary goals

- Minimize engineering risk around OAuth, billing, and native deep links.
- Give marketing the freedom to iterate without app-route coupling.
- Make future Cloudflare and networking rules easier to reason about.
- Allow the app to evolve independently of public marketing pages.
- Make future cross-product login possible for other Maple products on other domains or native-only apps.

---

## 3. Non-Goals

This plan does **not** attempt to do the following in the near-term redesign phase:

- move the app to `app.trymaple.ai` during the redesign itself
- make Cloudflare path-rewrites the permanent architecture
- keep the app hosted equally on apex and `app` forever
- fold billing callbacks into the first version of the auth broker
- fully design future multi-product SSO before Maple's immediate domain split is stabilized
- guarantee that no production issue is possible on migration day
- solve every legacy-client problem with edge routing alone
- preserve the current apex-marketing and apex-app mixture indefinitely

This plan also does **not** assume that a hidden redirect-based migration is acceptable. The long-term shift to `app.trymaple.ai` is treated as a deliberate platform migration with a published date, and the auth broker is treated as real shared infrastructure rather than a thin redirect page.

---

## 4. Current Reality in the Repo

This section exists so that the plan stays grounded in the actual Maple codebase rather than wishful routing diagrams.

## 4.1 The root route is currently dual-purpose

In `frontend/src/routes/index.tsx`, the root route `"/"` currently behaves differently depending on auth state:

- logged-out users see marketing
- logged-in users see the app shell (`UnifiedChat`)
- some query params open team setup and API-credit modals
- some search-param behavior is used for app-specific return flows

This is the main reason a same-host permanent split is messy. The current root path is not just a public landing page; it is also the app home.

## 4.2 Critical app/auth/billing routes are already public contracts

These routes are externally depended on today and are therefore risky to change:

- `/desktop-auth`
- `/auth/$provider/callback`
- `/pricing`
- `/payment-success`
- `/payment-canceled`
- `/verify/$code`
- `/password-reset`
- `/password-reset/confirm`
- `/team/invite/$inviteId`
- `/redeem`
- `/.well-known/apple-app-site-association`
- `/.well-known/assetlinks.json`

Several of these are not just normal pages; they are protocol surfaces for auth, checkout, return flows, or mobile deep links.

## 4.3 Host assumptions are hardcoded today

Current repo examples:

- `frontend/src/routes/login.tsx`
- `frontend/src/routes/signup.tsx`

  - Tauri auth launches `https://trymaple.ai/desktop-auth?provider=...`

- `frontend/src/routes/pricing.tsx`

  - checkout success/cancel URLs are hardcoded to `https://trymaple.ai/...` for several flows

- `frontend/src/components/apikeys/ApiCreditsSection.tsx`

  - API credits success/cancel paths are hardcoded against `https://trymaple.ai`

- `frontend/src/billing/billingApi.ts`

  - Tauri billing portal return URL is hardcoded to `https://trymaple.ai`

- `frontend/src/components/AccountMenu.tsx`

  - privacy and terms links are hardcoded to `https://trymaple.ai/privacy` and `https://trymaple.ai/terms`

- `frontend/public/sitemap.xml`
- `frontend/public/robots.txt`
- `frontend/public/llms.txt`
- `frontend/public/llms-full.txt`
  - SEO and public-discovery assets all assume apex is the canonical public site

## 4.4 Same-origin browser storage is part of current flow behavior

Current flows rely on browser storage scoped to the current host:

- `redirect-to-native`
- `selected_plan`
- `post_auth_redirect`
- `apple_form_data`
- `redeem_code`

This matters because a move from `trymaple.ai` to `app.trymaple.ai` is not just a routing change; it is an origin change.

Cross-host moves do not preserve:

- `localStorage`
- `sessionStorage`
- current authenticated web session assumptions, if they are origin-bound

Any flow that currently depends on same-origin storage must either:

- stay on the same host
- or be updated to pass state explicitly
- or be redesigned to survive cross-host handoff

## 4.5 Native deep links and app links are tied to apex today

Current repo evidence:

- `frontend/src-tauri/tauri.conf.json`
- `frontend/src-tauri/gen/android/app/src/main/AndroidManifest.xml`
- `frontend/src-tauri/gen/apple/maple_iOS/maple_iOS.entitlements`
- `frontend/public/.well-known/apple-app-site-association`
- `frontend/public/.well-known/assetlinks.json`

These currently bind mobile app-link/universal-link behavior to `trymaple.ai` for:

- `/pricing`
- `/payment-success`
- `/payment-canceled`

Desktop/native auth also depends on:

- `/desktop-auth`
- `cloud.opensecret.maple://auth?...`

## 4.6 Apple deserves extra caution

`frontend/src/components/AppleAuthProvider.tsx` builds:

- `redirectURI: window.location.origin + "/auth/apple/callback"`

That means the host being used during Apple web auth matters directly.

Apple can also use `form_post` behaviors that are less forgiving than simple GET redirects. This is one of the main reasons the app-host migration should be staged carefully and not improvised during the redesign.

## 4.7 There is at least one existing flow inconsistency worth fixing before later migration phases

`frontend/src/components/apikeys/ApiCreditsSection.tsx` uses:

- `/payment-success-credits`

But current route/deep-link handling is centered around:

- `/payment-success`
- `/payment-canceled`
- `?credits_success=true`

This is the type of edge-case contract that should be normalized before the auth-broker rollout or final app-host migration.

---

## 5. Why a Permanent Same-Host Split Was Rejected

One option considered was to keep both marketing and app on the same apex host indefinitely and rely on Cloudflare path-based ownership.

That was rejected as the long-term architecture.

### 5.1 The root path can only have one owner

If both marketing and app live on the same host forever, then `"/"` must belong to one side.

But by definition:

- a landing page should own `"/"` on its host
- the product also wants an app home and client-side catch-all routes

Cloudflare cannot infer auth state from current client-side storage. It cannot safely decide:

- anonymous users -> marketing root
- authenticated users -> app root

before the browser app even boots.

### 5.2 App routes are not a small whitelist

The product already has and will continue to gain client-side routes and return flows, including dynamic paths and search-param-driven behavior.

Examples:

- root app shell behavior
- archived chat routes
- auth callback routes
- pricing and return flows
- invite and verification flows
- any future chat or conversation route families

A permanent hand-maintained whitelist is brittle and gets worse over time.

### 5.3 Marketing also wants route flexibility

If marketing wants:

- landing pages
- pricing pages
- blog routes
- experiments
- A/B tests
- campaign-specific routes

then same-host path ownership becomes a negotiation problem instead of a clean architecture.

### 5.4 Cloudflare rewriting is still useful, but not as the destination architecture

Cloudflare remains useful for:

- redirects
- compatibility fallbacks
- migration-day safety nets
- temporary route bridging

It is not the recommended permanent way to express product-vs-marketing ownership.

---

## 6. Alternatives Considered

## 6.1 Keep everything coupled forever

### Pros

- lowest short-term change risk
- no host migration
- no provider callback changes

### Cons

- permanent app/marketing coupling
- marketing and app compete for routes and deployment shape
- harder SEO ownership
- harder Cloudflare/network rule separation
- harder future experimentation
- more technical debt

### Decision

Rejected.

## 6.2 Put marketing on `home.trymaple.ai`

### Pros

- very low short-term risk
- no app host migration

### Cons

- awkward public/brand shape
- less standard than `www`
- weaker long-term SEO and user expectation story
- still delays the real cleanup

### Decision

Rejected in favor of `www` for the marketing split.

## 6.3 Move app to `app.trymaple.ai` immediately during redesign

### Pros

- clean long-term architecture immediately

### Cons

- highest migration risk
- too many moving parts at the same time
- forces provider, client, deep-link, and marketing changes into one project
- unacceptable uncertainty for auth/billing/deep-link flows

### Decision

Rejected for now.

## 6.4 Host the app publicly on both apex and `app` for a long migration period

### Pros

- smoother migration path
- less rush on migration day

### Cons

- operational and conceptual complexity
- two live product hosts for too long
- duplicate public surfaces
- unclear canonical host
- greater long-term maintenance cost

### Decision

Rejected as a primary strategy.

`app.trymaple.ai` may exist before public cutover for staging/dark launch/testing, but it should not become a permanent equal public product surface.

## 6.5 Chosen strategy: split now, introduce auth infrastructure next, cut over later

### Phase 1

- `trymaple.ai` remains app
- `www.trymaple.ai` becomes marketing

### Phase 2

- `auth.trymaple.ai` becomes the stable auth broker host
- Maple starts moving OAuth and native auth plumbing onto the broker
- the broker is designed so future Maple products can reuse it

### Phase 3

- `app.trymaple.ai` becomes app
- `trymaple.ai` becomes marketing
- `www.trymaple.ai` redirects to apex

### Why this wins

- low risk during redesign
- removes callback-host churn from the final app-host cutover
- creates reusable auth infrastructure for future products
- clean end state
- clear migration day
- no permanent same-host path entanglement
- enough time to ship preparatory changes first

---

## 7. Target End State

After Phase 3, the intended public domain model should be:

- `https://trymaple.ai` = canonical marketing site
- `https://www.trymaple.ai` = redirect to apex
- `https://auth.trymaple.ai` = canonical auth broker
- `https://app.trymaple.ai` = canonical app

## 7.1 Marketing ownership after Phase 3

Marketing should own the apex host and all marketing/SEO/public content there.

This includes:

- `/`
- `/about`
- `/proof`
- `/downloads`
- `/teams` if it is a marketing page
- `/privacy`
- `/terms`
- `/blog/*`
- `robots.txt`
- `sitemap.xml`
- `llms.txt`
- `llms-full.txt`
- other marketing and editorial pages

## 7.2 App ownership after Phase 3

The app should own `https://app.trymaple.ai` and all product behavior there.

This includes:

- `/`
- `/login`
- `/signup`
- `/pricing`
- `/payment-success`
- `/payment-canceled`
- `/verify/$code`
- `/password-reset`
- `/password-reset/confirm`
- `/team/invite/$inviteId`
- `/redeem`
- app shell and chat routes
- billing portal returns
- app management/account routes

## 7.3 Auth broker ownership after Phase 3

The auth broker should own the long-lived auth contract at `https://auth.trymaple.ai`.

This includes:

- OAuth initiation surfaces
- OAuth callback surfaces
- desktop/native auth entry and handoff
- target-app selection and post-auth handoff logic
- Maple-first behavior with plumbing for future products on other domains or native-only apps

This does **not** initially include:

- Stripe or Zaprite success/cancel callbacks
- billing portal returns
- product pricing or checkout pages

The broker should be treated as shared platform infrastructure, not as a branded product shell and not as a dumb redirect page.

## 7.4 What happens to apex legacy routes after Phase 3

The desired business posture for Phase 3 is:

- all official product links point to `app.trymaple.ai`
- all provider configs point to `app.trymaple.ai` or `auth.trymaple.ai`, depending on flow ownership
- all updated clients use `app.trymaple.ai`
- older clients either update or stop working correctly

However, the recommended operational posture is:

- allow limited Cloudflare compatibility rules as a safety net if needed
- do not define migration success as depending on those rules

This is the right compromise between a clean cutover and pragmatic risk reduction.

## 7.5 Special note on pricing

`/pricing` is currently not just a marketing page. In Maple today it also handles:

- checkout initiation
- checkout success/cancel UI
- billing/account behavior
- mobile deep-link behavior

Therefore the safest long-term pattern is:

- `trymaple.ai/pricing` = marketing pricing overview
- `app.trymaple.ai/pricing` = transactional pricing and plan management

If a marketing pricing page exists on apex, it should not silently inherit all of the current product/billing logic from the app route.

---

## 8. Phase 1: Marketing Launch

Phase 1 is intentionally conservative.

## 8.1 Public shape of Phase 1

- `https://trymaple.ai` remains the app host
- `https://www.trymaple.ai` becomes the new marketing site

## 8.2 What changes on apex in Phase 1

Apex should remain the current app host, but it should stop pretending to be the full long-term marketing site.

Recommended changes:

- remove or simplify heavy marketing pages from the app codebase
- keep logged-out apex entry focused on:
  - sign up
  - log in
  - app entry
  - critical billing/auth actions
- keep all auth, billing, deep-link, and compatibility paths unchanged

## 8.3 What moves to `www` in Phase 1

Move the redesign-driven marketing experience to `www`.

Recommended `www` ownership:

- homepage
- product/benefit pages
- editorial/brand pages
- blog and campaign pages
- non-transactional pricing/feature comparison content
- SEO-focused artifacts if desired for the marketing site

## 8.4 Redirect strategy in Phase 1

Recommended:

- old apex informational marketing pages redirect to `www` where safe
- app-critical paths remain on apex
- root on apex remains app-oriented, not marketing-oriented

This means Phase 1 does **not** try to make apex both a marketing home and an app home at once.

## 8.5 SEO posture in Phase 1

Phase 1 should treat `www` as the canonical marketing host.

Recommended:

- self-canonical tags on `www`
- update internal links to `www`
- update sitemaps and Search Console ownership as needed
- remove duplicate marketing content from apex or redirect it cleanly

Because apex is still the app at this stage, the SEO objective is not to make apex and `www` equivalent. The objective is to give marketing a stable public home while leaving the app alone.

---

## 9. Phase 2: Auth Broker Launch

Phase 2 introduces a stable auth broker as shared platform infrastructure.

This phase is intentionally **not** the final app-host migration. The Maple app still lives on `https://trymaple.ai` during this phase. The goal is to move the most fragile auth and callback plumbing onto a domain that is no longer tied to a single product host.

## 9.1 Public shape of Phase 2

- `https://trymaple.ai` = existing app
- `https://www.trymaple.ai` = marketing
- `https://auth.trymaple.ai` = auth broker

The exact broker hostname can be renamed later, but this document uses `auth.trymaple.ai` as the working assumption.

## 9.2 Scope of broker v1

Broker v1 should focus on auth-only concerns:

- OAuth initiation surfaces
- OAuth callback surfaces
- desktop/native auth launch and handoff
- post-auth targeting for approved Maple destinations

Broker v1 should first work only for the current Maple product, but be designed so it can later support:

- another Maple product on another domain, e.g. `mapleagent.com`
- a future product with desktop/mobile apps but no public web app
- product-specific post-auth handoff without hardcoding a single app host forever

## 9.3 What is explicitly out of scope for broker v1

Do **not** make broker v1 responsible for:

- Stripe success/cancel callbacks
- Zaprite success/cancel callbacks
- billing portal returns
- pricing pages or checkout initiation
- general app route handling

Those remain app-owned for now and should be treated as a separate enhancement track.

## 9.4 Product-agnostic design requirements

The auth broker should be a real broker, not a thin redirect page.

Required properties:

- exact callback handling for both GET and POST flows
- ability to terminate provider callbacks itself
- allowlisted target products or destinations
- signed state and anti-CSRF protections
- no open-redirect behavior
- `noindex` and non-marketing posture
- logging, observability, and clear error surfaces

Recommended conceptual model:

- providers call back to the broker
- the broker determines the intended product or target
- the broker completes auth and then hands off to an approved destination

## 9.5 Why Phase 2 helps Phase 3

Phase 2 reduces the amount of app-host-specific auth plumbing that needs to change on app migration day.

That means Phase 3 no longer has to do all of this at once:

- change app host
- change marketing host
- change OAuth callback host
- change desktop/native auth entry host

Instead, Phase 3 mostly becomes:

- app-host migration
- marketing-host migration
- client update/version enforcement
- billing and deep-link host migration

## 9.6 Why Phase 2 also helps future products

If Maple later launches another product that needs the same login stack, the broker creates a cleaner path to:

- share auth infrastructure across products
- avoid coupling callbacks to one product domain
- support future native-only products
- support approved post-auth handoff to a different product host or app

---

## 10. Feature-Flag and Remote-Config Strategy

This is one of the most important parts of the plan.

The migration becomes much safer if Maple ships host-awareness before Phases 2 and 3.

## 10.1 Main recommendation

Do not implement this as a collection of one-off booleans like:

- `useAppSubdomainForOAuth`
- `useAppSubdomainForBilling`
- `useAppSubdomainForLinks`

Instead, introduce a small remote-config or feature-config layer with canonical URL values.

## 10.2 Suggested config model

Introduce config like:

- `marketingOrigin`
- `appOrigin`
- `authBrokerOrigin`
- `legacyApexOrigin`
- `canonicalMarketingOrigin`
- `canonicalAppOrigin`
- `minimumSupportedDesktopVersion`
- `minimumSupportedIosVersion`
- `minimumSupportedAndroidVersion`

At minimum, the app should have a single source of truth for:

- app base URL
- auth broker base URL
- marketing base URL
- legacy host

## 10.3 URL builders that should exist

Centralize helpers for:

- desktop auth entry URL
- OAuth broker initiation URL
- OAuth callback URL
- post-auth target URL
- billing portal return URL
- checkout success URL
- checkout cancel URL
- API credits success URL
- API credits cancel URL
- privacy URL
- terms URL
- app home URL
- marketing home URL

Do not leave raw `https://trymaple.ai/...` strings in feature code.

## 10.4 Prep-release plan

Ship a prep release several weeks before Phase 2 that:

- supports both old and new host assumptions
- reads host/origin config from remote config or a feature flag system
- can flip external URL generation without requiring another client release

Then, before Phase 3, ship any remaining client changes needed for the final app-host migration.

The objective is not to run two public app hosts forever. The objective is to avoid a second emergency release just to change hardcoded URLs at the last minute.

## 10.5 Important limitation of feature flags

Feature flags help with:

- URLs generated in frontend code
- links opened from app UI
- desktop auth entry URLs
- broker initiation URLs
- billing return URLs generated client-side

Feature flags do **not** by themselves update:

- iOS associated domains
- Android app links
- `/.well-known` assets
- provider allowlists
- Apple Service ID or OAuth dashboard settings
- backend-generated email links

So the right model is:

- **prep release(s)**
- **broker rollout**
- **then a config flip later for the final host migration**

not:

- "feature flags mean app updates are never needed"

## 10.6 Reliability requirements for config

Do not make migration-day auth or billing depend on a fragile config fetch.

Required properties:

- cache last-known-good config locally
- use sticky persisted values after the cutover
- define safe bootstrap fallbacks
- make the host switch reversible

If the remote-config service fails, Maple should still know which host behavior to use.

---

## 11. Route and Surface Ownership Matrix

The table below describes the intended ownership across phases.

| Surface / Path Family                            | Current State                                 | Phase 1 Owner                                  | Phase 2 Owner                                  | Phase 3 Owner                                             | Notes                                                                                             |
| ------------------------------------------------ | --------------------------------------------- | ---------------------------------------------- | ---------------------------------------------- | --------------------------------------------------------- | ------------------------------------------------------------------------------------------------- |
| `/`                                              | Marketing when logged out, app when logged in | `trymaple.ai` app-oriented root                | `trymaple.ai` app-oriented root                | `trymaple.ai` marketing root, `app.trymaple.ai/` app root | Root can only have one owner per host                                                             |
| `/login`                                         | App/auth                                      | `trymaple.ai`                                  | `trymaple.ai`                                  | `app.trymaple.ai`                                         | In Phase 2 the UI still lives on apex, but auth initiation should start moving through the broker |
| `/signup`                                        | App/auth                                      | `trymaple.ai`                                  | `trymaple.ai`                                  | `app.trymaple.ai`                                         | Same as login                                                                                     |
| `/desktop-auth`                                  | Critical native OAuth entry                   | `trymaple.ai`                                  | `auth.trymaple.ai`                             | `auth.trymaple.ai`                                        | Should become broker-owned before final app cutover                                               |
| `/auth/$provider/callback`                       | Critical OAuth callback                       | `trymaple.ai`                                  | `auth.trymaple.ai`                             | `auth.trymaple.ai`                                        | Apple/Google/GitHub risk surface                                                                  |
| `/pricing`                                       | Transactional pricing + billing behavior      | `trymaple.ai`                                  | `trymaple.ai`                                  | `app.trymaple.ai`                                         | Separate marketing pricing page recommended                                                       |
| `/payment-success`                               | Payment return                                | `trymaple.ai`                                  | `trymaple.ai`                                  | `app.trymaple.ai`                                         | Still app-specific; not broker v1                                                                 |
| `/payment-canceled`                              | Payment return                                | `trymaple.ai`                                  | `trymaple.ai`                                  | `app.trymaple.ai`                                         | Still app-specific; not broker v1                                                                 |
| `/verify/$code`                                  | Verification return                           | `trymaple.ai`                                  | `trymaple.ai`                                  | `app.trymaple.ai`                                         | Confirm backend-generated links too                                                               |
| `/password-reset*`                               | Reset flow                                    | `trymaple.ai`                                  | `trymaple.ai`                                  | `app.trymaple.ai`                                         | Confirm backend-generated links too                                                               |
| `/team/invite/$inviteId`                         | Team invite acceptance                        | `trymaple.ai`                                  | `trymaple.ai`                                  | `app.trymaple.ai`                                         | Confirm email-generated links                                                                     |
| `/redeem`                                        | Redeem flow                                   | `trymaple.ai`                                  | `trymaple.ai`                                  | `app.trymaple.ai`                                         | Preserve post-auth behavior                                                                       |
| `auth.trymaple.ai/*` broker auth surfaces        | n/a                                           | n/a                                            | `auth.trymaple.ai`                             | `auth.trymaple.ai`                                        | New shared auth infrastructure                                                                    |
| `/.well-known/*`                                 | Mobile link association                       | `trymaple.ai`                                  | `trymaple.ai`                                  | `app.trymaple.ai`                                         | Need updated assets on final app host                                                             |
| `/about`, `/proof`, `/downloads`, `/teams`, etc. | Mixed marketing on app host                   | `www.trymaple.ai`                              | `www.trymaple.ai`                              | `trymaple.ai`                                             | Marketing only                                                                                    |
| `robots.txt`, `sitemap.xml`, `llms*.txt`         | Apex-hosted                                   | `www.trymaple.ai` if marketing canonical there | `www.trymaple.ai` if marketing canonical there | `trymaple.ai`                                             | Update canonicals/Search Console accordingly                                                      |

## 11.1 Path families that should not be treated as "just pages"

The following are protocol surfaces and should be handled with migration rigor:

- `/desktop-auth`
- `/auth/$provider/callback`
- `/payment-success`
- `/payment-canceled`
- `/.well-known/apple-app-site-association`
- `/.well-known/assetlinks.json`
- `auth.trymaple.ai/*`

## 11.2 Route family that needs product clarification

`/pricing` should be treated as two separate concerns:

- marketing/comparison page
- transactional app/billing page

Keeping those as one route forever will continue to blur the architecture.

## 11.3 Billing remains separate from broker v1

Even after the auth broker exists, billing remains app-owned in this plan.

That means:

- pricing pages stay with the app
- payment success/cancel stays with the app
- billing portal returns stay with the app

Those can be revisited later as a separate platform project if needed.

---

## 12. Code and Config Inventory to Update

This inventory is based on the current repo and should be expanded with backend/provider systems before Phase 2 and then revisited before Phase 3.

## 12.1 Frontend code paths already identified

### App and auth flows

- `frontend/src/routes/index.tsx`
- `frontend/src/routes/login.tsx`
- `frontend/src/routes/signup.tsx`
- `frontend/src/routes/desktop-auth.tsx`
- `frontend/src/routes/auth.$provider.callback.tsx`
- `frontend/src/components/AppleAuthProvider.tsx`
- `frontend/src/components/DeepLinkHandler.tsx`

### Billing and payment flows

- `frontend/src/routes/pricing.tsx`
- `frontend/src/routes/payment-success.tsx`
- `frontend/src/routes/payment-canceled.tsx`
- `frontend/src/billing/billingApi.ts`
- `frontend/src/components/apikeys/ApiCreditsSection.tsx`

### Public/marketing/static references

- `frontend/src/components/Marketing.tsx`
- `frontend/src/components/TopNav.tsx`
- `frontend/src/components/Footer.tsx`
- `frontend/src/components/AccountMenu.tsx`
- `frontend/index.html`
- `frontend/public/robots.txt`
- `frontend/public/sitemap.xml`
- `frontend/public/llms.txt`
- `frontend/public/llms-full.txt`

### Mobile/native configuration

- `frontend/src-tauri/tauri.conf.json`
- `frontend/src-tauri/gen/android/app/src/main/AndroidManifest.xml`
- `frontend/src-tauri/gen/apple/maple_iOS/maple_iOS.entitlements`
- `frontend/public/.well-known/apple-app-site-association`
- `frontend/public/.well-known/assetlinks.json`

## 12.2 External systems that must be inventoried outside this repo

The repo alone is not enough. Before Phase 2, create a complete external-system inventory for:

- GitHub OAuth app settings
- Google OAuth settings
- Apple developer settings and Service ID configuration
- Stripe dashboard success/cancel/portal settings
- Zaprite settings
- backend-generated verification/password-reset/invite links
- app store metadata and update rollout plans
- Search Console and analytics/canonical settings
- any status/help/support documentation that links directly to the app

## 12.3 Hidden-link inventory

Before Phase 3, also search for app-host assumptions in:

- support docs
- status page links
- QR codes
- email templates
- customer support macros
- onboarding messages
- billing emails
- in-app announcements
- social/profile links

---

## 13. Phase 1 Detailed Plan

This is the operational plan for the near-term marketing launch.

## 13.1 Product decisions for Phase 1

- `trymaple.ai` remains the app and critical-flow host
- `www.trymaple.ai` becomes the new marketing site
- apex stops serving rich marketing as its main logged-out experience
- apex may keep a slim app-entry page for logged-out users

## 13.2 Suggested Phase 1 route behavior

### On `trymaple.ai`

- `/` -> app-oriented entry point
- `/login`, `/signup` -> unchanged
- `/pricing` -> unchanged transactional pricing
- auth and billing callbacks -> unchanged
- native deep-link and universal-link paths -> unchanged

### On `www.trymaple.ai`

- `/` -> redesigned landing page
- marketing routes -> redesigned marketing pages
- pricing overview -> optional public marketing pricing page
- all product CTAs -> point to apex app for now

## 13.3 Suggested redirects in Phase 1

Safe candidates to redirect from apex to `www`:

- `/about`
- `/proof`
- `/downloads`
- `/teams` if it is purely marketing
- `/privacy`
- `/terms`

Do not redirect app-critical paths in Phase 1.

## 13.4 Phase 1 success criteria

- marketing has its own public site and deployment cadence
- app remains stable for existing users
- no provider callback changes are needed yet
- no mobile/desktop host migration is attempted yet
- no SEO collapse from duplicate content

---

## 14. Phase 2 and Phase 3 Detailed Plan

This section breaks the later work into two distinct phases:

- Phase 2 = auth broker launch
- Phase 3 = final app-host migration

## 14.1 Phase 2 preconditions

Do not launch the auth broker until all of these are true:

- host values are centralized in code
- auth-related URL generation uses helpers instead of scattered literals
- product/target allowlisting strategy is defined
- broker error handling and monitoring are designed
- broker host is deployed and testable
- broker callback handling is validated for both GET and POST-style flows

## 14.2 Phase 2 public behavior

### Marketing

- `www.trymaple.ai` remains the marketing site
- `trymaple.ai` remains the Maple app

### Auth

- OAuth and native auth plumbing starts moving to `auth.trymaple.ai`
- Maple still lands users back in the current Maple app experience
- first version is Maple-only in production behavior, even if the plumbing supports future products

## 14.3 Phase 2 desired outcome

By the end of Phase 2:

- app-host migration is no longer tightly coupled to callback-host migration
- desktop/native auth entry is broker-owned
- OAuth provider callbacks are broker-owned
- Maple has a stable auth infrastructure layer that can later support other products

## 14.4 Phase 3 preconditions

Do not schedule the final app-host migration until all of these are true:

- Phase 2 auth broker is live and stable
- prep releases are shipped
- mobile apps that understand the new app host are approved and sufficiently adopted
- provider dashboards and broker settings are prepared
- `app.trymaple.ai` is deployed and tested
- migration communications are ready
- minimum-version enforcement is implemented
- rollback runbook exists

## 14.5 Phase 3 public behavior on migration day

### Marketing

- `trymaple.ai` becomes the new marketing site
- `www.trymaple.ai` redirects to apex

### App

- `app.trymaple.ai` becomes the only official app host
- all marketing CTAs and official docs point there
- all app-owned callbacks and return URLs are updated there

### Auth

- `auth.trymaple.ai` remains the stable auth broker
- Phase 3 should not require inventing a new auth callback contract again

## 14.6 Phase 3 version-enforcement stance

By Phase 3, Maple should have the ability to enforce minimum supported versions for:

- desktop
- iOS
- Android
- web, where relevant

Desired behavior:

- users on sufficiently new clients continue smoothly
- users on old clients are blocked and instructed to update

This is what allows Maple to "rip the bandaid off" in a controlled way.

## 14.7 Phase 3 day-of operational sequence

Suggested high-level order:

1. freeze deploys unrelated to migration
2. enable maintenance window if needed
3. confirm `app.trymaple.ai` is healthy
4. confirm `auth.trymaple.ai` is healthy
5. update provider dashboards and app-owned callback/return URL settings
6. flip remote-config host values
7. enable minimum-version enforcement
8. move marketing to apex
9. redirect `www` to apex
10. validate smoke-test matrix
11. publish status update when complete

## 14.8 Phase 3 first-hour monitoring focus

During the first hour after cutover, watch:

- OAuth callback completion rates
- broker handoff success rates
- Apple-specific failure rates
- desktop-auth completion
- payment success/cancel completion
- mobile deep-link open rates
- verification and password-reset success
- support-ticket spike categories
- app-version distribution

---

## 15. Cloudflare Strategy

Cloudflare remains useful, but it is intentionally not the architecture.

## 15.1 What Cloudflare should do in Phase 1

- DNS and host routing for `www`
- redirects from old apex marketing pages to `www`
- optional safety and caching rules

## 15.2 What Cloudflare may do in Phase 2

Potential fallback behaviors:

- standard DNS/routing for `auth.trymaple.ai`
- safety rules around the broker host
- limited compatibility routing during broker rollout if needed

## 15.3 What Cloudflare may do in Phase 3

Potential fallback behaviors:

- redirect apex marketing routes to apex marketing origin
- redirect old app bookmarks on apex to `app.trymaple.ai`
- temporarily proxy or rewrite select legacy callback/deep-link routes if necessary

## 15.4 What Cloudflare should not be asked to guarantee

Cloudflare should not be the only reason these work:

- Apple web auth callback edge cases
- provider POST callback behavior
- long-tail legacy mobile app-link behavior
- billing return safety for every historical client version

If those work through Cloudflare too, that is helpful. But success criteria should not assume it.

## 15.5 Why this matters

Relying on Cloudflare as the main contract turns the migration into a hidden path-routing system again.

That is exactly what this plan is trying to avoid.

---

## 16. SEO Plan

SEO matters across all three phases, but the canonical host changes over time.

## 16.1 SEO in Phase 1

Canonical marketing host:

- `www.trymaple.ai`

Recommended actions:

- self-canonical tags on `www`
- redirects from old apex marketing pages to `www`
- updated sitemap and robots for the `www` marketing experience
- Search Console setup for `www`
- updated internal links

## 16.2 SEO in Phase 2

Canonical marketing host:

- `www.trymaple.ai`

Additional Phase 2 SEO rule:

- `auth.trymaple.ai` should be treated as infrastructure, not marketing content
- broker pages and callbacks should be `noindex`

## 16.3 SEO in Phase 3

Canonical marketing host:

- `trymaple.ai`

Recommended actions:

- `www` 301 to apex
- updated canonical tags on apex
- updated sitemap and robots on apex
- Search Console/property updates if needed
- update structured metadata and social tags

## 16.4 App indexing posture

`app.trymaple.ai` should generally be treated as the product host, not the SEO/marketing host.

Recommended:

- avoid indexing app shell pages as if they are public marketing pages
- decide intentionally whether login/signup pages should be indexable

## 16.5 Public artifact ownership

The following should live with whichever host is the canonical marketing host in that phase:

- `robots.txt`
- `sitemap.xml`
- `llms.txt`
- `llms-full.txt`
- public metadata references in `index.html`

---

## 17. OAuth, Billing, and Native Platform Plan

This section is the highest-risk part of the migration.

## 17.1 Auth broker as shared auth infrastructure

Phase 2 should move Maple's auth contract onto a dedicated broker host.

The first implementation is Maple-only in production behavior, but the design should support future Maple products.

### Broker-owned concerns

- OAuth initiation
- OAuth callback handling
- desktop/native auth launch and handoff
- approved target selection and post-auth handoff

### Broker requirements

- handle GitHub, Google, and Apple
- accept exact provider callback semantics, including POST-capable Apple flows
- support a safe allowlist of destinations or product identifiers
- avoid open redirects
- produce good logs and monitoring

### Apple-specific notes

- web callback host changes matter directly because `redirectURI` is built from `window.location.origin`
- form-post behavior needs dedicated testing
- iOS native Sign in with Apple is a separate surface and still needs host-aware app updates around surrounding flows

## 17.2 OAuth providers

Maple needs a full inventory and explicit migration checklist for:

- GitHub
- Google
- Apple

### Requirements

- add new broker callback URLs where providers allow overlap
- rehearse exact settings changes
- know which providers support multiple callbacks versus one exact callback
- separate "broker callback host" decisions from "final app host" decisions

## 17.3 Billing providers remain app-specific

Inventory and update:

- Stripe checkout success URL
- Stripe cancel URL
- billing portal return URL
- Zaprite return URLs
- API-credits purchase return URLs

### Requirements

- do not move these into broker v1
- keep product code generating those URLs via central helpers
- update them during Phase 3 app migration unless a later billing-broker project is explicitly created

## 17.4 Mobile deep links and app links

Before Phase 3, new client builds must support the new app host.

### iOS

- update associated domains
- update AASA host ownership
- validate universal link behavior

### Android

- update intent filters / app links
- update `assetlinks.json`
- validate app-link opening

### Tauri/mobile config

- update `tauri.conf.json` and generated platform config
- update any host-specific capability allowlists

## 17.5 Desktop auth

Desktop auth currently depends on:

- `/desktop-auth`
- external browser flow
- `cloud.opensecret.maple://auth?...`

Phase 2 should move desktop auth entry and callback plumbing onto the broker so Phase 3 does not need to change both app host and auth host at the same time.

---

## 18. Testing Strategy

There is no honest way to promise literal zero-risk for every third-party flow before going live.

What Maple can do is reduce risk by staging, centralizing, and testing aggressively.

## 18.1 Phase 1 test matrix

### Web

- logged-out root behavior on apex
- marketing homepage on `www`
- redirects from old apex marketing routes to `www`
- login/signup on apex
- pricing checkout on apex
- verification/reset/invite flows on apex

### Native

- desktop OAuth continues using apex
- iOS/Android payment deep links continue using apex

### SEO

- canonical tags
- sitemap
- robots
- duplicate-content check between apex and `www`

## 18.2 Phase 2 auth-broker test matrix

- broker initiation for GitHub, Google, and Apple
- broker callback completion for GET-based flows
- broker callback completion for POST-capable Apple flows
- desktop/native auth handoff through broker
- allowlisted target selection and rejection behavior
- no open-redirect behavior
- broker error pages and observability
- Maple app still functioning on apex while broker is introduced

## 18.3 Prep-release test matrix before Phase 3

- app builds can generate `app.trymaple.ai` URLs when config says so
- app builds can use broker-owned auth flows
- app builds still work with old config
- config caching and fallback works offline or during config-service issues
- minimum-version enforcement screen works

## 18.4 Phase 3 migration-day smoke tests

### Web auth

- GitHub login
- Google login
- Apple login
- Apple callback completion
- broker handoff to `app.trymaple.ai`

### Billing

- Stripe checkout success
- Stripe checkout cancel
- Zaprite checkout success
- billing portal return
- API credits success/cancel

### Account flows

- verify email
- password reset
- team invite
- redeem flow

### Native

- desktop auth launch to `app.trymaple.ai`
- desktop return to app
- iOS payment success/cancel link handling
- Android payment success/cancel link handling

### Marketing

- apex marketing render
- `www` redirect to apex
- marketing CTA to app

## 18.5 Post-cutover validation

- support inbox category review
- analytics comparison vs baseline
- callback success-rate monitoring
- broker handoff success monitoring
- app-version cohort monitoring
- legacy-host hit monitoring

---

## 19. Rollback Strategy

Even a deliberate auth-broker rollout and a deliberate app cutover need rollback plans.

## 19.1 Rollback objective

Be able to restore working auth/payment access quickly if either the broker rollout or the final cutover has a critical flaw.

## 19.2 Rollback levers

- revert remote-config host values
- revert broker target configuration if needed
- relax minimum-version enforcement temporarily
- restore provider callback/return settings if needed
- restore previous apex behavior temporarily
- use Cloudflare fallback rules selectively

## 19.3 What should remain available during the rollback window

For some period after Phase 3, keep ready access to:

- old provider settings
- old Cloudflare rules
- old SEO config
- previous client config defaults

## 19.4 Timeboxed fallback policy

It is reasonable to keep compatibility tools available for a short time even if the strategic goal is a hard break.

That is not architectural indecision. It is operational prudence.

---

## 20. Communication Plan

Because Phase 3 is intentionally disruptive, communication is part of the technical plan.

Phase 2 may be a quieter infrastructure launch, but internal teams should still know when the broker becomes authoritative for auth flows.

## 20.1 Pre-announce the migration

Communicate:

- the date
- the new app URL
- the fact that old clients may need updates
- what users should do if login or payment stops working

## 20.2 Channels

- in-app banner
- email
- website announcement
- release notes
- status page
- support macros
- social/community channels if relevant

## 20.3 Messaging themes

- app is moving to `app.trymaple.ai`
- marketing will live at `trymaple.ai`
- users should update desktop/mobile apps before the migration date
- bookmarks should be updated

## 20.4 Day-of communication

- maintenance notice if needed
- progress updates on status page
- completion announcement
- clear support instructions if users are stuck on old versions

---

## 21. Timeline Recommendation

The exact dates can change, but the sequence should look roughly like this.

## 21.1 Phase 1 window

- launch `www.trymaple.ai`
- simplify apex marketing footprint
- keep app-critical routes stable on apex

## 21.2 Phase 2 preparation window

- centralize URLs in code
- implement remote-config host layer
- inventory external systems
- design auth broker target/allowlist model
- define minimum-version enforcement
- normalize odd flows like `payment-success-credits`

## 21.3 Phase 2 launch window

- deploy `auth.trymaple.ai`
- move OAuth and desktop/native auth plumbing onto the broker
- validate end-to-end behavior privately and then in production
- confirm Maple still works cleanly while the app remains on apex

## 21.4 Phase 3 preparation window

- ship prep release(s) to desktop/mobile
- begin app-store approval runway
- dark-launch `app.trymaple.ai`
- finalize provider settings runbook
- finalize Cloudflare fallback rules
- announce migration publicly
- confirm support readiness

## 21.5 Phase 3 migration day

- freeze deploys
- apply provider and app-owned callback changes
- flip config
- enable version enforcement
- move marketing to apex
- redirect `www`
- run smoke tests

## 21.6 T+1 to T+14 days after Phase 3

- monitor legacy traffic
- handle support issues
- keep rollback tools available
- decide when to remove temporary fallbacks

---

## 22. Work Breakdown Structure

## 22.1 Workstream A: URL centralization

- create canonical URL config layer
- replace hardcoded apex literals
- add route builders

## 22.2 Workstream B: marketing split

- build and deploy `www`
- redirect old marketing routes appropriately
- simplify logged-out apex entry

## 22.3 Workstream C: auth broker

- design broker target model
- build broker callback handling
- build desktop/native auth handoff
- harden Apple callback handling
- add broker monitoring and logs

## 22.4 Workstream D: app-host prep

- deploy `app.trymaple.ai`
- prep release with host flags/config
- validate broker/app/billing/deep-link support

## 22.5 Workstream E: provider and platform updates

- OAuth dashboards
- billing dashboards
- iOS/Android app links
- `.well-known` files
- app-store releases

## 22.6 Workstream F: migration-day operations

- runbook
- smoke tests
- monitoring
- rollback plan
- communications

---

## 23. Open Questions Requiring Explicit Decisions

These decisions should be made before implementation begins.

## 23.1 Auth broker host and branding

Question:

- Is the final auth-broker host definitely `auth.trymaple.ai`, or should it use a different neutral name?

Recommended answer:

- choose a stable, infra-flavored host
- avoid names that imply a temporary redirect shim

## 23.2 Auth broker target model

Question:

- How should the broker identify approved destinations for Maple and future products?

Recommended answer:

- use allowlisted product identifiers or signed targets
- do not accept arbitrary redirect URLs

## 23.3 Pricing split

Question:

- Does apex marketing need a public `/pricing` page after Phase 3?

Recommended answer:

- yes, but make it marketing-only
- keep app checkout and billing operations on `app.trymaple.ai/pricing`

## 23.4 Legacy-host support window

Question:

- How long should best-effort apex compatibility remain after Phase 3?

Recommended answer:

- timebox it
- do not let it become an undeclared permanent architecture

## 23.5 Minimum-version policy

Question:

- How aggressively should old versions be blocked?

Recommended answer:

- make the policy explicit before Phase 3
- especially for mobile, where store uptake is slower than web

## 23.6 Backend-generated links

Question:

- Which backend systems generate public links for verify/reset/invite flows?

Recommended answer:

- inventory and migrate them explicitly
- do not assume the frontend repo is the whole truth

---

## 24. Final Recommendation

Maple should **not** attempt the final app host migration during the current redesign effort.

Instead:

### Right now

- launch the new marketing site on `www.trymaple.ai`
- keep the app on `trymaple.ai`
- remove or redirect marketing content away from apex where appropriate

### Next

- centralize URLs
- ship a prep release with feature-flagged or remote-configured host generation
- introduce a real auth broker on `auth.trymaple.ai`
- move OAuth and native auth plumbing onto the broker

### After that

- stage provider and platform changes for the final app-host migration
- ship remaining client updates
- dark-launch `app.trymaple.ai`

### Later

- choose a published migration day
- move the canonical app to `app.trymaple.ai`
- move canonical marketing to apex
- redirect `www` to apex
- keep auth on the broker host
- require client updates through version enforcement
- treat Cloudflare fallback as optional insurance, not the architecture

This gives Maple:

- a low-risk redesign path
- a reusable auth platform layer
- a clean eventual domain model
- a deliberate migration day
- a way to avoid being trapped by permanent same-host route ownership hacks

---

## 25. Appendix: Key Repo Evidence

The following files were the main basis for this plan:

- `frontend/src/routes/index.tsx`
- `frontend/src/routes/login.tsx`
- `frontend/src/routes/signup.tsx`
- `frontend/src/routes/desktop-auth.tsx`
- `frontend/src/routes/auth.$provider.callback.tsx`
- `frontend/src/routes/pricing.tsx`
- `frontend/src/routes/payment-success.tsx`
- `frontend/src/routes/payment-canceled.tsx`
- `frontend/src/billing/billingApi.ts`
- `frontend/src/components/AppleAuthProvider.tsx`
- `frontend/src/components/DeepLinkHandler.tsx`
- `frontend/src/components/apikeys/ApiCreditsSection.tsx`
- `frontend/src/components/AccountMenu.tsx`
- `frontend/src-tauri/tauri.conf.json`
- `frontend/src-tauri/gen/android/app/src/main/AndroidManifest.xml`
- `frontend/src-tauri/gen/apple/maple_iOS/maple_iOS.entitlements`
- `frontend/public/.well-known/apple-app-site-association`
- `frontend/public/.well-known/assetlinks.json`
- `frontend/public/robots.txt`
- `frontend/public/sitemap.xml`
- `frontend/public/llms.txt`
- `frontend/public/llms-full.txt`

### Most important repo-specific takeaways

- The root route currently mixes marketing and app concerns.
- The billing and pricing flow is not just a brochure page.
- Desktop auth and native handoff currently depend on apex.
- Mobile app-link and universal-link config currently depends on apex.
- Apple web auth host behavior is sensitive to the current origin.
- There are enough hardcoded apex URLs that a pure one-day string-replacement migration would be too risky.
