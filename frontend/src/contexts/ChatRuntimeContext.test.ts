import { describe, expect, spyOn, test } from "bun:test";
import { createChatComposerState, createChatRuntimeStore } from "./ChatRuntimeContext";
import { createChatDraftKey, createConversationChatKey } from "../services/chatRuntimeStore";

type Conversation = { id: string };
type Message = { id: string };

describe("ChatRuntimeContext eviction policy", () => {
  test("retains and restores an idle draft with unsent composer content across selection", () => {
    const store = createChatRuntimeStore<Conversation, Message>(0);
    const draftKey = createChatDraftKey("retained-unsent-composer");
    const conversationKey = createConversationChatKey("visited-conversation");
    const composer = createChatComposerState("project-a");
    composer.input = "keep this unfinished message";
    composer.documentText = "document payload";
    composer.documentName = "notes.md";

    store.select(draftKey, { composer });
    store.select(conversationKey, { conversation: { id: "visited-conversation" } });

    expect(store.get(draftKey)?.composer).toMatchObject({
      input: "keep this unfinished message",
      draftProjectId: "project-a",
      documentText: "document payload",
      documentName: "notes.md"
    });

    store.select(draftKey);
    expect(store.getActiveKey()).toBe(draftKey);
    expect(store.getActive()?.composer.input).toBe("keep this unfinished message");
  });

  test("evicts a completed rekeyed conversation even if draft project metadata is stale", () => {
    const store = createChatRuntimeStore<Conversation, Message>(0);
    const draftKey = createChatDraftKey("project-draft");
    const conversationKey = createConversationChatKey("created-conversation");
    const nextActiveKey = createConversationChatKey("next-active");
    store.select(draftKey, {
      composer: createChatComposerState("project-a")
    });
    const run = store.beginRun(draftKey);

    expect(store.rekey(draftKey, conversationKey, run.token)).toBe(conversationKey);
    expect(store.finishRun(conversationKey, run.token)).toBe(true);
    expect(store.get(conversationKey)?.composer.draftProjectId).toBe("project-a");

    store.select(nextActiveKey);

    expect(store.get(conversationKey)).toBeUndefined();
    expect(store.get(draftKey)).toBeUndefined();
    expect(store.getActiveKey()).toBe(nextActiveKey);
  });

  test("deleting an idle superseded draft disposes only its object URLs", () => {
    const revokeObjectURL = spyOn(URL, "revokeObjectURL").mockImplementation(() => {});
    const store = createChatRuntimeStore<Conversation, Message>();
    const idleDraftKey = createChatDraftKey("idle-superseded");
    const runningDraftKey = createChatDraftKey("still-running");
    const idleFile = new File(["idle"], "idle.png", { type: "image/png" });
    const runningFile = new File(["running"], "running.png", { type: "image/png" });
    const idleComposer = createChatComposerState();
    idleComposer.draftImages = [idleFile];
    idleComposer.imageUrls.set(idleFile, "blob:idle");
    const runningComposer = createChatComposerState();
    runningComposer.draftImages = [runningFile];
    runningComposer.imageUrls.set(runningFile, "blob:running");

    try {
      store.ensure(idleDraftKey, { composer: idleComposer });
      store.select(runningDraftKey, { composer: runningComposer });
      const running = store.beginRun(runningDraftKey);

      expect(store.delete(idleDraftKey)).toBe(true);

      expect(revokeObjectURL).toHaveBeenCalledTimes(1);
      expect(revokeObjectURL).toHaveBeenCalledWith("blob:idle");
      expect(store.get(idleDraftKey)).toBeUndefined();
      expect(store.isRunCurrent(runningDraftKey, running.token)).toBe(true);
      expect(running.signal.aborted).toBe(false);
      expect(store.get(runningDraftKey)?.composer.imageUrls.get(runningFile)).toBe("blob:running");

      store.dispose();
      expect(running.signal.aborted).toBe(true);
      expect(revokeObjectURL).toHaveBeenCalledWith("blob:running");
    } finally {
      store.dispose();
      revokeObjectURL.mockRestore();
    }
  });
});
