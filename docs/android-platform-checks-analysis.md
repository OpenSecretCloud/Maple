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

## 2. PROXY SERVICE (`frontend/src/services/proxyService.ts`)

### Instance 1: Desktop Platform Check (Lines 99-106)
- **Current Behavior:** Returns true only for `macos`, `windows`, `linux`
- **Android Recommendation:** ❌ **Different from iOS** - Keep as desktop-only
- **Reasoning:** Proxy functionality not needed on mobile platforms
- **No changes needed** - Current logic correctly excludes mobile platforms

---

## 3. AUTHENTICATION (`frontend/src/routes/login.tsx` & `signup.tsx`)

### Instance 1: Platform Detection (Lines 58-59)
- **Current iOS Behavior:** Sets `isIOS` state when `platform === "ios"`
- **Android Recommendation:** ➕ **Add Android detection**
- **Implementation:** Use React hooks instead of manual state management
  ```typescript
  import { useIsIOS, useIsAndroid, useIsTauri } from '@/utils/platform';

  function LoginComponent() {
    const { isIOS } = useIsIOS();
    const { isAndroid } = useIsAndroid();
    const { isTauri } = useIsTauri();
    // No need for useEffect or manual state management
  }
  ```

### Instance 2: GitHub OAuth (Lines 127-160)
- **Current Behavior:** External browser for all Tauri platforms
- **Android Recommendation:** ✅ **Already supports Android** - Comments mention "iOS/Android"

### Instance 3: Google OAuth (Lines 162-195)
- **Current Behavior:** External browser for all Tauri platforms
- **Android Recommendation:** 🔄 **Consider enhancement** - Could use native Google Sign-In
- **Future Enhancement:** Android has native Google Play Services integration option

### Instance 4: Apple Sign In (Lines 197-302)
- **Current iOS Behavior:** Uses native `plugin:sign-in-with-apple|get_apple_id_credential`
- **Android Recommendation:** ❌ **Different from iOS** - Use external browser
- **Reasoning:** No native Apple Sign In SDK for Android

---

## 4. PRICING PAGE (`frontend/src/routes/pricing.tsx`)

### Instance 1: Platform Detection (Lines 202-203)
- **Current iOS Behavior:** Sets `isIOS` state
- **Android Recommendation:** ➕ **Add Android detection**
- **Implementation:**
  ```typescript
  import { useIsIOS, useIsAndroid } from '@/utils/platform';

  const { isIOS } = useIsIOS();
  const { isAndroid } = useIsAndroid();
  ```

### Instance 2: Bitcoin Toggle Auto-Enable (Line 228)
- **Current iOS Behavior:** Prevents auto-enabling Bitcoin payments
- **Android Recommendation:** ✅ **Same as iOS** - Prevent auto-enable
- **Implementation:**
  ```typescript
  import { useIsMobile } from '@/utils/platform';
  const { isMobile } = useIsMobile();

  // Disable auto-enable for mobile platforms
  if (!isMobile && zaprite_enabled) {
    setBitcoinToggle(true);
  }
  ```

### Instance 3: Product Availability (Lines 301-304, 461-463)
- **Current iOS Behavior:** Shows "Not available in app" for paid plans when `is_available === false`
- **Android Recommendation:** ❌ **Different from iOS** - Android can support paid plans
- **Implementation:**
  ```typescript
  // Server should distinguish between platforms
  // iOS: product.is_available = false for paid plans
  // Android: product.is_available = true for paid plans
  if (isIOS && !isFreeplan && !product.is_available) {
    return "Not available in app";
  }
  // Android will allow paid plans
  ```

### Instance 4: Payment Success URLs (Lines 394-409)
- **Current iOS Behavior:** Uses Universal Links `https://trymaple.ai/payment-success`
- **Android Recommendation:** ✅ **Same as iOS** - Use deep links for callbacks
- **Implementation:**
  ```typescript
  import { isMobile } from '@/utils/platform';

  const successUrl = await isMobile()
    ? "https://trymaple.ai/payment-success"
    : `${window.location.origin}/payment-success`;
  ```

