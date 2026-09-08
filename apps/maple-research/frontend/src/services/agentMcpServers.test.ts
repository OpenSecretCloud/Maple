import { describe, expect, test } from "bun:test";
import type { AgentMcpServer } from "./agentRuntimeService";
import {
  gooseMcpServerKey,
  isValidMcpTimeoutSeconds,
  reconcileNewChatMcpServerNames
} from "./agentMcpServers";

function server(name: string, enabled: boolean): AgentMcpServer {
  return {
    name,
    description: "",
    enabled,
    timeoutSeconds: 300,
    transport: { type: "stdio", command: "fixture", environment: [] }
  };
}

describe("Agent MCP server identity", () => {
  test("matches Goose name normalization", () => {
    expect(gooseMcpServerKey("My Server")).toBe("myserver");
    expect(gooseMcpServerKey("Foo.Bar")).toBe("foo_bar");
    expect(gooseMcpServerKey("foo/bar")).toBe("foo_bar");
    expect(gooseMcpServerKey("KEEP_me-2")).toBe("keep_me-2");
    expect(gooseMcpServerKey("café")).toBe("caf_");
    expect(gooseMcpServerKey("one\u0085two")).toBe("onetwo");
    expect(gooseMcpServerKey("one\ufefftwo")).toBe("one_two");
  });

  test("accepts only positive whole-second timeouts", () => {
    expect(isValidMcpTimeoutSeconds(1)).toBe(true);
    expect(isValidMcpTimeoutSeconds(300)).toBe(true);
    expect(isValidMcpTimeoutSeconds(0.5)).toBe(false);
    expect(isValidMcpTimeoutSeconds(1.5)).toBe(false);
    expect(isValidMcpTimeoutSeconds(Number.NaN)).toBe(false);
    expect(isValidMcpTimeoutSeconds(Number.POSITIVE_INFINITY)).toBe(false);
  });
});

describe("new-chat MCP selection reconciliation", () => {
  test("preserves explicit per-chat overrides across unrelated edits", () => {
    const previous = [server("default_on", true), server("default_off", false)];
    const saved = previous.map((entry) => ({ ...entry, description: "edited" }));

    expect(reconcileNewChatMcpServerNames(previous, saved, new Set(["default_off"]))).toEqual(
      new Set(["default_off"])
    );
  });

  test("applies changed defaults when the pending selection had no override", () => {
    const previous = [server("first", true), server("second", false)];
    const saved = [server("first", false), server("second", true)];

    expect(reconcileNewChatMcpServerNames(previous, saved, new Set(["first"]))).toEqual(
      new Set(["second"])
    );
  });

  test("adds new defaults and drops deleted servers", () => {
    const previous = [server("deleted", true)];
    const saved = [server("new_on", true), server("new_off", false)];

    expect(reconcileNewChatMcpServerNames(previous, saved, new Set(["deleted"]))).toEqual(
      new Set(["new_on"])
    );
  });

  test("preserves selection across names with the same Goose key", () => {
    const previous = [server("My Server", false)];
    const saved = [server("myserver", false)];

    expect(reconcileNewChatMcpServerNames(previous, saved, new Set(["My Server"]))).toEqual(
      new Set(["myserver"])
    );
  });
});
