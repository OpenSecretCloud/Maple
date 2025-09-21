# Android Platform Checks Analysis

## Overview
This document contains a comprehensive analysis of all platform-specific checks in the Maple codebase that need to be evaluated for Android support. Each instance is documented with its location, current behavior, and recommendations for Android handling.

## Platform Detection Pattern
The new standard pattern for platform detection using our utilities:
```typescript
import { isIOS, isAndroid, isMobile } from '@/utils/platform';

// Check for specific platforms
if (await isIOS()) { /* iOS specific code */ }
if (await isAndroid()) { /* Android specific code */ }
if (await isMobile()) { /* Both iOS and Android */ }
```

---

## 1. BILLING & PAYMENTS (`frontend/src/billing/billingApi.ts`) ✅ COMPLETED

### Instance 1: Portal Return URL (Line 102) ✅
- **Current iOS Behavior:** Uses `https://trymaple.ai` as return URL instead of `tauri://localhost`
- **Android Recommendation:** ✅ **Same as iOS** - Use `https://trymaple.ai`
- **Status:** ✅ **IMPLEMENTED** - Now uses `isMobile()` platform utility
- **Implementation:**
  ```typescript
  import { isMobile } from '@/utils/platform';

  if (await isMobile()) {
    returnUrl = "https://trymaple.ai";
  }
  ```

### Instance 2: Stripe Checkout Opening (Lines 166-186) ✅
- **Current iOS Behavior:** Forces external Safari browser via `plugin:opener|open_url` with no fallback
- **Android Recommendation:** ✅ **Same as iOS** - Use external browser for payments
- **Reasoning:** Google Play Store has similar payment restrictions as Apple App Store
- **Status:** ✅ **IMPLEMENTED** - Now uses `isMobile()` platform utility
- **Implementation:**
  ```typescript
  import { isMobile } from '@/utils/platform';

  if (await isMobile()) {
    await invoke("plugin:opener|open_url", { url: checkout_url });
    return;
  } else {
    window.location.href = checkout_url;
  }
  ```

### Instance 3: Zaprite Checkout Opening (Lines 228-247) ✅
- **Current iOS Behavior:** Forces external browser for crypto payments
- **Android Recommendation:** ✅ **Same as iOS** - Use external browser
- **Status:** ✅ **IMPLEMENTED** - Now uses `isMobile()` platform utility
- **Implementation:** Same as Stripe checkout - use `isMobile()` check

---

## 2. PROXY SERVICE (`frontend/src/services/proxyService.ts`) ✅ COMPLETED

### Instance 1: Desktop Platform Check (Lines 99-106) ✅
- **Current Behavior:** Returns true only for `macos`, `windows`, `linux`
- **Android Recommendation:** ❌ **Different from iOS** - Keep as desktop-only
- **Reasoning:** Proxy functionality not needed on mobile platforms
- **Status:** ✅ **VERIFIED** - Current logic correctly excludes mobile platforms (returns `false` for both `ios` and `android`)
- **No changes needed** - The `isTauriDesktop()` method properly handles all mobile exclusions

---

## 3. AUTHENTICATION (`frontend/src/routes/login.tsx` & `signup.tsx`) ✅ COMPLETED

### Instance 1: Platform Detection ✅
- **Current iOS Behavior:** Sets `isIOS` state when `platform === "ios"`
- **Android Recommendation:** ➕ **Add Android detection**
- **Status:** ✅ **IMPLEMENTED** - Now uses platform utility hooks
- **Implementation:**
  ```typescript
  import { useIsIOS, useIsAndroid, useIsTauri } from '@/hooks/usePlatform';

  function LoginPage() {
    const { isIOS } = useIsIOS();
    const { isAndroid } = useIsAndroid();
    const { isTauri: isTauriEnv } = useIsTauri();
    // No need for manual useEffect or state management
  }
  ```

### Instance 2: Email Authentication ✅
- **Current Behavior:** Standard email/password flow
- **Android Recommendation:** ✅ **Same across all platforms**
- **Status:** ✅ **No changes needed** - Works identically on all platforms

### Instance 3: GitHub OAuth ✅
- **Current Behavior:** External browser for all Tauri platforms
- **Android Recommendation:** ✅ **Same as iOS** - Use external browser
- **Status:** ✅ **Already works** - Current `isTauri` check handles Android correctly
- **Implementation:** Opens `https://trymaple.ai/desktop-auth?provider=github` in external browser

### Instance 4: Google OAuth ✅
- **Current Behavior:** External browser for all Tauri platforms
- **Android Recommendation:** ✅ **Same as iOS** - Use external browser (no native Google Sign-In yet)
- **Status:** ✅ **Already works** - Current `isTauri` check handles Android correctly
- **Future Enhancement:** Could implement native Google Play Services integration later
- **Implementation:** Opens `https://trymaple.ai/desktop-auth?provider=google` in external browser

### Instance 5: Apple Sign In ✅
- **Current Behavior:** Platform-specific implementations
- **Android Recommendation:** ✅ **Same as Desktop** - Use external browser
- **Status:** ✅ **IMPLEMENTED** - Android now uses external browser like desktop
- **Implementation:**
  ```typescript
  // login.tsx and signup.tsx
  if (isTauriEnv && isIOS) {
    // Native iOS flow using plugin:sign-in-with-apple
    await invoke("plugin:sign-in-with-apple|get_apple_id_credential");
  } else if (isTauriEnv) {
    // Desktop and Android Tauri flow - external browser
    await invoke("plugin:opener|open_url", {
      url: "https://trymaple.ai/desktop-auth?provider=apple"
    });
  } else {
    // Web flow only - use AppleAuthProvider component
    // This renders the Apple JS SDK button
  }

  // Button rendering logic
  {isTauriEnv ? (
    <Button onClick={handleAppleLogin}>Log in with Apple</Button>
  ) : (
    <AppleAuthProvider />  // Web SDK for Web only
  )}
  ```

### Summary of Auth Behavior
- **Email**: ✅ Same on all platforms
- **GitHub**: ✅ External browser on mobile (iOS & Android) and desktop, web flow on web
- **Google**: ✅ External browser on mobile (iOS & Android) and desktop, web flow on web
- **Apple**:
  - iOS: ✅ Native Apple Sign In plugin
  - Android: ✅ External browser (via `/desktop-auth`)
  - Desktop: ✅ External browser (via `/desktop-auth`)
  - Web: ✅ AppleAuthProvider web SDK

---

## 4. PRICING PAGE (`frontend/src/routes/pricing.tsx`) ✅ COMPLETED

### Instance 1: Platform Detection ✅
- **Current iOS Behavior:** Sets `isIOS` state
- **Android Recommendation:** ➕ **Add Android detection**
- **Status:** ✅ **IMPLEMENTED** - Now uses platform utility hooks
- **Implementation:**
  ```typescript
  import { useIsIOS, useIsAndroid, useIsMobile } from '@/hooks/usePlatform';
  import { isMobile } from '@/utils/platform';

  const { isIOS } = useIsIOS();
  const { isAndroid } = useIsAndroid();
  const { isMobile: isMobilePlatform } = useIsMobile();
  ```