### Instance 5: Portal Opening (Lines 507-525)
- **Current iOS Behavior:** Uses `plugin:opener|open_url` to launch Safari
- **Android Recommendation:** ✅ **Same as iOS** - Use external browser
- **Implementation:**
  ```typescript
  import { isMobile } from '@/utils/platform';

  if (await isMobile()) {
    await invoke("plugin:opener|open_url", { url: portalUrl });
  } else {
    window.open(portalUrl, "_blank");
  }
  ```

---

## 5. ACCOUNT MENU (`frontend/src/components/AccountMenu.tsx`)

### Instance 1: API Management Visibility (Lines 276-281)
- **Current iOS Behavior:** Hides API Management menu item
- **Android Recommendation:** ❌ **Different from iOS** - Show API Management
- **Reasoning:** No App Store restrictions on Android for API key features
- **Implementation:**
  ```typescript
  import { useIsIOS } from '@/utils/platform';

  const { isIOS } = useIsIOS();

  // Only hide for iOS, show for Android and other platforms
  {!isIOS && (
    <MenuItem onClick={handleApiManagement}>API Management</MenuItem>
  )}
  ```

### Instance 2: Manage Subscription (Lines 155-178)
- **Current iOS Behavior:** Uses external browser via opener plugin
- **Android Recommendation:** ✅ **Same as iOS** - Use external browser
- **Implementation:**
  ```typescript
  import { isMobile } from '@/utils/platform';

  if (await isMobile()) {
    await invoke("plugin:opener|open_url", { url: portalUrl });
  } else {
    window.location.href = portalUrl;
  }
  ```

---

## 6. TEAM INVITES (`frontend/src/components/team/TeamInviteDialog.tsx`)

### Instance 1: Billing Portal (Lines 53-71)
- **Current iOS Behavior:** Uses `plugin:opener|open_url` for external browser
- **Android Recommendation:** ✅ **Same as iOS** - Use external browser
- **Implementation:**
  ```typescript
  import { isMobile } from '@/utils/platform';

  if (await isMobile()) {
    await invoke("plugin:opener|open_url", { url });
  }
  ```

---

## 7. MARKETING (`frontend/src/components/Marketing.tsx`)

### Instance 1: Pricing Tier Buttons (Lines 141-152)
- **Current iOS Behavior:** Shows "Coming Soon" for paid plans
- **Android Recommendation:** ❌ **Different from iOS** - Enable paid plans
- **Reasoning:** Android doesn't have App Store payment restrictions
- **Implementation:**
  ```typescript
  import { useIsIOS } from '@/utils/platform';

  const { isIOS } = useIsIOS();

  // Only show "Coming Soon" for iOS
  const buttonText = isIOS && !isFreeplan ? "Coming Soon" : "Get Started";
  ```

---

## 8. API CREDITS (`frontend/src/components/apikeys/ApiCreditsSection.tsx`)

### Instance 1: Payment URLs (Lines 106-130)
- **Current iOS Behavior:** Uses Universal Links for payment callbacks
- **Android Recommendation:** ✅ **Same as iOS** - Use deep links
- **Implementation:**
  ```typescript
  import { isMobile } from '@/utils/platform';

  const successUrl = await isMobile()
    ? "https://trymaple.ai/payment-success-credits"
    : `${window.location.origin}/payment-success`;
  ```

### Instance 2: Feature Availability
- **Current iOS Behavior:** Feature disabled (comments indicate)
- **Android Recommendation:** ❌ **Different from iOS** - Enable feature
- **Reasoning:** No restrictions on Android
- **Implementation:**
  ```typescript
  import { useIsIOS } from '@/utils/platform';

  const { isIOS } = useIsIOS();

  // Enable for Android, disable only for iOS
  const isFeatureEnabled = !isIOS;
  ```

---

## 9. CHAT BOX (`frontend/src/components/ChatBox.tsx`)

