import { describe, expect, test } from "bun:test";
import {
  initializeNativeIOSCompactViewport,
  MOBILE_VIEWPORT_QUERY,
  NATIVE_IOS_COMPACT_VIEWPORT_CLASS,
  SHORT_LANDSCAPE_VIEWPORT_QUERY
} from "./nativeIOSViewport";

class FakeMediaQueryList {
  private listeners = new Set<() => void>();

  constructor(public matches: boolean) {}

  addEventListener(type: "change", listener: () => void) {
    if (type === "change") this.listeners.add(listener);
  }

  removeEventListener(type: "change", listener: () => void) {
    if (type === "change") this.listeners.delete(listener);
  }

  setMatches(matches: boolean) {
    this.matches = matches;
    for (const listener of this.listeners) listener();
  }

  get listenerCount() {
    return this.listeners.size;
  }
}

function createEnvironment({
  content = "width=device-width, initial-scale=1",
  compactWidth = false,
  shortLandscape = false
}: {
  content?: string | null;
  compactWidth?: boolean;
  shortLandscape?: boolean;
} = {}) {
  let viewportContent = content;
  const classes = new Set<string>();
  const widthQuery = new FakeMediaQueryList(compactWidth);
  const landscapeQuery = new FakeMediaQueryList(shortLandscape);
  const queriedMedia = new Map([
    [MOBILE_VIEWPORT_QUERY, widthQuery],
    [SHORT_LANDSCAPE_VIEWPORT_QUERY, landscapeQuery]
  ]);
  const meta = {
    getAttribute: () => viewportContent,
    setAttribute: (name: "content", value: string) => {
      if (name !== "content") throw new Error(`Unexpected attribute: ${name}`);
      viewportContent = value;
    },
    removeAttribute: () => {
      viewportContent = null;
    }
  };
  const document = {
    documentElement: {
      classList: {
        add: (token: string) => classes.add(token),
        remove: (token: string) => classes.delete(token)
      }
    },
    querySelector: () => meta
  };

  return {
    classes,
    document,
    landscapeQuery,
    matchMedia: (query: string) => {
      const result = queriedMedia.get(query);
      if (!result) throw new Error(`Unexpected media query: ${query}`);
      return result;
    },
    viewportContent: () => viewportContent,
    widthQuery
  };
}

describe("native iOS compact viewport", () => {
  test("enables full-bleed layout at mobile widths and restores the exact original", () => {
    const environment = createEnvironment({ compactWidth: true });
    const cleanup = initializeNativeIOSCompactViewport(true, environment);

    expect(environment.viewportContent()).toBe(
      "width=device-width, initial-scale=1, viewport-fit=cover"
    );
    expect(environment.classes.has(NATIVE_IOS_COMPACT_VIEWPORT_CLASS)).toBe(true);

    cleanup();

    expect(environment.viewportContent()).toBe("width=device-width, initial-scale=1");
    expect(environment.classes.has(NATIVE_IOS_COMPACT_VIEWPORT_CLASS)).toBe(false);
    expect(environment.widthQuery.listenerCount).toBe(0);
    expect(environment.landscapeQuery.listenerCount).toBe(0);
  });

  test("uses short landscape as an alternative compact layout signal", () => {
    const environment = createEnvironment({ shortLandscape: true });
    const cleanup = initializeNativeIOSCompactViewport(true, environment);

    expect(environment.viewportContent()).toContain("viewport-fit=cover");

    environment.landscapeQuery.setMatches(false);
    expect(environment.viewportContent()).toBe("width=device-width, initial-scale=1");
    expect(environment.classes.has(NATIVE_IOS_COMPACT_VIEWPORT_CLASS)).toBe(false);

    environment.widthQuery.setMatches(true);
    expect(environment.viewportContent()).toContain("viewport-fit=cover");
    expect(environment.classes.has(NATIVE_IOS_COMPACT_VIEWPORT_CLASS)).toBe(true);

    cleanup();
  });

  test("does not initialize or change metadata outside native iOS", () => {
    const environment = createEnvironment({ compactWidth: true });
    const cleanup = initializeNativeIOSCompactViewport(false, environment);

    expect(environment.viewportContent()).toBe("width=device-width, initial-scale=1");
    expect(environment.classes.has(NATIVE_IOS_COMPACT_VIEWPORT_CLASS)).toBe(false);
    expect(environment.widthQuery.listenerCount).toBe(0);
    expect(environment.landscapeQuery.listenerCount).toBe(0);

    cleanup();
  });

  test("is idempotent and replaces an existing viewport-fit directive", () => {
    const environment = createEnvironment({
      compactWidth: true,
      content: "width=device-width, viewport-fit=contain"
    });
    const firstCleanup = initializeNativeIOSCompactViewport(true, environment);
    const secondCleanup = initializeNativeIOSCompactViewport(true, environment);

    expect(environment.viewportContent()).toBe("width=device-width, viewport-fit=cover");
    expect(environment.viewportContent()?.match(/viewport-fit=/g)).toHaveLength(1);
    expect(environment.widthQuery.listenerCount).toBe(1);
    expect(environment.landscapeQuery.listenerCount).toBe(1);

    firstCleanup();
    expect(environment.viewportContent()).toBe("width=device-width, viewport-fit=cover");

    secondCleanup();
    expect(environment.viewportContent()).toBe("width=device-width, viewport-fit=contain");
  });

  test("restores a missing original content attribute", () => {
    const environment = createEnvironment({ compactWidth: true, content: null });
    const cleanup = initializeNativeIOSCompactViewport(true, environment);

    expect(environment.viewportContent()).toBe("viewport-fit=cover");

    cleanup();
    expect(environment.viewportContent()).toBeNull();
  });
});