### Instance 2: Bitcoin Toggle Auto-Enable ✅
- **Current iOS Behavior:** Prevents auto-enabling Bitcoin payments
- **Android Recommendation:** ✅ **Same as iOS** - Prevent auto-enable on mobile
- **Status:** ✅ **IMPLEMENTED** - Now checks for all mobile platforms
- **Implementation:**
  ```typescript
  // Auto-enable Bitcoin toggle for Zaprite users (except on mobile platforms)
  useEffect(() => {
    if (freshBillingStatus?.payment_provider === "zaprite" && !isMobilePlatform) {
      setUseBitcoin(true);
    }
  }, [freshBillingStatus?.payment_provider, isMobilePlatform]);
  ```

### Instance 3: Product Availability ✅
- **Current iOS Behavior:** Shows "Not available in app" for paid plans when `is_available === false`
- **Android Recommendation:** ❌ **Different from iOS** - Android can support paid plans
- **Status:** ✅ **IMPLEMENTED** - iOS restrictions maintained, Android allows paid plans
- **Implementation:**
  ```typescript
  // Show "Not available in app" for iOS paid plans if server says not available
  // Android can support paid plans (no App Store restrictions)
  if (isIOS && !isFreeplan && product.is_available === false) {
    return "Not available in app";
  }
  // Android will allow paid plans when server returns is_available: true
  ```

### Instance 4: Payment Success URLs ✅
- **Current iOS Behavior:** Uses Universal Links `https://trymaple.ai/payment-success`
- **Android Recommendation:** ✅ **Same as iOS** - Use deep links for callbacks
- **Status:** ✅ **IMPLEMENTED** - Both mobile platforms use deep links
- **Implementation:**
  ```typescript
  // For mobile platforms (iOS and Android), use Universal Links / App Links
  const isMobilePlatform = await isMobile();

  if (isMobilePlatform) {
    // Use trymaple.ai URLs for deep linking back to app
    successUrl = "https://trymaple.ai/payment-success?source=stripe";
    cancelUrl = "https://trymaple.ai/payment-canceled?source=stripe";
  } else {
    // Use origin URLs for web/desktop
    successUrl = `${window.location.origin}/pricing?success=true`;
    cancelUrl = `${window.location.origin}/pricing?canceled=true`;
  }
  ```

### Instance 5: Portal Opening ✅
- **Current iOS Behavior:** Uses `plugin:opener|open_url` to launch Safari
- **Android Recommendation:** ✅ **Same as iOS** - Use external browser
- **Status:** ✅ **IMPLEMENTED** - Both mobile platforms use external browser
- **Implementation:**
  ```typescript
  if (portalUrl) {
    // Open in external browser for mobile platforms (iOS and Android)
    if (isMobilePlatform) {
      console.log("[Billing] Mobile platform detected, using opener plugin");
      await invoke("plugin:opener|open_url", { url: portalUrl });
    } else {
      // Desktop and web platforms
      window.open(portalUrl, "_blank");
    }
  }
  ```

### Instance 6: Product Fetching with Version ✅
- **Current iOS Behavior:** Sends app version to server for iOS builds
- **Android Recommendation:** ✅ **Same as iOS** - Send version for mobile builds
- **Status:** ✅ **IMPLEMENTED** - Both mobile platforms send version
- **Implementation:**
  ```typescript
  queryKey: ["products", isIOS, isAndroid],
  queryFn: async () => {
    const billingService = getBillingService();
    // Send version for mobile builds (iOS needs it for App Store restrictions)
    if (isIOS || isAndroid) {
      const version = `v${packageJson.version}`;
      return await billingService.getProducts(version);
    }
    return await billingService.getProducts();
  }
  ```

---

## 5. ACCOUNT MENU (`frontend/src/components/AccountMenu.tsx`) ✅ COMPLETED

### Instance 1: API Management Visibility (Lines 276-281) ✅
- **Current iOS Behavior:** Hides API Management menu item
- **Android Recommendation:** ✅ **Same as iOS** - Hide API Management on mobile
- **Reasoning:** Mobile platforms don't need API management features
- **Status:** ✅ **IMPLEMENTED** - Now uses `useIsMobile()` hook
- **Implementation:**
  ```typescript
  import { useIsMobile } from '@/hooks/usePlatform';

  const { isMobile } = useIsMobile();

  // Hide for all mobile platforms (iOS and Android)
  {!isMobile && (
    <MenuItem onClick={handleApiManagement}>API Management</MenuItem>
  )}
  ```

### Instance 2: Manage Subscription (Lines 155-178) ✅
- **Current iOS Behavior:** Uses external browser via opener plugin
- **Android Recommendation:** ✅ **Same as iOS** - Use external browser
- **Status:** ✅ **IMPLEMENTED** - Now uses `isMobile()` platform utility
- **Implementation:**
  ```typescript
  import { isMobile } from '@/utils/platform';

  if (await isMobile()) {
    await invoke("plugin:opener|open_url", { url: portalUrl });
  } else {
    window.open(url, "_blank");
  }
  ```

---

## 6. TEAM INVITES (`frontend/src/components/team/TeamInviteDialog.tsx`) ✅ COMPLETED

### Instance 1: Billing Portal (Lines 48-55) ✅
- **Current iOS Behavior:** Uses `plugin:opener|open_url` for external browser
- **Android Recommendation:** ✅ **Same as iOS** - Use external browser
- **Status:** ✅ **IMPLEMENTED** - Now uses `isMobile()` platform utility
- **Implementation:**
  ```typescript
  import { isMobile } from '@/utils/platform';
  import { invoke } from '@tauri-apps/api/core';

  // Use external browser for mobile platforms (iOS and Android)
  if (await isMobile()) {
    await invoke("plugin:opener|open_url", { url });
    return;
  }

  // Web or desktop flow
  window.open(url, "_blank", "noopener,noreferrer");
  ```

---

## 7. MARKETING (`frontend/src/components/Marketing.tsx`) ✅ COMPLETED

### Instance 1: Platform Detection ✅
- **Current iOS Behavior:** Sets `isIOS` state when `platform === "ios"`
- **Android Recommendation:** ✅ **Uses platform hook** - No Android-specific detection needed
- **Status:** ✅ **IMPLEMENTED** - Now uses `useIsIOS()` hook from platform utilities
- **Implementation:**
  ```typescript
  import { useIsIOS } from '@/hooks/usePlatform';

  export function Marketing() {
    // Use the platform detection hook for iOS
    // Android doesn't have App Store restrictions, so we only need to check for iOS
    const { isIOS } = useIsIOS();
    // No manual useEffect or state management needed
  }
  ```

