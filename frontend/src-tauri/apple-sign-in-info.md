# Sign in with Apple Integration

## Overview
This document provides a comprehensive guide to the Sign in with Apple integration for Maple, supporting both native iOS authentication and web-based OAuth.

## Integration Types

### 1. Native iOS Authentication
- Uses the iOS native Sign In with Apple dialog
- Implemented via `tauri-plugin-sign-in-with-apple` (version 1.0.0)
- Returns user credentials directly to the app
- Provides access to user identifiers, email, and name (only on first sign-in)

### 2. Web-based OAuth Flow
- Similar to GitHub and Google OAuth flow
- Redirects users to Apple's authentication page
- Supports both web and desktop (non-iOS) platforms
- Handles callback with auth code and state verification

## Configuration

### iOS Native Auth
1. Required entitlements have been added in `maple_iOS.entitlements`
2. The capability is registered in `capabilities/default.json` and `capabilities/mobile-ios.json`
3. Plugin is configured in Cargo.toml and registered in the app

### OAuth Configuration
1. Set up these parameters in your OpenSecret project settings:
   - **Client ID**: Your Apple Services ID (e.g., com.example.web)
   - **Client Secret**: The base64-encoded contents of your Apple private key (p8 file)
   - **Redirect URI**: Configure as `https://api.opensecret.cloud/auth/apple/callback`

## Implementation Details

### Frontend Integration

#### iOS Native Flow
```typescript
// iOS native authentication
const result = await invoke("plugin:sign-in-with-apple|get_apple_id_credential", {
  payload: {
    scope: ["email", "fullName"],
    state: "apple-auth-state",
    options: { debug: true }
  }
});

// Format and send to backend
const appleUser = {
  user_identifier: result.user,
  identity_token: result.identityToken,
  email: result.email,
  given_name: result.fullName?.givenName,
  family_name: result.fullName?.familyName
};

// Call OpenSecret SDK
await os.handleAppleNativeSignIn(appleUser, inviteCode);
```

#### Web OAuth Flow
```typescript
// Web OAuth authentication
const { auth_url } = await os.initiateAppleAuth(inviteCode);
window.location.href = auth_url;

// Callback handling (in separate component)
await handleAppleCallback(code, state, inviteCode);
```

### Platform Detection
The app automatically determines the appropriate flow:
1. Checks if the app is running on iOS and uses native flow
2. Checks if running in a Tauri environment (desktop) and uses the desktop auth flow
3. Uses the web OAuth flow for all other cases

## Callback Handling
For the web OAuth flow, callbacks are handled in `auth.$provider.callback.tsx`:
1. Extracts code and state from URL parameters
2. Verifies auth state to prevent CSRF attacks
3. Processes the authentication with the backend
4. Redirects to appropriate page after successful authentication

## Debugging
If you experience issues with Sign in with Apple:
1. Check debug logs (enabled by default in both flows)
2. Verify that iOS entitlements are properly configured (for native flow)
3. For OAuth flow, check if Apple Developer account is properly set up
4. Verify that the OpenSecret project settings are correctly configured

## Apple Developer Setup
To use Sign in with Apple, you need:
1. An Apple Developer account
2. An App ID with "Sign In with Apple" capability
3. A Services ID for web authentication
4. A Bundle ID for iOS apps
5. A private key for JWT token signing

## Resources
- [Apple Developer Documentation](https://developer.apple.com/documentation/sign_in_with_apple)
- [OpenSecret Apple Auth API](https://docs.opensecret.cloud/docs/guides/authentication)
- [tauri-plugin-sign-in-with-apple](https://crates.io/crates/tauri-plugin-sign-in-with-apple)