### Instance 1: Document Processing (Line 243)
- **Current Behavior:** Tauri environments support PDF processing
- **Android Recommendation:** ✅ **Same as iOS** - Support local document processing
- **Implementation:**
  ```typescript
  import { useIsTauri } from '@/utils/platform';

  const { isTauri } = useIsTauri();

  // All Tauri platforms (desktop and mobile) support document processing
  if (isTauri) {
    // Enable PDF processing
  }
  ```

---

## 10. PROXY CONFIGURATION (`frontend/src/components/apikeys/ProxyConfigSection.tsx`)

### Instance 1: Component Visibility (Lines 28-40)
- **Current Behavior:** Only shows on desktop platforms
- **Android Recommendation:** ✅ **Same as iOS** - Hide proxy config
- **Reasoning:** Proxy not needed on mobile
- **Implementation:**
  ```typescript
  import { useIsTauriDesktop } from '@/utils/platform';

  const { isTauriDesktop } = useIsTauriDesktop();

  if (!isTauriDesktop) return null;
  ```

---

## 11. API KEY DASHBOARD (`frontend/src/components/apikeys/ApiKeyDashboard.tsx`)

### Instance 1: Proxy Tab Visibility (Lines 215-230)
- **Current Behavior:** Shows proxy tab only on desktop
- **Android Recommendation:** ✅ **Same as iOS** - Hide proxy tab
- **Implementation:**
  ```typescript
  import { useIsTauriDesktop } from '@/utils/platform';

  const { isTauriDesktop } = useIsTauriDesktop();

  // Only show proxy tab on desktop
  const tabs = isTauriDesktop
    ? ['credits', 'keys', 'proxy']
    : ['credits', 'keys'];
  ```

---

## 12. AUTH CALLBACK (`frontend/src/routes/auth.$provider.callback.tsx`)

