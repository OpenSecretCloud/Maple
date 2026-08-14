import { describe, expect, it } from "bun:test";
import {
  MOBILE_NAVIGATION_HISTORY_KEY,
  activeMobilePage,
  createNativeMobileLaunchGate,
  createInitialMobileNavigation,
  createMobileHistoryState,
  mobileMenuHistoryDelta,
  mobileMenuOwnsDocumentCanvas,
  mobilePageHref,
  mobilePageUsesMenuButton,
  pageFromHref,
  promoteNewChatToConversation,
  pushMobilePage,
  readMobileHistoryState,
  resolveMissingMobileConversation,
  resolveMobileDraftProjectId,
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

  it("starts a fresh native launch on New Chat even with a stale home URL", () => {
    expect(
      createInitialMobileNavigation("/?conversation_id=stale", { nativeFreshLaunch: true })
    ).toEqual({
      version: 1,
      stack: [
        { type: "menu", instanceId: 0 },
        { type: "new-chat", instanceId: 1, projectId: null }
      ],
      hasInAppParent: false,
      historyIndex: 0
    });
  });

  it("does not consume the native launch claim during an abandoned render", () => {
    const gate = createNativeMobileLaunchGate();

    expect(gate.peek(true)).toBe(true);
    expect(gate.peek(true)).toBe(true);

    gate.commit();
    expect(gate.peek(true)).toBe(false);
  });

  it("does not let a web render consume the native launch claim", () => {
    const gate = createNativeMobileLaunchGate();

    expect(gate.peek(false)).toBe(false);
    expect(gate.peek(true)).toBe(true);
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
    const state = createMobileHistoryState(pushed, { routerIndex: 4, __TSR_index: 9 });

    expect(state.routerIndex).toBe(4);
    expect(state.__TSR_index).toBe(9);
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
  it("uses an explicit project scope, including explicit unscoped New Chat", () => {
    expect(resolveMobileDraftProjectId("project_8", "stale_project")).toBe("project_8");
    expect(resolveMobileDraftProjectId(null, "stale_project")).toBeNull();
    expect(resolveMobileDraftProjectId(undefined, "selected_project")).toBe("selected_project");
  });

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
    const conversation = promoteNewChatToConversation(newChat, 8, "conv_8");

    expect(activeMobilePage(conversation)).toEqual({
      type: "chat",
      instanceId: 8,
      conversationId: "conv_8",
      openedFromNewChat: true
    });
    expect(mobilePageHref(activeMobilePage(conversation))).toBe("/?conversation_id=conv_8");
  });

  it("keeps the menu control after New Chat becomes a conversation", () => {
    const initial = createInitialMobileNavigation("/", { nativeFreshLaunch: true });
    const promoted = promoteNewChatToConversation(initial, 1, "conv_new");

    expect(mobilePageUsesMenuButton(activeMobilePage(initial))).toBe(true);
    expect(mobilePageUsesMenuButton(activeMobilePage(promoted))).toBe(true);
    expect(
      mobilePageUsesMenuButton({
        type: "chat",
        instanceId: 4,
        conversationId: "conv_selected"
      })
    ).toBe(false);
  });

  it("ignores a late creation callback from a New Chat page that is no longer active", () => {
    const initial = createInitialMobileNavigation("/", { nativeFreshLaunch: true });
    const replacement = pushMobilePage(initial, {
      type: "new-chat",
      instanceId: 2,
      projectId: null
    });

    expect(promoteNewChatToConversation(replacement, 1, "conv_late")).toBe(replacement);
  });
});

