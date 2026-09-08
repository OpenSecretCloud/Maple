import { describe, expect, test } from "bun:test";
import { AgentSessionSelectionMemory } from "./agentSessionSelection";

describe("AgentSessionSelectionMemory", () => {
  test("restores a remembered session that is still available", () => {
    const memory = new AgentSessionSelectionMemory();
    memory.remember("user-a", "session-b");

    expect(memory.resolve("user-a", [{ id: "session-a" }, { id: "session-b" }])).toBe("session-b");
  });

  test("clears a remembered session that is no longer available", () => {
    const memory = new AgentSessionSelectionMemory();
    memory.remember("user-a", "deleted-session");

    expect(memory.resolve("user-a", [{ id: "session-a" }])).toBeNull();
    expect(memory.resolve("user-a", [{ id: "deleted-session" }])).toBeNull();
  });

  test("forgets conditionally when an expected session is supplied", () => {
    const memory = new AgentSessionSelectionMemory();
    memory.remember("user-a", "session-a");

    memory.forget("user-a", "session-b");
    expect(memory.resolve("user-a", [{ id: "session-a" }])).toBe("session-a");

    memory.forget("user-a", "session-a");
    expect(memory.resolve("user-a", [{ id: "session-a" }])).toBeNull();

    memory.remember("user-a", "session-b");
    memory.forget("user-a");
    expect(memory.resolve("user-a", [{ id: "session-b" }])).toBeNull();
  });

  test("isolates remembered selections by user", () => {
    const memory = new AgentSessionSelectionMemory();
    memory.remember("user-a", "session-a");
    memory.remember("user-b", "session-b");

    expect(memory.resolve("user-a", [{ id: "session-a" }])).toBe("session-a");
    expect(memory.resolve("user-b", [{ id: "session-b" }])).toBe("session-b");

    memory.forget("user-a");
    expect(memory.resolve("user-b", [{ id: "session-b" }])).toBe("session-b");
  });
});