### Instance 1: Native App Redirect (Lines 38-59)
- **Current Behavior:** Checks `redirect-to-native` flag for deep linking
- **Android Recommendation:** ✅ **Same as iOS** - Use native app flow
- **Implementation:**
  ```typescript
  // Works for both iOS and Android
  const isTauriAuth = localStorage.getItem("redirect-to-native") === "true";

  if (isTauriAuth) {
    // Deep link back to app with auth tokens
    window.location.href = `maple://auth-callback?${params}`;
  }
  ```

---

## 13. APPLE AUTH PROVIDER (`frontend/src/components/AppleAuthProvider.tsx`)

### Instance 1: Component Rendering (Lines 392-395)
- **Current Behavior:** Returns null in Tauri (expects native iOS)
- **Android Recommendation:** ❌ **Different from iOS** - Show web SDK button
- **Reasoning:** No native Apple auth on Android
- **Implementation:**
  ```typescript
  import { useIsIOS, useIsTauri } from '@/utils/platform';

  const { isIOS } = useIsIOS();
  const { isTauri } = useIsTauri();

  // Only hide for iOS Tauri app (which has native auth)
  // Show for Android Tauri and web
  if (isTauri && isIOS) return null;

  // Show Apple web auth button for Android and web
  return <AppleAuthButton />;
  ```

---

## 14. DEEP LINK HANDLER (`frontend/src/components/DeepLinkHandler.tsx`)

### Instance 1: Deep Link Events (Lines 31-84)
- **Current Behavior:** Sets up listeners in Tauri environments
- **Android Recommendation:** ✅ **Same as iOS** - Handle deep links
- **Implementation:**
  ```typescript
  import { isTauri } from '@/utils/platform';

  useEffect(() => {
    const setupDeepLinks = async () => {
      if (await isTauri()) {
        // Set up deep link listeners for both iOS and Android
        const unsubscribe = await onOpenUrl((urls) => {
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

### Current Issues with Android Deep Linking

1. **Problem**: When returning from Stripe, the app goes to `http://tauri.localhost/pricing?canceled=true` instead of the app
2. **Cause**: The success/cancel URLs are using `window.location.origin` which resolves to `tauri://localhost` on Android
3. **Solution**: Update `pricing.tsx` to use `isMobile()` instead of just `isTauriIOS` (already documented in Section 4)

### Implementation Steps for Android

#### 1. Configure Android Manifest (Priority: HIGH)
- [ ] Add App Links intent filter for `https://trymaple.ai`
- [ ] Add custom scheme intent filter for `cloud.opensecret.maple`
- [ ] Set `android:launchMode="singleTask"` to prevent multiple instances

#### 2. Server Configuration (Priority: HIGH)
- [ ] Create `.well-known/assetlinks.json` file
- [ ] Host it at https://trymaple.ai/.well-known/assetlinks.json
- [ ] Include app signing certificate SHA256 fingerprint
- [ ] Test with Android's Statement List Generator and Tester

#### 3. Update Payment/Auth Flows (Priority: HIGH)
- [ ] Update all instances checking `isTauriIOS` to use `isMobile()`
- [ ] Ensure all callback URLs use `https://trymaple.ai` for mobile
- [ ] Test OAuth flows (GitHub, Google, Apple)
- [ ] Test payment flows (Stripe, Zaprite)

#### 4. Native Android Code (Priority: MEDIUM)
- [ ] Implement intent handling in MainActivity
- [ ] Pass deep link URLs to the WebView/Tauri runtime
- [ ] Ensure "deep-link-received" event is emitted to JavaScript

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
- [ ] Add Android platform detection alongside iOS
- [ ] Update billing API to treat Android like iOS for external browser
- [ ] Ensure deep linking works for auth callbacks
- [ ] Configure opener plugin for Android in Tauri

### Medium Priority (Feature Parity)
- [ ] Enable paid plans on Android (server-side changes)
- [ ] Show API Management features on Android
- [ ] Handle Apple auth with web SDK on Android
- [ ] Test document processing on Android

### Low Priority (Enhancements)
- [ ] Consider native Google Sign-In integration
- [ ] Optimize Android-specific UI elements
- [ ] Add Android-specific telemetry

---

## Code Patterns to Use

### Platform Detection Utilities Available
We now have comprehensive platform detection utilities at:
- `src/utils/platform.ts` - Core async utility functions
- `src/hooks/usePlatform.ts` - React hooks for components
- `src/utils/platform/index.ts` - Unified exports

### Usage Examples

#### In Async Functions (e.g., billing, auth handlers)
```typescript
import { isIOS, isAndroid, isMobile, isDesktop, getPlatformInfo } from '@/utils/platform';

// Simple checks
if (await isMobile()) {
  // Logic for both iOS and Android
  console.log("Mobile platform detected");
}

if (await isIOS()) {
  // iOS-specific logic
  await invoke("plugin:sign-in-with-apple|get_apple_id_credential");
}

if (await isAndroid()) {
  // Android-specific logic
  // Could implement native Google Sign-In here
}

// Get full platform info
const platform = await getPlatformInfo();
if (platform.isTauri && platform.isMobile) {
  // Mobile app-specific logic
}
```

#### In React Components
```typescript
import { useIsIOS, useIsAndroid, useIsMobile, usePlatform } from '@/utils/platform';

function PricingComponent() {
  const { isIOS } = useIsIOS();
  const { isAndroid } = useIsAndroid();
  const { isMobile } = useIsMobile();

  // Simple mobile check
  if (isMobile) {
    return <MobileLayout />;
  }

  // Platform-specific rendering
  if (isIOS) {
    return <IOSSpecificView />;
  }

  if (isAndroid) {
    return <AndroidView />;
  }

  return <DesktopView />;
}

// Using full platform info
function ComplexComponent() {
  const { platform, loading } = usePlatform();

  if (loading) return <Spinner />;

  if (platform.isTauriDesktop) {
    // Show desktop-only features like proxy
  }

  if (platform.isTauriMobile) {
    // Mobile app features
  }
}
```

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
import { useIsIOS, useIsAndroid, useIsTauri } from '@/utils/platform';

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