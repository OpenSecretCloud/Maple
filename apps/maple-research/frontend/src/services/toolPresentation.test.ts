import { describe, expect, test } from "bun:test";
import { toolKindFromAgentTimeline, toolKindFromName, toolKindLabel } from "./toolPresentation";

describe("toolKindFromName", () => {
  test.each([
    ["shell", "shell"],
    ["developer__shell", "shell"],
    ["read_file", "file-read"],
    ["developer__read_image", "file-read"],
    ["edit_file", "file-write"],
    ["write_file", "file-write"],
    ["web_search", "web"],
    ["developer__open_url", "web"],
    ["open_urls", "web"],
    ["github__search_code", "mcp"],
    ["skills__load_skill", "mcp"],
    ["skills__shell", "mcp"],
    ["unknown_tool", "generic"]
  ] as const)("classifies %s as %s", (name, expected) => {
    expect(toolKindFromName(name)).toBe(expected);
  });
});

describe("toolKindFromAgentTimeline", () => {
  test.each([
    ["functions.shell:1", "shell"],
    ["functions.developer__shell:2", "shell"],
    ["functions.read_file:3", "file-read"],
    ["functions.developer__read_image:4", "file-read"],
    ["functions.read:4", "file-read"],
    ["functions.edit_file:5", "file-write"],
    ["functions.write_file:6", "file-write"],
    ["functions.edit:6", "file-write"],
    ["functions.write:6", "file-write"],
    ["functions.web_search:7", "web"],
    ["functions.developer__open_url:8", "web"],
    ["functions.github__search_code:9", "mcp"],
    ["functions.skills__load_skill:10", "mcp"],
    ["functions.skills__shell:11", "mcp"]
  ] as const)("classifies %s as %s", (id, expected) => {
    expect(toolKindFromAgentTimeline(id)).toBe(expected);
  });

  test.each([
    ["Terminal: pwd", "shell"],
    ["Read file: notes.txt", "file-read"],
    ["read: notes.txt", "file-read"],
    ["Editor: report.txt", "file-write"],
    ["edit: report.txt", "file-write"],
    ["write: report.txt", "file-write"],
    ["Write file: report.txt", "file-write"],
    ["Web Search: Example Domain", "web"],
    ["Open url: https://example.com", "web"]
  ] as const)(
    "uses Maple's stable %s label when a provider ID has no tool name",
    (title, expected) => {
      expect(toolKindFromAgentTimeline("chatcmpl-tool-123", title)).toBe(expected);
    }
  );

  test("uses the generic type when the canonical tool name and stable label are unsupported", () => {
    expect(
      toolKindFromAgentTimeline("chatcmpl-tool-123", "Unrecognized action: shell in prose")
    ).toBe("generic");
    expect(toolKindFromAgentTimeline("functions.load_skill:10", "Loading skill: example")).toBe(
      "generic"
    );
    expect(
      toolKindFromAgentTimeline("functions.unknown_tool:11", "Terminal: ignored fallback")
    ).toBe("generic");
  });
});

describe("toolKindLabel", () => {
  test("provides a text alternative for every visual type", () => {
    expect(toolKindLabel("shell")).toBe("Shell command");
    expect(toolKindLabel("file-read")).toBe("File read");
    expect(toolKindLabel("file-write")).toBe("File change");
    expect(toolKindLabel("web")).toBe("Web tool");
    expect(toolKindLabel("mcp")).toBe("MCP tool");
    expect(toolKindLabel("generic")).toBe("Tool call");
  });
});