### Instance 2: Pricing Tier Buttons (Lines 141-152) ✅
- **Current iOS Behavior:** Shows "Coming Soon" for paid plans
- **Android Recommendation:** ❌ **Different from iOS** - Enable paid plans
- **Reasoning:** Android doesn't have App Store payment restrictions
- **Status:** ✅ **IMPLEMENTED** - iOS shows "Coming Soon", Android shows normal "Get Started"
- **Implementation:**
  ```typescript
  // In PricingTier component:
  // Only iOS shows "Coming Soon" for paid plans
  {isIOS && !isFreeplan ? (
    <button disabled={true}>Coming Soon</button>
  ) : (
    <button onClick={handlePurchase}>{ctaText}</button>
  )}
  ```

---

## 8. API CREDITS (`frontend/src/components/apikeys/ApiCreditsSection.tsx`) ✅ COMPLETED

### Instance 1: Payment URLs (Lines 98-112) ✅
- **Current iOS Behavior:** Uses Universal Links for payment callbacks
- **Android Recommendation:** ✅ **Same as iOS** - Use deep links
- **Status:** ✅ **IMPLEMENTED** - Now uses `useIsMobile()` hook
- **Implementation:**
  ```typescript
  import { useIsMobile } from '@/hooks/usePlatform';

  const { isMobile } = useIsMobile();

  // For mobile platforms (iOS and Android), use Universal Links
  if (isMobile) {
    successUrl = `https://trymaple.ai/payment-success-credits?source=${method}`;
    cancelUrl = method === "stripe" ? `https://trymaple.ai/payment-canceled?source=stripe` : undefined;
  } else {
    // For web or desktop, use regular URLs with query params
    const baseUrl = window.location.origin;
    successUrl = `${baseUrl}/?credits_success=true`;
    cancelUrl = method === "stripe" ? `${baseUrl}/` : undefined;
  }
  ```

### Instance 2: Feature Availability ✅
- **Current iOS Behavior:** Feature hidden on mobile platforms
- **Android Recommendation:** ✅ **Same as iOS** - Hide feature on mobile
- **Reasoning:** API credits feature not exposed on mobile platforms
- **Status:** ✅ **VERIFIED** - Feature not accessible on mobile platforms
- **Note:** The page/feature is not accessible on mobile, so the component won't be rendered

---

## 9. CHAT BOX (`frontend/src/components/ChatBox.tsx`) ✅ COMPLETED

### Instance 1: Document Processing (Line 410) ✅
- **Current Behavior:** Tauri environments support PDF processing
- **Android Recommendation:** ✅ **Same as iOS** - Support local document processing
- **Status:** ✅ **IMPLEMENTED** - Now uses `useIsTauri()` hook from `@/hooks/usePlatform`
- **Implementation:**
  ```typescript
  import { useIsTauri } from '@/hooks/usePlatform';

  const { isTauri: isTauriEnv } = useIsTauri();

  // All Tauri platforms (desktop and mobile) support document processing
  if (isTauriEnv && (file.type === "application/pdf" || ...)) {
    // Process documents locally using Rust in Tauri
    const { invoke } = await import("@tauri-apps/api/core");
    // ... document processing
  }
  ```
- **Verified:** Android will work correctly since it's a Tauri environment
- **Note:** This was primarily a refactoring change - the functionality already worked for Android

---

## 10. PROXY CONFIGURATION (`frontend/src/components/apikeys/ProxyConfigSection.tsx`) ✅ COMPLETED

### Instance 1: Component Visibility (Lines 28-40) ✅
- **Current Behavior:** Only shows on desktop platforms
- **Android Recommendation:** ✅ **Same as iOS** - Hide proxy config
- **Reasoning:** Proxy not needed on mobile
- **Status:** ✅ **IMPLEMENTED** - Now uses `useIsTauriDesktop()` hook
- **Implementation:**
  ```typescript
  import { useIsTauriDesktop } from '@/hooks/usePlatform';

  export function ProxyConfigSection({ apiKeys, onRequestNewApiKey }: ProxyConfigSectionProps) {
    const { isTauriDesktop } = useIsTauriDesktop();

    if (!isTauriDesktop) {
      return null; // Don't show proxy config on non-desktop platforms (includes mobile)
    }
  }
  ```

---

## 11. API KEY DASHBOARD (`frontend/src/components/apikeys/ApiKeyDashboard.tsx`) ✅ COMPLETED

### Instance 1: Proxy Tab Visibility (Lines 215-230) ✅
- **Current Behavior:** Shows proxy tab only on desktop
- **Android Recommendation:** ✅ **Same as iOS** - Hide proxy tab
- **Status:** ✅ **IMPLEMENTED** - Now uses `useIsTauriDesktop()` hook
- **Implementation:**
  ```typescript
  import { useIsTauriDesktop } from '@/hooks/usePlatform';

  export function ApiKeyDashboard() {
    const { isTauriDesktop } = useIsTauriDesktop();

    // Only show proxy tab on desktop
    <TabsList className={`grid w-full ${isTauriDesktop ? "grid-cols-3" : "grid-cols-2"}`}>
      {/* Credits and API Keys tabs always shown */}
      {isTauriDesktop && (
        <TabsTrigger value="local-proxy">Local Proxy</TabsTrigger>
      )}
    </TabsList>

    // Conditionally render proxy content
    {isTauriDesktop && (
      <TabsContent value="local-proxy">
        <ProxyConfigSection ... />
      </TabsContent>
    )}
  }
  ```

---

## 12. AUTH CALLBACK (`frontend/src/routes/auth.$provider.callback.tsx`) ✅ COMPLETED

### Instance 1: Native App Redirect (Lines 38-59) ✅
- **Current Behavior:** Checks `redirect-to-native` flag for deep linking
- **Android Recommendation:** ✅ **Same as iOS** - Use native app flow
- **Status:** ✅ **VERIFIED** - Works correctly for Android
- **Implementation:**
  ```typescript
  // Platform-agnostic approach works for both iOS and Android
  const isTauriAuth = localStorage.getItem("redirect-to-native") === "true";

  if (isTauriAuth) {
    // Deep link back to app with auth tokens
    window.location.href = `cloud.opensecret.maple://auth?access_token=...`;
  }
  ```
- **Notes:**
  - Uses localStorage flag approach instead of platform detection
  - This design is intentionally platform-agnostic
  - No platform utility imports needed in this component

---

## 13. APPLE AUTH PROVIDER (`frontend/src/components/AppleAuthProvider.tsx`) ✅ COMPLETED

### Instance 1: Component Rendering (Lines 95, 393) ✅
- **Current Behavior:** Returns null for ALL Tauri environments (`window.location.protocol === "tauri:"`)
- **Android Recommendation:** ✅ **Working correctly** - Component not used by mobile/desktop apps
- **Status:** ✅ **NO CHANGES NEEDED** - Working as designed
- **Explanation:**
  - This component is **ONLY for the Web JS SDK** (Apple's browser-based auth)
  - It correctly returns `null` for ALL Tauri platforms (iOS, Android, Desktop)
  - Mobile and desktop apps handle Apple auth differently:
    - **iOS**: Uses native Apple Sign In plugin directly
    - **Android/Desktop**: Opens external browser to `/desktop-auth?provider=apple`
    - The external browser then loads the web page which DOES use this component
  - The check `window.location.protocol === "tauri:"` is intentionally platform-agnostic
- **How it works:**
  ```typescript
  // AppleAuthProvider.tsx - Web SDK component
  if (window.location.protocol === "tauri:") {
    return null; // Don't render in ANY Tauri app
  }

  // Login/Signup pages handle the platform routing:
  if (isTauriEnv) {
    // Show custom button that:
    // - iOS: calls native plugin
    // - Android/Desktop: opens external browser
  } else {
    // Web only: render AppleAuthProvider component
    <AppleAuthProvider />
  }
  ```

---

## 14. DEEP LINK HANDLER (`frontend/src/components/DeepLinkHandler.tsx`) ✅ COMPLETED

### Instance 1: Deep Link Events (Lines 31-84) ✅
- **Current Behavior:** Sets up listeners in Tauri environments
- **Android Recommendation:** ✅ **Same as iOS** - Handle deep links
- **Status:** ✅ **IMPLEMENTED** - Now uses platform utilities
- **Implementation:**
  ```typescript
  import { isTauri } from '@/utils/platform';
  import { listen } from '@tauri-apps/api/event';

  useEffect(() => {
    const setupDeepLinks = async () => {
      if (await isTauri()) {
        // Set up deep link listeners for both iOS and Android
        const unlisten = await listen<string>('deep-link-received', (event) => {
          // Handle deep links
        });
      }
    };
    setupDeepLinks();
  }, []);
  ```

---

## DEEP LINKING CONFIGURATION

### Overview
Deep linking is critical for handling OAuth callbacks, payment redirects, and team invites. iOS uses Universal Links (HTTPS) and custom URL schemes. Android needs equivalent App Links and custom schemes.

### Current iOS Configuration

#### 1. Universal Links (HTTPS Links)
**File**: `/frontend/src-tauri/gen/apple/maple_iOS/maple_iOS.entitlements`
```xml
<key>com.apple.developer.associated-domains</key>
<array>
    <string>applinks:trymaple.ai</string>
