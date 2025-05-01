// Type definitions for native iOS Apple Sign In plugin

// For native Apple Sign In plugin
export interface AppleCredential {
  user: string;
  identityToken: string;
  email?: string;
  fullName?: {
    givenName?: string;
    familyName?: string;
  };
  state?: string;
}
