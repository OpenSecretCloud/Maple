import { describe, expect, test } from "bun:test";
import {
  ChatProjectionScrollCoordinator,
  chatProjectionScrollTarget,
  projectedUserTurnScrollTop
} from "./chatProjectionScroll";

describe("ChatProjectionScrollCoordinator", () => {
  test("requests positioning whenever a different cached runtime is projected", () => {
    const coordinator = new ChatProjectionScrollCoordinator<string>();
    const resolveKey = (key: string) => key;

    expect(coordinator.activate("conversation:a", resolveKey)).toBe(true);
    expect(coordinator.takePositionRequest("conversation:a", true, resolveKey)).toBe(true);
    expect(coordinator.takePositionRequest("conversation:a", true, resolveKey)).toBe(false);

    expect(coordinator.activate("conversation:b", resolveKey)).toBe(true);
    expect(coordinator.takePositionRequest("conversation:b", true, resolveKey)).toBe(true);

    expect(coordinator.activate("conversation:a", resolveKey)).toBe(true);
    expect(coordinator.takePositionRequest("conversation:a", true, resolveKey)).toBe(true);
  });

  test("keeps a projection request pending until its messages are ready", () => {
    const coordinator = new ChatProjectionScrollCoordinator<string>();
    const resolveKey = (key: string) => key;

    coordinator.activate("conversation:a", resolveKey);
    expect(coordinator.takePositionRequest("conversation:a", false, resolveKey)).toBe(false);
    expect(coordinator.takePositionRequest("conversation:a", true, resolveKey)).toBe(true);
  });

  test("treats a draft rekey as the same logical projection", () => {
    const coordinator = new ChatProjectionScrollCoordinator<string>();
    const aliases = new Map<string, string>();
    const resolveKey = (key: string) => aliases.get(key) ?? key;

    coordinator.activate("draft:a", resolveKey);
    expect(coordinator.takePositionRequest("draft:a", true, resolveKey)).toBe(true);

    aliases.set("draft:a", "conversation:a");
    expect(coordinator.activate("conversation:a", resolveKey)).toBe(false);
    expect(coordinator.owns("draft:a", resolveKey)).toBe(true);
    expect(coordinator.owns("conversation:a", resolveKey)).toBe(true);
  });

  test("fences delayed work to the current projection", () => {
    const coordinator = new ChatProjectionScrollCoordinator<string>();
    const resolveKey = (key: string) => key;

    coordinator.activate("conversation:a", resolveKey);
    const firstAVisit = coordinator.captureLease("conversation:a", resolveKey);
    coordinator.activate("conversation:b", resolveKey);

    expect(coordinator.owns("conversation:a", resolveKey)).toBe(false);
    expect(coordinator.owns("conversation:b", resolveKey)).toBe(true);
    expect(firstAVisit && coordinator.ownsLease(firstAVisit, resolveKey)).toBe(false);

    coordinator.deactivate();
    expect(coordinator.owns("conversation:b", resolveKey)).toBe(false);
    expect(coordinator.activate("conversation:b", resolveKey)).toBe(true);
  });

  test("rejects delayed work from an earlier visit after an A to B to A switch", () => {
    const coordinator = new ChatProjectionScrollCoordinator<string>();
    const resolveKey = (key: string) => key;

    coordinator.activate("conversation:a", resolveKey);
    const firstAVisit = coordinator.captureLease("conversation:a", resolveKey)!;
    coordinator.activate("conversation:b", resolveKey);
    coordinator.activate("conversation:a", resolveKey);
    const secondAVisit = coordinator.captureLease("conversation:a", resolveKey)!;

    expect(coordinator.ownsLease(firstAVisit, resolveKey)).toBe(false);
    expect(coordinator.ownsLease(secondAVisit, resolveKey)).toBe(true);
  });
});

describe("chat projection scroll target", () => {
  const messages = [
    { id: "user-1", type: "message", role: "user" },
    { id: "assistant-1", type: "message", role: "assistant" },
    { id: "user-2", type: "message", role: "user" },
    { id: "assistant-2", type: "message", role: "assistant" }
  ];

  test("anchors an active stream to its newest user turn", () => {
    expect(chatProjectionScrollTarget(messages, true)).toEqual({
      type: "latest-user",
      messageId: "user-2"
    });
  });

  test("opens completed chats and user-less streams at the bottom", () => {
    expect(chatProjectionScrollTarget(messages, false)).toEqual({ type: "bottom" });
    expect(
      chatProjectionScrollTarget([{ id: "assistant-1", type: "message", role: "assistant" }], true)
    ).toEqual({ type: "bottom" });
  });

  test("calculates a stable user-turn offset from the current projection", () => {
    expect(
      projectedUserTurnScrollTop({
        currentScrollTop: 1200,
        containerTop: 56,
        userTurnTop: 380,
        topOffset: 16
      })
    ).toBe(1508);
    expect(
      projectedUserTurnScrollTop({
        currentScrollTop: 0,
        containerTop: 56,
        userTurnTop: 40,
        topOffset: 16
      })
    ).toBe(0);
  });
});