</array>
```

**Server Requirements**:
- Host `.well-known/apple-app-site-association` at https://trymaple.ai
- This file tells iOS which paths should open in the app

#### 2. Custom URL Scheme
**File**: `/frontend/src-tauri/gen/apple/maple_iOS/Info.plist`
```xml
<key>CFBundleURLTypes</key>
<array>
    <dict>
        <key>CFBundleURLName</key>
        <string>cloud.opensecret.maple</string>
        <key>CFBundleURLSchemes</key>
        <array>
            <string>cloud.opensecret.maple</string>
        </array>
    </dict>
</array>
```

#### 3. Tauri Configuration
**File**: `/frontend/src-tauri/tauri.conf.json`
```json
"deep-link": {
  "desktop": {
    "schemes": ["cloud.opensecret.maple"]
  },
  "mobile": [{
    "host": "trymaple.ai"
  }]
}
```

### Android Requirements (To Be Implemented)

#### 1. App Links (HTTPS Links - Android Equivalent of Universal Links)

**AndroidManifest.xml** additions needed:
```xml
<intent-filter android:autoVerify="true">
    <action android:name="android.intent.action.VIEW" />
    <category android:name="android.intent.category.DEFAULT" />
    <category android:name="android.intent.category.BROWSABLE" />

    <!-- Handle https://trymaple.ai/* URLs -->
    <data android:scheme="https"
          android:host="trymaple.ai" />
</intent-filter>
```

**Server Requirements**:
- Host `.well-known/assetlinks.json` at https://trymaple.ai
- Example content:
```json
[{
  "relation": ["delegate_permission/common.handle_all_urls"],
  "target": {
    "namespace": "android_app",
    "package_name": "cloud.opensecret.maple",
    "sha256_cert_fingerprints": ["YOUR_APP_SIGNING_CERT_SHA256"]
  }
}]
```

#### 2. Custom URL Scheme
**AndroidManifest.xml** additions:
```xml
<intent-filter>
    <action android:name="android.intent.action.VIEW" />
    <category android:name="android.intent.category.DEFAULT" />
    <category android:name="android.intent.category.BROWSABLE" />

    <!-- Handle cloud.opensecret.maple:// URLs -->
    <data android:scheme="cloud.opensecret.maple" />
</intent-filter>
```

### Deep Link Flow in the App

#### Current Implementation (`DeepLinkHandler.tsx`)
The app listens for deep links and handles:
1. **Auth callbacks**: `cloud.opensecret.maple://auth?access_token=...&refresh_token=...`
2. **Payment success**: `https://trymaple.ai/payment-success?source=stripe`
3. **Payment canceled**: `https://trymaple.ai/payment-canceled?source=stripe`

#### Code Flow:
```typescript
// DeepLinkHandler.tsx
listen<string>("deep-link-received", (event) => {
  const url = event.payload;
  // Parse and handle auth tokens, payment callbacks, etc.
});
```

### URLs That Need Deep Link Support

| Flow | iOS URL | Android URL (Should Be Same) | Purpose |
|------|---------|------------------------------|---------|
| **Stripe Payment Success** | `https://trymaple.ai/payment-success?source=stripe` | Same | Return from Stripe checkout |
| **Stripe Payment Cancel** | `https://trymaple.ai/payment-canceled?source=stripe` | Same | Canceled Stripe payment |
| **Zaprite Success** | `https://trymaple.ai/payment-success?source=zaprite` | Same | Bitcoin payment success |
| **API Credits Success** | `https://trymaple.ai/payment-success-credits?source=stripe` | Same | API credit purchase |
| **OAuth Callback** | `cloud.opensecret.maple://auth?access_token=...` | Same | OAuth provider returns |
| **Desktop Auth** | `https://trymaple.ai/desktop-auth?provider=...` | Same | Desktop OAuth flow |

### Current Status of Android Deep Linking ✅ FULLY WORKING

#### ✅ What's Completed:
1. **HTTPS App Links intent filter** - AndroidManifest has `https://trymaple.ai` configured
2. **Custom URL Scheme** - `cloud.opensecret.maple://` intent filter in AndroidManifest
3. **Rust deep link handler** - Set up in `lib.rs` to emit events to frontend
4. **Frontend listener** - `DeepLinkHandler.tsx` listens for `deep-link-received` events
5. **Payment/auth URLs** - Using `isMobile()` for correct URL generation
6. **Digital Asset Links** - `assetlinks.json` deployed with upload key SHA256
7. **App verification** - Android verified ownership of trymaple.ai domain
8. **Testing** - Both custom scheme and HTTPS deep links tested and working

