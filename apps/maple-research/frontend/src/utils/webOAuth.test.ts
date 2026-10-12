import { afterEach, describe, expect, test } from "bun:test";
import { webOAuthCallbackUrl } from "./webOAuth";

const originalWindow = Object.getOwnPropertyDescriptor(globalThis, "window");
afterEach(() => {
  if (originalWindow) Object.defineProperty(globalThis, "window", originalWindow);
  else Reflect.deleteProperty(globalThis, "window");
});

function setLocation(url: string) {
  Object.defineProperty(globalThis, "window", {
    configurable: true,
    value: { location: new URL(url) }
  });
}

describe("browser OAuth callback selection", () => {
  for (const origin of [
    "https://trymaple.ai",
    "https://app.maple.ai",
    "https://app-dev.trymaple.ai",
    "https://app-dev.maple.ai",
    "http://localhost:5173"
  ]) {
    for (const provider of ["github", "google", "apple"] as const) {
      test(`${provider} returns to the initiating origin ${origin}`, () => {
        setLocation(`${origin}/signup?next=%2Fsettings`);
        expect(webOAuthCallbackUrl(provider)).toBe(`${origin}/auth/${provider}/callback`);
      });
    }
  }

  test("cannot turn a native custom-scheme origin into a provider callback", () => {
    setLocation("tauri://localhost/login");
    expect(() => webOAuthCallbackUrl("github")).toThrow("Browser OAuth requires an HTTP origin");
  });
});
