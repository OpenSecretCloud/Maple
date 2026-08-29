import { describe, expect, test } from "bun:test";
import {
  chatToolCallStatus,
  chatToolOutputStatus,
  chatToolTitle,
  chatWebSearchStatus,
  formatChatToolArguments
} from "./chatToolPresentation";

describe("chatToolTitle", () => {
  test("uses the query for web search", () => {
    expect(chatToolTitle("web_search", '{"query":"maple privacy"}')).toBe(
      'Web Search: "maple privacy"'
    );
  });

  test("summarizes one or several opened URLs", () => {
    expect(chatToolTitle("open_urls", '{"urls":["https://example.com"]}')).toBe(
      "Open URL: https://example.com"
    );
    expect(
      chatToolTitle("open_urls", '{"urls":["https://example.com","https://example.org"]}')
    ).toBe("Open URLs: 2 pages");
  });

  test("gives automatic image descriptions a readable title", () => {
    expect(chatToolTitle("read_image", '{"image_number":2,"content_index":1}')).toBe(
      "Read image: Image 2"
    );
  });

  test("keeps an unknown canonical function name recognizable", () => {
    expect(chatToolTitle("lookup_record", "{}")).toBe("lookup_record");
    expect(chatToolTitle("function", "{}")).toBe("Tool call");
  });
});

describe("chatToolCallStatus", () => {
  test("keeps a completed function-call item active until a result exists", () => {
    expect(chatToolCallStatus("completed", [])).toBe("active");
  });

  test("settles from an associated output even when it renders separately", () => {
    expect(chatToolCallStatus("completed", [{ output: "result", status: "completed" }])).toBe(
      "completed"
    );
    expect(chatToolCallStatus("completed", [{ output: "", status: undefined }])).toBe("completed");
  });

  test("keeps an explicitly active output active even when partial text exists", () => {
    expect(chatToolCallStatus("completed", [{ output: "partial", status: "in_progress" }])).toBe(
      "active"
    );
  });

  test("gives failure and incomplete output states precedence over result text", () => {
    expect(chatToolCallStatus("completed", [{ output: "failed", status: "error" }])).toBe("error");
    expect(chatToolCallStatus("completed", [{ output: "partial", status: "incomplete" }])).toBe(
      "incomplete"
    );
  });
});

describe("standalone Chat tool statuses", () => {
  test("preserves active, incomplete, error, and completed output states", () => {
    expect(chatToolOutputStatus("in_progress")).toBe("active");
    expect(chatToolOutputStatus("incomplete")).toBe("incomplete");
    expect(chatToolOutputStatus("error")).toBe("error");
    expect(chatToolOutputStatus("completed")).toBe("completed");
  });

  test("preserves native web search states", () => {
    expect(chatWebSearchStatus("searching")).toBe("active");
    expect(chatWebSearchStatus("completed")).toBe("completed");
    expect(chatWebSearchStatus("incomplete")).toBe("incomplete");
    expect(chatWebSearchStatus("failed")).toBe("error");
  });
});

test("formatChatToolArguments prettifies JSON without changing non-JSON input", () => {
  expect(formatChatToolArguments('{"query":"maple"}')).toBe('{\n  "query": "maple"\n}');
  expect(formatChatToolArguments("raw arguments")).toBe("raw arguments");
});
