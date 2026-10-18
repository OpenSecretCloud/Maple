import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import {
  CHAT_BOTTOM_ATTACH_THRESHOLD_PX,
  ChatScrollFollow,
  chatDistanceFromBottom,
  isChatBackwardScrollKey,
  listenForChatScrollIntent
} from "./chatScrollFollow";

const viewport = 600;
const at = (scrollTop: number, scrollHeight = 2000, clientHeight = viewport) => ({
  scrollTop,
  scrollHeight,
  clientHeight
});
const bottomOf = (scrollHeight = 2000, clientHeight = viewport) =>
  at(scrollHeight - clientHeight, scrollHeight, clientHeight);

function followingAtBottom() {
  const follow = new ChatScrollFollow();
  follow.handleScroll(bottomOf());
  expect(follow.isFollowing).toBe(true);
  return follow;
}

describe("chatDistanceFromBottom", () => {
  test("measures the remaining scroll range and never goes negative", () => {
    expect(chatDistanceFromBottom(at(1000))).toBe(400);
    expect(chatDistanceFromBottom(bottomOf())).toBe(0);
    expect(chatDistanceFromBottom(at(1500))).toBe(0);
  });
});

describe("ChatScrollFollow", () => {
  test("follows by default", () => {
    expect(new ChatScrollFollow().isFollowing).toBe(true);
  });

  test("a single small upward wheel event detaches immediately", () => {
    const follow = followingAtBottom();
    follow.handleWheel(-1);
    expect(follow.isFollowing).toBe(false);
  });

  test("downward and zero wheel deltas keep following", () => {
    const follow = followingAtBottom();
    follow.handleWheel(0);
    follow.handleWheel(40);
    expect(follow.isFollowing).toBe(true);
  });

  test("dragging a finger down (scrolling up) detaches; dragging up does not", () => {
    const follow = followingAtBottom();
    follow.handleTouchMove(-12);
    expect(follow.isFollowing).toBe(true);
    follow.handleTouchMove(3);
    expect(follow.isFollowing).toBe(false);
  });

  test("backward navigation keys detach", () => {
    for (const event of [
      { key: "ArrowUp", shiftKey: false },
      { key: "PageUp", shiftKey: false },
      { key: "Home", shiftKey: false },
      { key: " ", shiftKey: true }
    ]) {
      const follow = followingAtBottom();
      follow.handleKey(event);
      expect(follow.isFollowing).toBe(false);
    }

    const follow = followingAtBottom();
    follow.handleKey({ key: "ArrowDown", shiftKey: false });
    follow.handleKey({ key: " ", shiftKey: false });
    expect(follow.isFollowing).toBe(true);
  });

  test("a small upward scroll detaches, even within the old 100 px zone", () => {
    const follow = followingAtBottom();
    follow.handleScroll(at(1400 - 2));
    expect(follow.isFollowing).toBe(false);
  });

  test("a scrollbar drag upward (scroll event only, no wheel) detaches", () => {
    const follow = followingAtBottom();
    follow.handleScroll(at(1300));
    expect(follow.isFollowing).toBe(false);
  });

  test("stays detached while scrolling down short of the bottom", () => {
    const follow = followingAtBottom();
    follow.handleWheel(-30);
    follow.handleScroll(at(1370));
    follow.handleScroll(at(1390));
    follow.handleScroll(at(1400 - CHAT_BOTTOM_ATTACH_THRESHOLD_PX - 1));
    expect(follow.isFollowing).toBe(false);
  });

  test("re-attaches when the user returns to the bottom", () => {
    const follow = followingAtBottom();
    follow.handleWheel(-30);
    follow.handleScroll(at(1370));
    follow.handleScroll(at(1400 - CHAT_BOTTOM_ATTACH_THRESHOLD_PX));
    expect(follow.isFollowing).toBe(true);
  });

  test("a tiny upward move that stays inside the bottom zone does not re-attach", () => {
    const follow = followingAtBottom();
    follow.handleWheel(-2);
    follow.handleScroll(at(1398));
    expect(follow.isFollowing).toBe(false);
  });

  test("re-attaches at the bottom after content grew while detached", () => {
    const follow = followingAtBottom();
    follow.handleScroll(at(1200));
    expect(follow.isFollowing).toBe(false);
    // Streaming added 500 px below; the user scrolls all the way down.
    follow.handleScroll(at(1600, 2500));
    expect(follow.isFollowing).toBe(false);
    follow.handleScroll(bottomOf(2500));
    expect(follow.isFollowing).toBe(true);
  });

  test("scrollTop clamped by shrinking content does not count as user input", () => {
    const follow = followingAtBottom();
    // A thinking block collapses: the browser clamps scrollTop to the new maximum.
    follow.handleScroll(bottomOf(1700));
    expect(follow.isFollowing).toBe(true);
  });

  test("a growing viewport (composer shrinks) that clamps scrollTop keeps following", () => {
    const follow = followingAtBottom();
    follow.handleScroll(bottomOf(2000, 680));
    expect(follow.isFollowing).toBe(true);
  });

  test("following writes keep it attached as content grows", () => {
    const follow = followingAtBottom();
    for (let height = 2040; height <= 2400; height += 40) {
      follow.attach(bottomOf(height));
      follow.handleScroll(bottomOf(height));
      expect(follow.isFollowing).toBe(true);
    }
  });

  test("sending a message re-attaches without reaching the bottom first", () => {
    const follow = followingAtBottom();
    follow.handleScroll(at(500));
    expect(follow.isFollowing).toBe(false);
    follow.attach();
    // The send scroll animates downward toward the new turn.
    follow.handleScroll(at(900));
    expect(follow.isFollowing).toBe(true);
  });

  test("an explicit detach with metrics holds until the user reaches the bottom", () => {
    const follow = new ChatScrollFollow();
    follow.detach(at(800));
    follow.handleScroll(at(800, 2100));
    expect(follow.isFollowing).toBe(false);
    follow.handleScroll(bottomOf(2100));
    expect(follow.isFollowing).toBe(true);
  });

  test("a pinned frame's scroll event cannot re-attach after an upward wheel", () => {
    const follow = followingAtBottom();
    // Streaming grows the content and the follow write pins to the new bottom.
    follow.attach(bottomOf(2040));
    follow.handleWheel(-2);
    follow.handleScroll(at(1438, 2040));
    expect(follow.isFollowing).toBe(false);
  });

  test("a stale baseline cannot turn an upward wheel into a re-attach", () => {
    const follow = followingAtBottom();
    // Content grew to 2040 without a baseline update, then the user wheeled up.
    follow.handleWheel(-2);
    follow.handleScroll(at(1438, 2040));
    expect(follow.isFollowing).toBe(false);
  });

  test("a queued scroll event with unchanged metrics keeps an upward wheel detached", () => {
    const follow = followingAtBottom();
    follow.handleWheel(-3);
    follow.handleScroll(bottomOf());
    expect(follow.isFollowing).toBe(false);
    follow.handleScroll(at(1397));
    expect(follow.isFollowing).toBe(false);
    // A real return to the bottom re-attaches.
    follow.handleWheel(3);
    follow.handleScroll(bottomOf());
    expect(follow.isFollowing).toBe(true);
  });

  test("an overscroll bounce at the bottom keeps following", () => {
    const follow = followingAtBottom();
    follow.handleScroll(at(1420));
    follow.handleScroll(at(1400));
    expect(follow.isFollowing).toBe(true);
  });

  test("returning to the bottom with a bounce re-attaches and stays attached", () => {
    const follow = followingAtBottom();
    follow.handleWheel(-40);
    follow.handleScroll(at(1300));
    follow.handleWheel(60);
    follow.handleScroll(at(1425));
    follow.handleScroll(at(1400));
    expect(follow.isFollowing).toBe(true);
  });

  test("an overscroll bounce at the top is not a downward return", () => {
    const follow = new ChatScrollFollow();
    follow.detach(at(0));
    follow.handleScroll(at(-30));
    follow.handleScroll(at(0));
    expect(follow.isFollowing).toBe(false);
  });

  test("the first scroll event decides from position alone", () => {
    const away = new ChatScrollFollow();
    away.handleScroll(at(100));
    expect(away.isFollowing).toBe(false);

    const bottom = new ChatScrollFollow();
    bottom.handleScroll(bottomOf());
    expect(bottom.isFollowing).toBe(true);
  });
});