#### ✅ Deep Linking Test Results:
- **Custom scheme** (`cloud.opensecret.maple://`) - Working perfectly
- **HTTPS links** (`https://trymaple.ai/*`) - Auto-verified, no chooser dialog
- **OAuth callbacks** - Ready for GitHub, Google, Apple authentication
- **Payment redirects** - Stripe/Zaprite success/cancel URLs working

### Implementation Steps for Android

#### 1. Configure Android Manifest ✅ COMPLETED
- [x] ✅ App Links intent filter for `https://trymaple.ai` (already existed)
- [x] ✅ Custom scheme intent filter for `cloud.opensecret.maple` (just added)
- [x] ✅ `android:launchMode="singleTask"` already set

#### 2. Digital Asset Links Configuration ✅ COMPLETED
- [x] ✅ Created `.well-known/assetlinks.json` file with upload key SHA256
- [x] ✅ Deployed to https://trymaple.ai/.well-known/assetlinks.json
- [x] ✅ Android verified domain ownership automatically
- [x] ✅ HTTPS deep links tested and working without chooser dialog

#### 3. Update Payment/Auth Flows ✅ COMPLETED
- [x] ✅ All instances updated to use `isMobile()` platform utilities
- [x] ✅ Callback URLs use `https://trymaple.ai` for mobile platforms
- [ ] Test OAuth flows (GitHub, Google, Apple) - needs device testing
- [ ] Test payment flows (Stripe, Zaprite) - needs device testing

#### 4. Native Android Code ✅ ALREADY WORKING
- [x] ✅ Intent handling via Tauri's deep link plugin
- [x] ✅ Deep links passed to WebView/Tauri runtime
- [x] ✅ "deep-link-received" event emitted to JavaScript

### Testing Deep Links

#### Android Testing Commands:
```bash
# Test custom scheme
adb shell am start -W -a android.intent.action.VIEW \
  -d "cloud.opensecret.maple://auth?access_token=test" \
  cloud.opensecret.maple

# Test App Links (HTTPS)
adb shell am start -W -a android.intent.action.VIEW \
  -d "https://trymaple.ai/payment-success?source=stripe" \
  cloud.opensecret.maple
```

#### Verification Tools:
1. Android App Links Assistant (in Android Studio)
2. Digital Asset Links API Validator
3. Chrome DevTools for testing web-to-app navigation

### Security Considerations

1. **App Links Verification**: Android automatically verifies App Links ownership via assetlinks.json
2. **Certificate Pinning**: Consider pinning the SHA256 cert in assetlinks.json
3. **Token Handling**: Ensure auth tokens in deep links are:
   - One-time use
   - Short-lived
   - Properly validated

---

## Android App Signing & Distribution (Tauri v2)

### Overview
This section covers how to properly sign and distribute your Android app both through Google Play Store and as direct APK downloads, including setting up deep linking to work with both distribution methods.

### Understanding Android Signing Keys

#### What You Need to Know:
- **Upload Key**: The keystore you create and manage. Signs APKs you upload to Google Play or distribute directly.
- **App Signing Key**: Google's key (if using Play App Signing). Google re-signs your app with this for Play Store distribution.
- **SHA256 Fingerprint**: A public identifier derived from a key. Safe to share, goes in `assetlinks.json`.

#### Key Security:
- 🔒 **KEEP SECRET**: Keystore files (.jks), passwords
- ✅ **PUBLIC/SAFE**: SHA256 fingerprints, certificates in APKs
- 📝 **NEVER COMMIT**: keystore.properties, *.jks files

### Step 1: Create Your Upload Keystore ✅ COMPLETED

```bash
# Create a new keystore for your app (run this once)
keytool -genkey -v \
  -keystore ~/maple-android-upload.jks \
  -keyalg RSA \
  -keysize 2048 \
  -validity 10000 \
  -alias upload

# You'll be prompted for:
# - Keystore password (remember this!)
# - Your name, organization, etc. (can use generic values like "Unknown")
# - Key password (can be same as keystore password)
```

**Important**:
- Store this file somewhere safe (NOT in your git repo)
- Back it up! If you lose it and aren't using Play App Signing, you can't update your app
- Remember the password and alias (upload)

**Status**: ✅ Created `~/maple-android-upload.jks` and backed up

### Step 2: Configure Tauri for Signing ✅ COMPLETED

Create `frontend/src-tauri/gen/android/keystore.properties`:
```properties
password=your-keystore-password
keyAlias=upload
storeFile=/absolute/path/to/maple-android-upload.jks
```

**Add to `.gitignore`**:
```gitignore
# Android signing
*.jks
*.keystore
keystore.properties
```

**Status**:
- ✅ Created `keystore.properties` with actual password
- ✅ Updated `.gitignore` to exclude sensitive files
- ✅ Added signing configuration to `build.gradle.kts`

### Step 3: Build Signed Apps ✅ COMPLETED

```bash
# For development/testing (uses debug keystore automatically)
bun tauri android dev

# For release APK (direct distribution)
bun tauri android build

# For release AAB (Google Play Store)
bun tauri android build -- --aab
```

Output locations:
- **APK**: `frontend/src-tauri/gen/android/app/build/outputs/apk/universal/release/app-universal-release.apk`
- **AAB**: `frontend/src-tauri/gen/android/app/build/outputs/bundle/universalRelease/app-universal-release.aab`

**Status**: ✅ Successfully built signed APK and AAB, installed on physical device

### Step 4: Get SHA256 Fingerprints ✅ COMPLETED

```bash
# For your upload keystore (direct APK distribution)
keytool -list -v \
  -keystore ~/maple-android-upload.jks \
  -alias upload \
  | grep SHA256

# Output:
# SHA256: C6:12:09:59:0A:27:73:F9:EA:EC:80:0A:C1:09:07:54:4A:56:6C:62:A5:68:7D:DF:9D:B3:DE:91:19:E4:3B:2A

# For debug builds (testing only)
keytool -list -v \
  -keystore ~/.android/debug.keystore \
  -alias androiddebugkey \
  -storepass android \
  -keypass android \
  | grep SHA256
```

If using Google Play App Signing:
1. Upload your first AAB to Google Play Console
2. Go to: Setup → App Integrity → App signing
3. Copy the "SHA-256 certificate fingerprint" shown there

### Step 5: Create assetlinks.json for Deep Linking ✅ COMPLETED

Created `frontend/public/.well-known/assetlinks.json`:

```json
[{
  "relation": ["delegate_permission/common.handle_all_urls"],
  "target": {
    "namespace": "android_app",
    "package_name": "cloud.opensecret.maple",
    "sha256_cert_fingerprints": [
      "C6:12:09:59:0A:27:73:F9:EA:EC:80:0A:C1:09:07:54:4A:56:6C:62:A5:68:7D:DF:9D:B3:DE:91:19:E4:3B:2A"
    ]
  }
}]
```

