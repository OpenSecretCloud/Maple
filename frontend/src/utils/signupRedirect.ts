type AuthenticatedSignupRedirectState = {
  isAuthenticated: boolean;
  isGuestSignup: boolean;
  isSignupRoute: boolean;
  showGuestCredentials: boolean;
};

export function shouldRedirectAuthenticatedSignup({
  isAuthenticated,
  isGuestSignup,
  isSignupRoute,
  showGuestCredentials
}: AuthenticatedSignupRedirectState): boolean {
  return isSignupRoute && isAuthenticated && !isGuestSignup && !showGuestCredentials;
}
