import { describe, expect, test } from "bun:test";

import { validateAgentTaskRename } from "./agentTaskRename";

describe("validateAgentTaskRename", () => {
  test("rejects blank titles", () => {
    expect(validateAgentTaskRename(" \n\t ", "Current title")).toEqual({
      ok: false,
      error: "Task title cannot be empty."
    });
  });

  test("rejects the untouched opening title", () => {
    expect(validateAgentTaskRename("  New task  ", "New task")).toEqual({
      ok: false,
      error: "Please enter a different title."
    });
  });

  test("returns a trimmed Unicode title when it changed", () => {
    expect(validateAgentTaskRename("  調査 👩‍💻  ", "New task")).toEqual({
      ok: true,
      title: "調査 👩‍💻"
    });
  });
});