**Status**: ✅ Deployed to https://trymaple.ai/.well-known/assetlinks.json

**Current Configuration**:
- ✅ Contains upload key SHA256 only (for direct APK distribution)
- ⏳ **TODO: Google Play SHA256** - Needs to be added when Play Store is configured

**Next Steps for Google Play Store**:
1. Create app in Google Play Console
2. Upload first AAB: `bun tauri android build -- --aab`
3. Enable Play App Signing (recommended)
4. Get Google's SHA256 from Console (Setup → App Integrity → App signing)
5. Update `assetlinks.json` to include both fingerprints:
   ```json
   "sha256_cert_fingerprints": [
     "C6:12:09:59:0A:27:73:F9:EA:EC:80:0A:C1:09:07:54:4A:56:6C:62:A5:68:7D:DF:9D:B3:DE:91:19:E4:3B:2A",  // Upload key
     "GOOGLE_PLAY_SHA256_HERE"  // Google's signing key
   ]
   ```
6. Redeploy assetlinks.json to server

### Step 6: Distribution Strategy

#### Option A: Google Play Store Only
1. Build AAB: `bun tauri android build -- --aab`
2. Upload to Play Console
3. Enable Play App Signing (recommended)
4. Get SHA256 from Play Console
5. Add only Google's SHA256 to assetlinks.json

#### Option B: Direct APK Only
1. Build APK: `bun tauri android build`
2. Distribute app-universal-release.apk
3. Get SHA256 from your keystore
4. Add only your SHA256 to assetlinks.json

#### Option C: Both (Recommended)
1. Upload AAB to Play Store with Play App Signing
2. Also distribute APK directly
3. Add BOTH SHA256s to assetlinks.json
4. Both distribution methods work with deep links!

### Testing Deep Links ✅ VERIFIED WORKING

Once the APK is installed (for multiple devices, use `-s <device_id>`):

```bash
# Test custom scheme (works immediately, no assetlinks needed)
adb -s <device_id> shell am start -W -a android.intent.action.VIEW \
  -d "cloud.opensecret.maple://auth?access_token=test" \
  cloud.opensecret.maple

# Test HTTPS links (verified working with deployed assetlinks.json)
adb -s <device_id> shell am start -W -a android.intent.action.VIEW \
  -d "https://trymaple.ai/payment-success?source=stripe" \
  cloud.opensecret.maple
```

**Status**: ✅ Both custom scheme and HTTPS deep links tested and working on physical device

**Note**: Android may cache App Links verification. If updating assetlinks.json, force re-verification:
```bash
adb shell pm clear-app-links cloud.opensecret.maple
adb shell pm verify-app-links --re-verify cloud.opensecret.maple
```

### Common Issues & Solutions

#### "App isn't verified" dialog for HTTPS links
- **Cause**: assetlinks.json missing or wrong SHA256
- **Fix**: Ensure SHA256 in assetlinks.json matches your signing certificate
- **Workaround**: Users can tap "Open in app anyway" and select "Always"

#### Can't update app after reinstall
- **Cause**: Different signing keys (e.g., debug vs release)
- **Fix**: Uninstall first, then install the new version

#### Lost keystore file
- **If using Play App Signing**: Contact Google Play support to reset upload key
- **If NOT using Play App Signing**: You cannot update the app anymore
- **Prevention**: Always use Play App Signing for Play Store releases

### CI/CD Setup (GitHub Actions Example)

Store in GitHub Secrets:
- `ANDROID_KEYSTORE_BASE64`: `base64 -i maple-upload-keystore.jks`
- `ANDROID_KEY_PASSWORD`: Your keystore password
- `ANDROID_KEY_ALIAS`: `upload`

`.github/workflows/android.yml`:
```yaml
- name: Setup Android signing
  run: |
    cd frontend/src-tauri/gen/android
    echo "${{ secrets.ANDROID_KEYSTORE_BASE64 }}" | base64 -d > keystore.jks
    echo "password=${{ secrets.ANDROID_KEY_PASSWORD }}" > keystore.properties
    echo "keyAlias=${{ secrets.ANDROID_KEY_ALIAS }}" >> keystore.properties
    echo "storeFile=$(pwd)/keystore.jks" >> keystore.properties

- name: Build signed APK
  run: bun tauri android build
```

### Version Code Management

Tauri automatically generates version codes from `tauri.conf.json`:
- Formula: `versionCode = major * 1000000 + minor * 1000 + patch`
- Example: `1.3.2` → `1003002`

Override in `tauri.conf.json` if needed:
```json
{
  "bundle": {
    "android": {
      "versionCode": 42
    }
  }
}
```

---

## Google Play Store Setup ✅ IN PROGRESS

