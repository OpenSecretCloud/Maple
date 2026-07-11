import { describe, expect, it } from "bun:test";
import {
  MOBILE_NAVIGATION_HISTORY_KEY,
  activeMobilePage,
  createInitialMobileNavigation,
  createMobileHistoryState,
  mobilePageHref,
  pageFromHref,
  promoteNewChatToConversation,
  pushMobilePage,
  readMobileHistoryState,
  type MobileNavigationPage
} from "./mobileNavigation";

describe("mobile navigation URL resolution", () => {
  it("uses the main menu for a root URL", () => {
    expect(createInitialMobileNavigation("/")).toEqual({
      version: 1,
      stack: [{ type: "menu", instanceId: 0 }],
      hasInAppParent: false,
      historyIndex: 0
    });
  });

  it("loads a directly addressed conversation on web", () => {
    expect(createInitialMobileNavigation("/?conversation_id=conv_123")).toEqual({
      version: 1,
      stack: [
        { type: "menu", instanceId: 0 },
        { type: "chat", instanceId: 1, conversationId: "conv_123" }
      ],
      hasInAppParent: false,
      historyIndex: 0
    });
  });

  it("loads a directly addressed project on web", () => {
    expect(pageFromHref("/?project_id=project_123", 7)).toEqual({
      type: "project",
      instanceId: 7,
      projectId: "project_123"
    });
  });

  it("starts a fresh native launch at the main menu even with a stale home URL", () => {
    expect(
      createInitialMobileNavigation("/?conversation_id=stale", { nativeFreshLaunch: true })
    ).toEqual({
      version: 1,
      stack: [{ type: "menu", instanceId: 0 }],
      hasInAppParent: false,
      historyIndex: 0
    });
  });
});

describe("mobile navigation history state", () => {
  it("round-trips a valid stack while preserving unrelated history state", () => {
    const initial = createInitialMobileNavigation("/");
    const pushed = pushMobilePage(initial, {
      type: "chat",
      instanceId: 2,
      conversationId: "conv_2"
    });
    const state = createMobileHistoryState(pushed, { routerIndex: 4 });

    expect(state.routerIndex).toBe(4);
    expect(readMobileHistoryState(state)).toEqual(pushed);
  });

  it("rejects malformed or unversioned history state", () => {
    expect(readMobileHistoryState(null)).toBeNull();
    expect(readMobileHistoryState({ [MOBILE_NAVIGATION_HISTORY_KEY]: { version: 2 } })).toBeNull();
    expect(
      readMobileHistoryState({
        [MOBILE_NAVIGATION_HISTORY_KEY]: {
          version: 1,
          hasInAppParent: true,
          historyIndex: 1,
          stack: [{ type: "chat", instanceId: 1, conversationId: "conv_1" }]
        }
      })
    ).toBeNull();
  });

  it("preserves parent descriptors while pushing detail pages", () => {
    let snapshot = createInitialMobileNavigation("/");
    snapshot = pushMobilePage(snapshot, {
      type: "project",
      instanceId: 1,
      projectId: "project_1"
    });
    snapshot = pushMobilePage(snapshot, {
      type: "chat",
      instanceId: 2,
      conversationId: "conv_2"
    });

    expect(snapshot.stack.map((page) => page.type)).toEqual(["menu", "project", "chat"]);
    expect(snapshot.historyIndex).toBe(2);
    expect(activeMobilePage(snapshot)).toEqual({
      type: "chat",
      instanceId: 2,
      conversationId: "conv_2"
    });
  });
});

describe("transient new chat", () => {
  it("keeps New Chat on the root URL", () => {
    const page: MobileNavigationPage = {
      type: "new-chat",
      instanceId: 3,
      projectId: null
    };
    expect(mobilePageHref(page)).toBe("/");
  });

  it("promotes a transient new chat without changing its mounted instance", () => {
    const initial = createInitialMobileNavigation("/");
    const newChat = pushMobilePage(initial, {
      type: "new-chat",
      instanceId: 8,
      projectId: "project_8"
    });
    const conversation = promoteNewChatToConversation(newChat, "conv_8");

    expect(activeMobilePage(conversation)).toEqual({
      type: "chat",
      instanceId: 8,
      conversationId: "conv_8"
    });
    expect(mobilePageHref(activeMobilePage(conversation))).toBe("/?conversation_id=conv_8");
  });
});