describe("isChatBackwardScrollKey", () => {
  test("matches only backward navigation keys", () => {
    expect(isChatBackwardScrollKey({ key: "ArrowUp", shiftKey: false })).toBe(true);
    expect(isChatBackwardScrollKey({ key: "Spacebar", shiftKey: true })).toBe(true);
    expect(isChatBackwardScrollKey({ key: "End", shiftKey: false })).toBe(false);
    expect(isChatBackwardScrollKey({ key: "a", shiftKey: true })).toBe(false);
  });
});

describe("listenForChatScrollIntent", () => {
  class FakeElement extends EventTarget {
    constructor(private readonly matches: boolean) {
      super();
    }
    closest() {
      return this.matches ? this : null;
    }
  }

  const globals = globalThis as unknown as { window?: unknown; Element?: unknown };
  let originalWindow: unknown;
  let originalElement: unknown;
  let fakeWindow: EventTarget;
  let container: EventTarget;

  beforeEach(() => {
    originalWindow = globals.window;
    originalElement = globals.Element;
    fakeWindow = new EventTarget();
    container = new EventTarget();
    globals.window = fakeWindow;
    globals.Element = FakeElement;
  });

  afterEach(() => {
    globals.window = originalWindow;
    globals.Element = originalElement;
  });

  const event = (type: string, fields: Record<string, unknown>) =>
    Object.assign(new Event(type), fields);
  const keyFrom = (target: EventTarget, key: string) => {
    const keyEvent = event("keydown", { key, shiftKey: false });
    Object.defineProperty(keyEvent, "target", { value: target });
    return keyEvent;
  };
  const listen = () => {
    const follow = new ChatScrollFollow();
    const stop = listenForChatScrollIntent(container as HTMLElement, follow);
    return { follow, stop };
  };

  test("an upward wheel detaches; a pinch-zoom wheel does not", () => {
    const { follow } = listen();
    container.dispatchEvent(event("wheel", { deltaY: -3, ctrlKey: true }));
    expect(follow.isFollowing).toBe(true);
    container.dispatchEvent(event("wheel", { deltaY: -3, ctrlKey: false }));
    expect(follow.isFollowing).toBe(false);
  });

  test("dragging a finger down detaches", () => {
    const { follow } = listen();
    container.dispatchEvent(event("touchstart", { touches: [{ clientY: 300 }] }));
    container.dispatchEvent(event("touchmove", { touches: [{ clientY: 290 }] }));
    expect(follow.isFollowing).toBe(true);
    container.dispatchEvent(event("touchmove", { touches: [{ clientY: 296 }] }));
    expect(follow.isFollowing).toBe(false);
  });

  test("backward keys detach unless typed into a text field or control", () => {
    const { follow } = listen();
    fakeWindow.dispatchEvent(keyFrom(new FakeElement(true), "ArrowUp"));
    expect(follow.isFollowing).toBe(true);
    fakeWindow.dispatchEvent(keyFrom(new FakeElement(false), "PageUp"));
    expect(follow.isFollowing).toBe(false);
  });

  test("cleanup removes every listener", () => {
    const { follow, stop } = listen();
    stop();
    container.dispatchEvent(event("wheel", { deltaY: -40, ctrlKey: false }));
    container.dispatchEvent(event("touchstart", { touches: [{ clientY: 300 }] }));
    container.dispatchEvent(event("touchmove", { touches: [{ clientY: 340 }] }));
    fakeWindow.dispatchEvent(keyFrom(new FakeElement(false), "Home"));
    expect(follow.isFollowing).toBe(true);
  });
});
