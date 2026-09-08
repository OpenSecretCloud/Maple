import { describe, expect, it } from "bun:test";
import { getSafeInternalRedirect, navigateToSafeInternalRedirect } from "./internalRedirect";

describe("getSafeInternalRedirect", () => {
  it("preserves an internal path, query, and hash", () => {
    expect(getSafeInternalRedirect("/settings/api?credits_success=true#balance")).toBe(
      "/settings/api?credits_success=true#balance"
    );
  });

  it("rejects absolute and protocol-relative URLs", () => {
    expect(getSafeInternalRedirect("https://evil.example/settings")).toBeUndefined();
    expect(getSafeInternalRedirect("//evil.example/settings")).toBeUndefined();
  });

  it("rejects backslash paths that browsers can normalize to another origin", () => {
    expect(getSafeInternalRedirect("/\\evil.example/settings")).toBeUndefined();
  });

  it("rejects paths that normalize to a protocol-relative URL", () => {
    expect(getSafeInternalRedirect("/a/..//evil.example/settings")).toBeUndefined();
  });

  it("rejects missing and non-string values", () => {
    expect(getSafeInternalRedirect(undefined)).toBeUndefined();
    expect(getSafeInternalRedirect(42)).toBeUndefined();
  });
});

describe("navigateToSafeInternalRedirect", () => {
  it("preserves route search and hash through the router history", () => {
    let pushedHref: string | undefined;
    const history = {
      push: (href: string) => {
        pushedHref = href;
      },
      replace: () => {}
    };

    expect(
      navigateToSafeInternalRedirect(history, "/settings/api?credits_success=true#balance")
    ).toBe(true);
    expect(pushedHref).toBe("/settings/api?credits_success=true#balance");
  });

  it("rejects unsafe redirects without navigating", () => {
    let navigated = false;
    const history = {
      push: () => {
        navigated = true;
      },
      replace: () => {
        navigated = true;
      }
    };

    expect(navigateToSafeInternalRedirect(history, "https://evil.example/settings")).toBe(false);
    expect(navigated).toBe(false);
  });
});
