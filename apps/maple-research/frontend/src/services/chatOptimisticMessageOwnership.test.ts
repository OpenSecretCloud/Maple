import { describe, expect, test } from "bun:test";
import {
  getRegisteredChatOptimisticMessage,
  markOptimisticMessageIncomplete,
  registerChatOptimisticMessage,
  unregisterChatOptimisticMessage
} from "./chatOptimisticMessageOwnership";

describe("chat optimistic message ownership", () => {
  test("a remounted view marks the exact pre-response optimistic message incomplete", () => {
    const persistentRuntimeStore = {};
    const runToken = 17;
    const sourceMessage = { id: "source-user", text: "source prompt", status: "completed" };
    const olderMessage = { id: "older-user", text: "older prompt", status: "completed" };
    registerChatOptimisticMessage(persistentRuntimeStore, runToken, sourceMessage.id);

    // A new UnifiedChat mount has only the persistent store and run token.
    const ownedMessageId = getRegisteredChatOptimisticMessage(persistentRuntimeStore, runToken);
    const messages = markOptimisticMessageIncomplete([olderMessage, sourceMessage], ownedMessageId);

    expect(messages).toEqual([olderMessage, { ...sourceMessage, status: "incomplete" }]);
    expect(
      unregisterChatOptimisticMessage(persistentRuntimeStore, runToken, sourceMessage.id)
    ).toBe(true);
    expect(getRegisteredChatOptimisticMessage(persistentRuntimeStore, runToken)).toBeUndefined();
  });

  test("identity-safe unregister cannot clear a replacement registration", () => {
    const persistentRuntimeStore = {};
    registerChatOptimisticMessage(persistentRuntimeStore, 23, "obsolete");
    registerChatOptimisticMessage(persistentRuntimeStore, 23, "replacement");

    expect(unregisterChatOptimisticMessage(persistentRuntimeStore, 23, "obsolete")).toBe(false);
    expect(getRegisteredChatOptimisticMessage(persistentRuntimeStore, 23)).toBe("replacement");
    expect(unregisterChatOptimisticMessage(persistentRuntimeStore, 23, "replacement")).toBe(true);
  });

  test("a post-response failure cannot guess and alter an unrelated user message", () => {
    const messages = [{ id: "existing-user", status: "completed" }];

    expect(markOptimisticMessageIncomplete(messages, undefined)).toEqual(messages);
    expect(messages[0]?.status).toBe("completed");
  });
});
