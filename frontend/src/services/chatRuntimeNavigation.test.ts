import { describe, expect, test } from "bun:test";
import {
  canonicalConversationHistoryHref,
  conversationIdFromChatRuntimeKey,
  createChatHistoryEntryForDraft,
  createFreshChatHistoryEntry,
  draftRuntimeKeyFromHistoryState,
  historyStateWithDraftRuntimeKey,
  pushFreshChatHistoryEntry,
  pushChatHistoryEntryForDraft,
  runtimeKeyForChatLocation,
  runtimeKeyForChatProjection,
  shouldProjectMigratedConversation
} from "./chatRuntimeNavigation";
import {
  ChatRuntimeStore,
  createChatDraftKey,
  createConversationChatKey
} from "./chatRuntimeStore";

describe("chat runtime history navigation", () => {
  test("restores the same draft runtime from a bare home history entry", () => {
    const draftKey = createChatDraftKey("retained-composer");
    const historyState = historyStateWithDraftRuntimeKey(
      { __TSR_key: "router-owned", unrelated: 42 },
      draftKey
    );

    expect(runtimeKeyForChatLocation(undefined, historyState)).toBe(draftKey);
    expect(historyState).toEqual({
      __TSR_key: "router-owned",
      unrelated: 42,
      mapleChatDraftRuntimeKey: draftKey
    });
  });

  test("keeps the URL conversation authoritative regardless of draft restoration", () => {
    const draftKey = createChatDraftKey("previous-draft");

    for (const restoreDraftFromHistory of [true, false]) {
      expect(
        runtimeKeyForChatLocation(
          "conversation-a",
          historyStateWithDraftRuntimeKey({}, draftKey),
          () => createChatDraftKey("unused"),
          { restoreDraftFromHistory }
        )
      ).toBe(createConversationChatKey("conversation-a"));
    }
  });

  test("ignores an inherited draft key for a stack-owned mobile New Chat", () => {
    const previousDraftKey = createChatDraftKey("materialized-previous-chat");
    const nextDraftKey = createChatDraftKey("fresh-mobile-chat");

    expect(
      runtimeKeyForChatLocation(
        undefined,
        historyStateWithDraftRuntimeKey({}, previousDraftKey),
        () => nextDraftKey,
        { restoreDraftFromHistory: false }
      )
    ).toBe(nextDraftKey);
  });

  test("projects an uncached initial conversation instead of the previously selected chat", () => {
    const requestedKey = createConversationChatKey("historical-chat");
    const previouslySelectedKey = createConversationChatKey("just-created-chat");

    expect(runtimeKeyForChatProjection(requestedKey, false, previouslySelectedKey, false)).toBe(
      requestedKey
    );
  });

  test("uses a synchronously selected replacement after a committed runtime is deleted", () => {
    const deletedKey = createConversationChatKey("deleted-chat");
    const replacementKey = createChatDraftKey("replacement");

    expect(runtimeKeyForChatProjection(deletedKey, false, replacementKey, true)).toBe(
      replacementKey
    );
  });

  test("keeps an existing requested runtime after the initial selection commits", () => {
    const requestedKey = createConversationChatKey("cached-historical-chat");
    const otherSelectedKey = createConversationChatKey("other-chat");

    expect(runtimeKeyForChatProjection(requestedKey, true, otherSelectedKey, true)).toBe(
      requestedKey
    );
  });

  test("a saved draft history entry follows an offscreen rekeyed conversation", () => {
    const draftKey = createChatDraftKey("creating-conversation");
    const conversationKey = createConversationChatKey("created-offscreen");
    const store = new ChatRuntimeStore<unknown, unknown, { input: string }>({
      createComposer: () => ({ input: "" })
    });
    store.select(draftKey, { composer: { input: "original prompt" } });
    const run = store.beginRun(draftKey);
    store.rekey(draftKey, conversationKey, run.token);

    const restoredKey = runtimeKeyForChatLocation(
      undefined,
      historyStateWithDraftRuntimeKey({}, draftKey)
    );

    expect(restoredKey).toBe(draftKey);
    const restoredCanonicalKey = store.resolveKey(restoredKey);
    const restoredConversationId = conversationIdFromChatRuntimeKey(restoredCanonicalKey);
    expect(restoredCanonicalKey).toBe(conversationKey);
    expect(restoredConversationId).toBe("created-offscreen");
    expect(
      canonicalConversationHistoryHref(
        {
          pathname: "/",
          search: "?keep=1&project_id=stale-project",
          hash: "#latest-turn"
        },
        restoredConversationId!
      )
    ).toBe("/?keep=1&conversation_id=created-offscreen#latest-turn");
    expect(store.get(restoredKey)?.composer.input).toBe("original prompt");
    store.dispose();
  });

  test("does not project a migrated conversation after Unified Chat unmounts", () => {
    expect(shouldProjectMigratedConversation(false, true, false)).toBe(false);
    expect(shouldProjectMigratedConversation(false, false, true)).toBe(false);
    expect(shouldProjectMigratedConversation(true, true, false)).toBe(true);
    expect(shouldProjectMigratedConversation(true, false, true)).toBe(true);
  });

  test("creates a distinct keyed history entry for every explicit new chat", () => {
    const previous = createFreshChatHistoryEntry("first");
    const next = createFreshChatHistoryEntry("second");

    expect(previous.draftRuntimeKey).toBe(createChatDraftKey("first"));
    expect(next.draftRuntimeKey).toBe(createChatDraftKey("second"));
    expect(next.draftRuntimeKey).not.toBe(previous.draftRuntimeKey);
    expect(draftRuntimeKeyFromHistoryState(previous.historyState)).toBe(previous.draftRuntimeKey);
    expect(draftRuntimeKeyFromHistoryState(next.historyState)).toBe(next.draftRuntimeKey);
  });

  test("creates and pushes history entries for an existing retained draft key", () => {
    const draftKey = createChatDraftKey("retained");
    const entry = createChatHistoryEntryForDraft(draftKey);
    const calls: Array<{ state: unknown; url: string | URL | null | undefined }> = [];
    const history = {
      pushState: (state: unknown, _unused: string, url?: string | URL | null) => {
        calls.push({ state, url });
      }
    };

    const detail = pushChatHistoryEntryForDraft(history, "/?keep=1", null, draftKey);

    expect(entry).toEqual({
      draftRuntimeKey: draftKey,
      historyState: { mapleChatDraftRuntimeKey: draftKey }
    });
    expect(calls).toEqual([
      {
        state: { mapleChatDraftRuntimeKey: draftKey },
        url: "/?keep=1"
      }
    ]);
    expect(detail).toEqual({ projectId: null, draftRuntimeKey: draftKey });
  });

  test("pushes a fresh keyed project draft without replacing the streaming history entry", () => {
    const calls: Array<{ state: unknown; url: string | URL | null | undefined }> = [];
    const history = {
      pushState: (state: unknown, _unused: string, url?: string | URL | null) => {
        calls.push({ state, url });
      }
    };

    const detail = pushFreshChatHistoryEntry(history, "/?keep=1", "project-a", "project-draft");

    expect(calls).toEqual([
      {
        state: { mapleChatDraftRuntimeKey: createChatDraftKey("project-draft") },
        url: "/?keep=1"
      }
    ]);
    expect(detail).toEqual({
      projectId: "project-a",
      draftRuntimeKey: createChatDraftKey("project-draft")
    });
  });

  test("ignores malformed or non-draft history values", () => {
    expect(draftRuntimeKeyFromHistoryState(null)).toBeNull();
    expect(
      draftRuntimeKeyFromHistoryState({ mapleChatDraftRuntimeKey: "conversation:a" })
    ).toBeNull();
    expect(draftRuntimeKeyFromHistoryState({ mapleChatDraftRuntimeKey: "draft:" })).toBeNull();
  });
});
