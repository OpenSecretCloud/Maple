// Type definitions for Apple Sign In plugin and OAuth

// For native Apple Sign In plugin
interface AppleCredential {
  user: string;
  identityToken: string;
  email?: string;
  fullName?: {
    givenName?: string;
    familyName?: string;
  };
  state?: string;
}

// For Apple OAuth options
interface AppleOAuthOptions {
  response_mode?: "query" | "form_post";
}