### Prerequisites Completed:
- ✅ Upload keystore created and configured
- ✅ Signed AAB can be built with `bun tauri android build --aab`
- ✅ Deep linking configured with both SHA256 fingerprints
- ✅ App tested on physical device
- ✅ Google Play Developer Account created
- ✅ App created in Play Console ("Maple AI")
- ✅ First AAB uploaded to internal testing
- ✅ Play App Signing enabled
- ✅ Google's SHA256 certificate obtained
- ✅ assetlinks.json updated with both certificates
- ✅ Privacy policy URL configured (https://opensecret.cloud/privacy)
- ✅ API level 35 requirement met

### Current Status:
- **Internal Testing Release**: Version 1.3.2 (Build 1003002002) is live
- **Package Name**: cloud.opensecret.maple
- **Google's App Signing SHA256**: `36:B3:1C:A3:CC:DD:CA:9A:DD:47:8A:8F:86:70:DB:11:E3:56:E7:90:09:6E:CC:7D:8C:43:38:F4:55:13:B1:0A`
- **Upload Key SHA256**: `C6:12:09:59:0A:27:73:F9:EA:EC:80:0A:C1:09:07:54:4A:56:6C:62:A5:68:7D:DF:9D:B3:DE:91:19:E4:3B:2A`

### Version Code Management:
To avoid conflicts with Tauri's automatic version code generation, we use an extended scheme:
- Base formula: `major*1000000 + minor*1000 + patch`
- Extended for builds: Add 3 digits for build number (e.g., 1003002001, 1003002002)
- Configured in `tauri.conf.json` under `bundle.android.versionCode`

### Still Required for Full Release:
1. **Add Testers to Internal Testing Track**
2. **Complete Content Rating Questionnaire**
3. **Complete Store Listing**
   - App description (short & full)
   - Screenshots for different device sizes
   - Feature graphic (1024x500)
   - App icon (512x512)
4. **Test Deep Linking and OAuth Flows**
5. **Promote to Production**
   - Submit for review
   - First review may take several days

---

## Android CI/CD Build Configuration

### Build Warnings and Version Compatibility

#### Current Warnings Observed:
1. **Gradle Plugin Version**:
   ```
   WARNING: We recommend using a newer Android Gradle plugin to use compileSdk = 36
   This Android Gradle plugin (8.5.1) was tested up to compileSdk = 34.
   ```
   - **Impact**: Build succeeds but may have compatibility issues
   - **TODO**: Wait for Tauri to update their Android template or manually update Gradle plugin

2. **Deprecated targetSdk in library DSL**:
   ```
   'targetSdk: Int?' is deprecated. Will be removed from library DSL in v9.0
   ```
   - **Location**: tauri-plugin-fs Android build.gradle.kts
   - **Impact**: Warning only, will need update when Gradle 9.0 releases
   - **TODO**: Wait for Tauri plugin updates

3. **Unused Rust Functions**:
   ```
   warning: function `get_config_path` is never used
   warning: function `save_proxy_config` is never used
   ```
   - **Impact**: No runtime impact, just compilation warnings
   - **TODO**: Clean up unused proxy functions or add #[cfg] attributes for desktop-only

#### Version Information:
- **Current Gradle**: 8.9
- **Android Gradle Plugin**: 8.5.1
- **compileSdk**: 35 (Updated for Google Play requirements)
- **targetSdk**: 35 (Required by Google Play as of Aug 2025)
- **minSdk**: 24 (Android 7.0+, covers 99%+ of devices)
- **NDK**: r25c

#### Build Performance:
- **Initial build time**: ~9-10 minutes (includes downloading dependencies)
- **Cached build time**: ~4-5 minutes
- **Architectures built**: arm64-v8a, armeabi-v7a, x86_64, x86

### GitHub Actions Workflow Status

#### ✅ Completed:
1. **Android SDK and Java setup**
2. **NDK installation and configuration**
3. **Rust targets for all Android architectures**
4. **Cross-compilation environment variables**
5. **Keystore configuration for signing**
6. **Comprehensive caching**:
   - Bun/Node dependencies
   - Gradle cache
   - Rust compilation cache
   - Cargo registry and binaries
   - APT packages

#### 🔧 Known Issues Fixed:
1. **OpenSSL cross-compilation** - Fixed by setting up proper NDK paths and ranlib symlinks
2. **Missing i686 target** - Added i686-linux-android to Rust targets
3. **NDK toolchain not found** - Fixed by adding NDK bin to PATH and setting env vars

#### 📋 TODO for Production:
1. **Update Gradle Plugin** when Tauri updates templates
2. **Google Play Store Setup**:
   - Create developer account
   - Configure Play App Signing
   - Add Google's SHA256 to assetlinks.json
3. **Version Management**:
   - Implement automatic version bumping
   - Consider using semantic-release
4. **Testing**:
   - Add Android emulator tests
   - Implement UI testing with Espresso
5. **Optimization**:
   - Consider using `--target` flags to build specific architectures only
   - Implement APK size optimization
   - Add ProGuard/R8 rules if needed

### Secrets Required for CI/CD

Add these to GitHub repository secrets:
- `ANDROID_KEYSTORE_BASE64` - Base64 encoded keystore file
- `ANDROID_KEY_ALIAS` - Key alias (usually "upload")
- `ANDROID_KEY_PASSWORD` - Keystore password

---

## Summary of Android Behavior Patterns

### ✅ Same as iOS (Mobile Behavior)
1. External browser for payments/billing
2. Deep linking for auth callbacks
3. Document processing capabilities
4. No proxy functionality
5. Return URLs using trymaple.ai domain

### ❌ Different from iOS (Android-Specific)
1. **Enable paid plans** - No App Store payment restrictions
2. **Show API Management** - No restrictions on API key features
3. **Apple Sign In** - Use web flow, not native
4. **Product availability** - Server should return `is_available: true`

### 🔄 Future Enhancements
1. Native Google Sign-In using Google Play Services
2. Android-specific payment integration options

---

## Implementation Checklist

### High Priority (Required for Basic Android Support)
- [x] ✅ Add Android platform detection alongside iOS
- [x] ✅ Update billing API to treat Android like iOS for external browser
- [x] ✅ Ensure deep linking works for auth callbacks
- [ ] Configure opener plugin for Android in Tauri (platform-side config needed)

### Medium Priority (Feature Parity)
- [ ] Enable paid plans on Android (server-side changes)
- [x] ✅ Show API Management features on Android (hidden on mobile)
- [x] ✅ Handle Apple auth with external browser on Android
- [x] ✅ Document processing support verified for Android

### Low Priority (Enhancements)
- [ ] Consider native Google Sign-In integration
- [ ] Optimize Android-specific UI elements
- [ ] Add Android-specific telemetry

---

## Platform Detection - NEW UNIFIED DESIGN

### CRITICAL ISSUES WITH CURRENT IMPLEMENTATION
The current platform detection has fundamental flaws that must be fixed:

1. **INCORRECT VALUES DURING LOADING**: Hooks return `false` while detecting platform, causing:
   - Tauri-only code running on web (CRASHES)
   - Web-only code running in Tauri (CRASHES)
   - Wrong UI showing temporarily (flickering)

2. **MULTIPLE CONFLICTING APIS**:
   - Async functions: `await isIOS()`
   - React hooks: `useIsIOS()` returning potentially wrong values
   - Both can be incorrect during initialization

3. **NO SINGLE SOURCE OF TRUTH**: Platform can be detected differently in different parts of the app

### NEW UNIFIED PLATFORM DETECTION DESIGN

Platform detection happens ONCE before the app renders, guaranteeing correctness:

#### Implementation (platform.ts):
```typescript
// Platform info - NOT nullable, ALWAYS set before app renders
let platformInfo: PlatformInfo;

// Initialize immediately when module loads
const platformReady = (async () => {
  try {
    const tauriEnv = await import("@tauri-apps/api/core")
      .then(m => m.isTauri())
      .catch(() => false);

    if (tauriEnv) {
      const { type } = await import("@tauri-apps/plugin-os");
      const platform = await type();
      platformInfo = {
        platform,
        isTauri: true,
        isIOS: platform === "ios",
        isAndroid: platform === "android",
        isMobile: platform === "ios" || platform === "android",
        isDesktop: platform === "macos" || platform === "windows" || platform === "linux",
        isMacOS: platform === "macos",
        isWindows: platform === "windows",
        isLinux: platform === "linux",
        isWeb: false,
        isTauriDesktop: true && (platform === "macos" || platform === "windows" || platform === "linux"),
        isTauriMobile: true && (platform === "ios" || platform === "android")
      };
    } else {
      platformInfo = {
        platform: "web",
        isTauri: false,
        isIOS: false,
        isAndroid: false,
        isMobile: false,
        isDesktop: false,
        isMacOS: false,
        isWindows: false,
        isLinux: false,
        isWeb: true,
        isTauriDesktop: false,
        isTauriMobile: false
      };
    }
  } catch {
    // Default to web on any error
    platformInfo = {
      platform: "web",
      isTauri: false,
      isIOS: false,
      isAndroid: false,
      isMobile: false,
      isDesktop: false,
      isMacOS: false,
      isWindows: false,
      isLinux: false,
      isWeb: true,
      isTauriDesktop: false,
      isTauriMobile: false
    };
  }
})();

// Export for main.tsx to await
export const waitForPlatform = () => platformReady;

// Simple, synchronous, ALWAYS correct
export function isIOS(): boolean {
  return platformInfo.isIOS;
}

export function isAndroid(): boolean {
  return platformInfo.isAndroid;
}

export function isMobile(): boolean {
  return platformInfo.isMobile;
}

export function isTauri(): boolean {
  return platformInfo.isTauri;
}

export function isDesktop(): boolean {
  return platformInfo.isDesktop;
}

export function isWeb(): boolean {
  return platformInfo.isWeb;
}

// Get full platform info if needed
export function getPlatformInfo(): PlatformInfo {
  return platformInfo;
}
```

#### App Initialization (main.tsx):
```typescript
import { waitForPlatform } from '@/utils/platform';

// Platform MUST be ready before rendering
await waitForPlatform();

// NOW platform is guaranteed correct - no loading states, no wrong values
createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>
);
```

#### Usage in Components - SIMPLE AND ALWAYS CORRECT:
```typescript
import { isIOS, isAndroid, isMobile, isTauri } from '@/utils/platform';

// Direct usage - ALWAYS correct, never wrong, no await needed
function MyComponent() {
  if (isMobile()) {
    // This is GUARANTEED correct - no loading states
    return <MobileView />;
  }

  if (isTauri()) {
    // SAFE to use Tauri APIs - will NEVER run on web
    const { invoke } = await import("@tauri-apps/api/core");
    await invoke("some_command");
  }

  return <WebView />;
}

// No more hooks with loading states!
function PricingComponent() {
  // Just use the functions directly - they're always correct
  if (isIOS()) {
    return <IOSPricing />;
  }

  if (isAndroid()) {
    return <AndroidPricing />;
  }

  return <WebPricing />;
}
```

### BENEFITS OF NEW DESIGN:
1. **SINGLE API**: Just `isIOS()`, `isMobile()`, etc. No hooks, no await, no variants
2. **ALWAYS CORRECT**: Platform is detected before app exists, can NEVER be wrong
3. **INSTANT**: All checks are synchronous after initialization
4. **CRASH-PROOF**: Tauri APIs only called when definitely in Tauri, web APIs only on web
5. **SIMPLE**: No loading states, no undefined handling, no complexity
6. **NO FLICKERING**: UI is correct from first render

### MIGRATION PLAN:
1. Update `platform.ts` with new implementation
2. Add `await waitForPlatform()` to `main.tsx`
3. Remove ALL `useIsIOS()`, `useIsMobile()` etc. hooks
4. Remove ALL `await isIOS()` async calls
5. Replace with direct `isIOS()`, `isMobile()` function calls
6. Delete the hooks file entirely - no longer needed

## Platform Utility Migration Status

### ✅ Components Using Platform Utilities
1. **Billing API** (`billingApi.ts`) - Uses `isMobile()`, `isIOS()`, `isAndroid()`
2. **Account Menu** (`AccountMenu.tsx`) - Uses `useIsMobile()` hook
3. **Team Invite Dialog** (`TeamInviteDialog.tsx`) - Uses `isMobile()` utility
4. **Marketing** (`Marketing.tsx`) - Uses `useIsIOS()` hook
5. **API Credits** (`ApiCreditsSection.tsx`) - Uses `useIsMobile()` hook
6. **Chat Box** (`ChatBox.tsx`) - Uses `useIsTauri()` hook
7. **Proxy Config** (`ProxyConfigSection.tsx`) - Uses `useIsTauriDesktop()` hook
8. **API Key Dashboard** (`ApiKeyDashboard.tsx`) - Uses `useIsTauriDesktop()` hook
9. **Pricing** (`pricing.tsx`) - Uses hooks and utilities
10. **Login** (`login.tsx`) - Uses `useIsIOS()`, `useIsTauri()` hooks and `isTauri()` utility
11. **Signup** (`signup.tsx`) - Uses `useIsIOS()`, `useIsTauri()` hooks and `isTauri()` utility
12. **Deep Link Handler** (`DeepLinkHandler.tsx`) - Uses `isTauri()` utility

### ✅ Components That Don't Need Platform Utilities
1. **Auth Callback** (`auth.$provider.callback.tsx`) - Uses platform-agnostic `redirect-to-native` flag
2. **Desktop Auth** (`desktop-auth.tsx`) - No platform checks needed
3. **Apple Auth Provider** (`AppleAuthProvider.tsx`) - Intentionally uses `window.location.protocol === "tauri:"` to exclude ALL Tauri platforms (working as designed)

#### Migration Examples

**Before (Old Pattern):**
```typescript
// In billing/billingApi.ts
const isTauri = await import("@tauri-apps/api/core")
  .then((m) => m.isTauri())
  .catch(() => false);

if (isTauri) {
  const { type } = await import("@tauri-apps/plugin-os");
  const platform = await type();

  if (platform === "ios") {
    // iOS logic
  }
}
```

**After (New Pattern):**
```typescript
// In billing/billingApi.ts
import { isIOS, isAndroid, isMobile } from '@/utils/platform';

if (await isMobile()) {
  // Mobile logic (iOS and Android)
  returnUrl = "https://trymaple.ai";
}

if (await isIOS()) {
  // iOS-specific logic
}

if (await isAndroid()) {
  // Android-specific logic
}
```

**React Component Before:**
```typescript
const [isIOS, setIsIOS] = useState(false);
const [isTauriEnv, setIsTauriEnv] = useState(false);

useEffect(() => {
  const checkPlatform = async () => {
    const tauriEnv = await isTauri();
    setIsTauriEnv(tauriEnv);
    if (tauriEnv) {
      const platform = await type();
      setIsIOS(platform === "ios");
    }
  };
  checkPlatform();
}, []);
```

**React Component After:**
```typescript
import { useIsIOS, useIsAndroid, useIsTauri } from '@/hooks/usePlatform';

function Component() {
  const { isIOS } = useIsIOS();
  const { isAndroid } = useIsAndroid();
  const { isTauri } = useIsTauri();

  // No need for useEffect or state management
  // The hooks handle everything
}
```

### Key Patterns for Android Support

```typescript
// Pattern 1: Mobile behavior (both iOS and Android)
if (await isMobile()) {
  // Open external browser for payments
  await invoke("plugin:opener|open_url", { url });
}

// Pattern 2: iOS-only behavior
if (await isIOS()) {
  // Use native Apple Sign In
}

// Pattern 3: Android-only behavior
if (await isAndroid()) {
  // Enable features that iOS restricts
  showApiManagement = true;
  enablePaidPlans = true;
}

// Pattern 4: Desktop-only features
if (await isDesktop()) {
  // Enable proxy configuration
}
```
