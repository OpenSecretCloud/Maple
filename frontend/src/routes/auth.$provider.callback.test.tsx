import { afterEach, beforeEach, describe, expect, mock, test } from "bun:test";
import { act, create, type ReactTestRenderer } from "react-test-renderer";

/**
 * The OAuth callback route has two ways to end: the deep-link hand-off back to the
 * Tauri app, and an error. These tests cover the error end, which is the one a
 * returning password-account user hits when they press "Sign in with Google" and
 * OpenSecret answers 409 `UserExistsNotLinked`.
 */

class MemoryStorage implements Storage {
  private readonly values = new Map<string, string>();
  get length(): number {
    return this.values.size;
  }
  clear(): void {
    this.values.clear();
  }
  getItem(key: string): string | null {
    return this.values.get(key) ?? null;
  }
  key(index: number): string | null {
    return [...this.values.keys()][index] ?? null;
  }
  removeItem(key: string): void {
    this.values.delete(key);
  }
  setItem(key: string, value: string): void {
    this.values.set(key, value);
  }
}

let capturedComponent: (() => JSX.Element) | null = null;
let provider = "google";
let handleGoogleCallback = mock(async () => {});
const navigate = mock(() => {});

mock.module("@tanstack/react-router", () => ({
  // Capture the route's component so the test can render it without a router.
  createFileRoute: () => (options: { component: () => JSX.Element }) => {
    capturedComponent = options.component;
    return { useParams: () => ({ provider }), useSearch: () => ({}) };
  },
  useNavigate: () => navigate,
  useRouter: () => ({ history: {} }),
  Link: ({ children }: { children?: unknown }) => children as JSX.Element
}));

mock.module("@opensecret/react", () => ({
  useOpenSecret: () => ({
    handleGitHubCallback: mock(async () => {}),
    handleGoogleCallback,
    handleAppleCallback: mock(async () => {})
  })
}));

mock.module("@/billing/billingService", () => ({
  getBillingService: () => ({ clearToken: mock(() => {}) })
}));

// Import once, at module scope: the route registers its component the first time
// it is evaluated, and a later import returns the cached module without re-running.
await import("./auth.$provider.callback");

const originalLocalStorage = Object.getOwnPropertyDescriptor(globalThis, "localStorage");
const originalSessionStorage = Object.getOwnPropertyDescriptor(globalThis, "sessionStorage");
const originalWindow = Object.getOwnPropertyDescriptor(globalThis, "window");

function setGlobal(name: string, value: unknown): void {
  Object.defineProperty(globalThis, name, { configurable: true, value, writable: true });
}
function restore(name: string, d: PropertyDescriptor | undefined): void {
  if (d) Object.defineProperty(globalThis, name, d);
  else Reflect.deleteProperty(globalThis, name);
}

/** Flatten the rendered tree to text so assertions read like what a user sees. */
function textOf(renderer: ReactTestRenderer): string {
  const walk = (node: unknown): string => {
    if (node === null || node === undefined || typeof node === "boolean") return "";
    if (typeof node === "string" || typeof node === "number") return String(node);
    if (Array.isArray(node)) return node.map(walk).join(" ");
    const children = (node as { children?: unknown }).children;
    return children ? walk(children) : "";
  };
  return walk(renderer.toJSON()).replace(/\s+/g, " ").trim();
}

describe("OAuth callback route", () => {
  let localStorage: MemoryStorage;
  let sessionStorage: MemoryStorage;
  let renderer: ReactTestRenderer | null;
  let originalConsoleError: typeof console.error;

  beforeEach(() => {
    renderer = null;
    provider = "google";
    navigate.mockClear();
    originalConsoleError = console.error;
    console.error = mock(() => {});

    localStorage = new MemoryStorage();
    sessionStorage = new MemoryStorage();
    setGlobal("localStorage", localStorage);
    setGlobal("sessionStorage", sessionStorage);
    setGlobal("window", {
      localStorage,
      sessionStorage,
      location: {
        href: "https://trymaple.ai/auth/google/callback?code=abc&state=xyz",
        origin: "https://trymaple.ai",
        search: "?code=abc&state=xyz"
      }
    });
  });

  afterEach(() => {
    if (renderer) act(() => renderer!.unmount());
    console.error = originalConsoleError;
    restore("localStorage", originalLocalStorage);
    restore("sessionStorage", originalSessionStorage);
    restore("window", originalWindow);
  });

  async function renderCallback(): Promise<ReactTestRenderer> {
    const Component = capturedComponent!;
    let r!: ReactTestRenderer;
    await act(async () => {
      r = create(<Component />);
    });
    renderer = r;
    return r;
  }

  const ALREADY_REGISTERED =
    "An account with this email already exists. Please sign in using your existing account.";

  test("shows the failure to a desktop user instead of spinning forever", async () => {
    // The desktop flow sets this flag on trymaple.ai before bouncing to Google.
    // Left unhandled, the native branch renders above the error branch and the
    // user watches a spinner that will never resolve.
    localStorage.setItem("redirect-to-native", "true");
    handleGoogleCallback = mock(async () => {
      throw new Error(ALREADY_REGISTERED);
    });

    const r = await renderCallback();
    const text = textOf(r);

    expect(text).not.toContain("Completing authentication");
    expect(text).toContain("This email already has a Maple account");
  });

  test("points a 409 at the right sign-in method rather than at retrying", async () => {
    handleGoogleCallback = mock(async () => {
      throw new Error(ALREADY_REGISTERED);
    });

    const text = textOf(await renderCallback());

    expect(text).toContain("Go to log in");
    expect(text).toContain("Log in with Email");
    // retrying the same provider is exactly what will not work here
    expect(text).not.toContain("Try Again");
  });

  test("keeps the generic failure card for ordinary errors", async () => {
    localStorage.setItem("redirect-to-native", "true");
    handleGoogleCallback = mock(async () => {
      throw new Error("Failed to authenticate with Google. Please try again.");
    });

    const text = textOf(await renderCallback());

    expect(text).toContain("Authentication Failed");
    expect(text).toContain("Try Again");
    expect(text).not.toContain("Completing authentication");
  });

  test("clears the native hand-off flag on failure so later web sign-ins are unaffected", async () => {
    // Left set, the flag makes a subsequent *successful* web sign-in deep-link
    // into the desktop app instead of continuing in the browser.
    localStorage.setItem("redirect-to-native", "true");
    handleGoogleCallback = mock(async () => {
      throw new Error("Failed to authenticate with Google. Please try again.");
    });

    await renderCallback();

    expect(localStorage.getItem("redirect-to-native")).toBeNull();
  });

  test("still surfaces the failure in the plain web flow", async () => {
    handleGoogleCallback = mock(async () => {
      throw new Error(ALREADY_REGISTERED);
    });

    expect(textOf(await renderCallback())).toContain("This email already has a Maple account");
  });
});