describe("missing mobile conversations", () => {
  it("sanitizes a pushed chat before returning to its project parent", () => {
    let snapshot = createInitialMobileNavigation("/");
    snapshot = pushMobilePage(snapshot, {
      type: "project",
      instanceId: 1,
      projectId: "project_1"
    });
    snapshot = pushMobilePage(snapshot, {
      type: "chat",
      instanceId: 2,
      conversationId: "missing"
    });

    expect(resolveMissingMobileConversation(snapshot, 2)).toEqual({
      sanitizedSnapshot: {
        version: 1,
        stack: [
          { type: "menu", instanceId: 0 },
          { type: "project", instanceId: 1, projectId: "project_1" }
        ],
        hasInAppParent: true,
        historyIndex: 2
      },
      targetSnapshot: {
        version: 1,
        stack: [
          { type: "menu", instanceId: 0 },
          { type: "project", instanceId: 1, projectId: "project_1" }
        ],
        hasInAppParent: true,
        historyIndex: 1
      },
      historyDelta: -1
    });
  });

  it("falls back in place for a directly loaded missing chat", () => {
    const snapshot = createInitialMobileNavigation("/?conversation_id=missing");

    expect(resolveMissingMobileConversation(snapshot, 1)).toEqual({
      sanitizedSnapshot: {
        version: 1,
        stack: [{ type: "menu", instanceId: 0 }],
        hasInAppParent: false,
        historyIndex: 0
      },
      targetSnapshot: {
        version: 1,
        stack: [{ type: "menu", instanceId: 0 }],
        hasInAppParent: false,
        historyIndex: 0
      },
      historyDelta: null
    });
  });

  it("returns a missing promoted project New Chat directly to the menu", () => {
    let snapshot = createInitialMobileNavigation("/");
    snapshot = pushMobilePage(snapshot, {
      type: "project",
      instanceId: 1,
      projectId: "project_1"
    });
    snapshot = pushMobilePage(snapshot, {
      type: "new-chat",
      instanceId: 2,
      projectId: "project_1"
    });
    snapshot = promoteNewChatToConversation(snapshot, 2, "missing");

    expect(resolveMissingMobileConversation(snapshot, 2)).toEqual({
      sanitizedSnapshot: {
        version: 1,
        stack: [{ type: "menu", instanceId: 0 }],
        hasInAppParent: true,
        historyIndex: 2
      },
      targetSnapshot: {
        version: 1,
        stack: [{ type: "menu", instanceId: 0 }],
        hasInAppParent: false,
        historyIndex: 0
      },
      historyDelta: -2
    });
  });

  it("replaces a root-like missing promoted New Chat with the menu", () => {
    const snapshot = promoteNewChatToConversation(
      createInitialMobileNavigation("/", { nativeFreshLaunch: true }),
      1,
      "missing"
    );

    expect(resolveMissingMobileConversation(snapshot, 1)).toEqual({
      sanitizedSnapshot: {
        version: 1,
        stack: [{ type: "menu", instanceId: 0 }],
        hasInAppParent: false,
        historyIndex: 0
      },
      targetSnapshot: {
        version: 1,
        stack: [{ type: "menu", instanceId: 0 }],
        hasInAppParent: false,
        historyIndex: 0
      },
      historyDelta: null
    });
  });

  it("crosses a direct-chat root when a pushed promoted New Chat is missing", () => {
    let snapshot = createInitialMobileNavigation("/?conversation_id=direct_chat");
    snapshot = pushMobilePage(snapshot, {
      type: "new-chat",
      instanceId: 2,
      projectId: null
    });
    snapshot = promoteNewChatToConversation(snapshot, 2, "missing");

    expect(resolveMissingMobileConversation(snapshot, 2)?.historyDelta).toBe(-1);
    expect(resolveMissingMobileConversation(snapshot, 2)?.targetSnapshot).toEqual(
      createInitialMobileNavigation("/")
    );
  });

  it("ignores a stale missing-chat callback from a page that is no longer active", () => {
    let snapshot = createInitialMobileNavigation("/");
    snapshot = pushMobilePage(snapshot, {
      type: "chat",
      instanceId: 1,
      conversationId: "missing"
    });
    snapshot = pushMobilePage(snapshot, {
      type: "new-chat",
      instanceId: 2,
      projectId: null
    });

    expect(resolveMissingMobileConversation(snapshot, 1)).toBeNull();
  });
});

describe("opening the mobile menu", () => {
  it("returns through all pushed history entries to reach the menu", () => {
    let snapshot = createInitialMobileNavigation("/");
    snapshot = pushMobilePage(snapshot, {
      type: "project",
      instanceId: 1,
      projectId: "project_1"
    });
    snapshot = pushMobilePage(snapshot, {
      type: "new-chat",
      instanceId: 2,
      projectId: "project_1"
    });

    expect(mobileMenuHistoryDelta(snapshot)).toBe(-2);
  });

  it("crosses pushed entries even when the root browser entry was a direct chat", () => {
    let snapshot = createInitialMobileNavigation("/?conversation_id=direct_chat");
    snapshot = pushMobilePage(snapshot, {
      type: "new-chat",
      instanceId: 2,
      projectId: null
    });

    // The navigation shell normalizes that direct-chat root entry to the menu after the jump.
    expect(mobileMenuHistoryDelta(snapshot)).toBe(-1);
  });

  it("replaces a root-like native New Chat entry instead of leaving the app", () => {
    const snapshot = createInitialMobileNavigation("/", { nativeFreshLaunch: true });
    expect(mobileMenuHistoryDelta(snapshot)).toBeNull();
  });
});

describe("mobile menu document canvas", () => {
  const menu: MobileNavigationPage = { type: "menu", instanceId: 0 };
  const chat: MobileNavigationPage = {
    type: "chat",
    instanceId: 1,
    conversationId: "conversation-a"
  };

  it("uses the menu canvas while the menu is active or interactively revealed", () => {
    expect(mobileMenuOwnsDocumentCanvas("/", menu, null)).toBe(true);
    expect(mobileMenuOwnsDocumentCanvas("/?conversation_id=conversation-a", chat, menu)).toBe(true);
    expect(mobileMenuOwnsDocumentCanvas("/?conversation_id=conversation-a", chat, null)).toBe(
      false
    );
  });

  it("does not leak the menu canvas into Settings while the home stack is suspended", () => {
    expect(mobileMenuOwnsDocumentCanvas(null, menu, null)).toBe(false);
    expect(mobileMenuOwnsDocumentCanvas(null, chat, menu)).toBe(false);
  });
});
