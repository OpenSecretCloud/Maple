import { afterEach, describe, expect, mock, test } from "bun:test";
import { act, create, type ReactTestInstance, type ReactTestRenderer } from "react-test-renderer";

import { GlobalNotification, type Notification } from "./GlobalNotification";

function textContent(node: ReactTestInstance): string {
  return node.children
    .map((child) => (typeof child === "string" ? child : textContent(child)))
    .join("");
}

const originalSetTimeout = globalThis.setTimeout;
const originalClearTimeout = globalThis.clearTimeout;

function installFakeTimers() {
  let nextTimerId = 1;
  const timers = new Map<number, () => void>();

  globalThis.setTimeout = ((handler: TimerHandler) => {
    const timerId = nextTimerId++;
    if (typeof handler === "function") timers.set(timerId, handler as () => void);
    return timerId;
  }) as unknown as typeof setTimeout;
  globalThis.clearTimeout = ((timerId: number) => {
    timers.delete(timerId);
  }) as unknown as typeof clearTimeout;

  return {
    runAll() {
      for (const [timerId, handler] of [...timers]) {
        timers.delete(timerId);
        handler();
      }
    }
  };
}

function installReadyNotification(): Notification {
  return {
    id: "install-ready",
    type: "update",
    title: "Update Ready",
    message: "Version 9.8.7 is ready.",
    duration: 0,
    actions: [
      {
        label: "Install Now",
        onClick: () => {}
      }
    ]
  };
}

describe("GlobalNotification", () => {
  let renderer: ReactTestRenderer | null = null;

  afterEach(() => {
    if (renderer) act(() => renderer?.unmount());
    renderer = null;
    globalThis.setTimeout = originalSetTimeout;
    globalThis.clearTimeout = originalClearTimeout;
  });

  for (const replacement of [
    { id: "installed", type: "update" as const, title: "Update Installed" },
    { id: "failed", type: "error" as const, title: "Update Not Installed" }
  ]) {
    test(`does not dismiss a fast ${replacement.type} result from an older install action`, () => {
      const timers = installFakeTimers();
      const onDismiss = mock(() => {});

      act(() => {
        renderer = create(
          <GlobalNotification notification={installReadyNotification()} onDismiss={onDismiss} />
        );
      });

      const installButton = renderer!.root
        .findAllByType("button")
        .find((button) => textContent(button) === "Install Now");
      act(() => installButton!.props.onClick());

      act(() => {
        renderer!.update(
          <GlobalNotification
            notification={{
              ...replacement,
              message: "Native updater result.",
              duration: 0
            }}
            onDismiss={onDismiss}
          />
        );
      });
      act(() => timers.runAll());

      expect(textContent(renderer!.root)).toContain(replacement.title);
      expect(onDismiss).not.toHaveBeenCalled();
    });
  }
});
