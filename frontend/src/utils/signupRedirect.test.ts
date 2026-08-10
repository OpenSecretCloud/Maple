import { describe, expect, test } from "bun:test";
import { shouldRedirectAuthenticatedSignup } from "./signupRedirect";

describe("shouldRedirectAuthenticatedSignup", () => {
  test("does not redirect from the anonymous signup handoff", () => {
    expect(
      shouldRedirectAuthenticatedSignup({
        isAuthenticated: true,
        isGuestSignup: true,
        isSignupRoute: true,
        showGuestCredentials: false
      })
    ).toBe(false);
  });

  test("does not redirect while anonymous credentials are visible", () => {
    expect(
      shouldRedirectAuthenticatedSignup({
        isAuthenticated: true,
        isGuestSignup: true,
        isSignupRoute: true,
        showGuestCredentials: true
      })
    ).toBe(false);
  });

  test("still redirects an authenticated user outside the guest handoff", () => {
    expect(
      shouldRedirectAuthenticatedSignup({
        isAuthenticated: true,
        isGuestSignup: false,
        isSignupRoute: true,
        showGuestCredentials: false
      })
    ).toBe(true);
  });

  test("does not redirect a transient signup outlet while navigation leaves the route", () => {
    expect(
      shouldRedirectAuthenticatedSignup({
        isAuthenticated: true,
        isGuestSignup: false,
        isSignupRoute: false,
        showGuestCredentials: false
      })
    ).toBe(false);
  });
});
