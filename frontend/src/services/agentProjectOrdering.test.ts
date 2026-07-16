import { describe, expect, test } from "bun:test";

import type { AgentSessionSummary, RecentProjectRoot } from "./agentRuntimeService";
import {
  createProjectOrderState,
  groupAgentSessionsByRoot,
  hasExceededProjectDragThreshold,
  mergeAgentProjectRoots,
  projectInsertionIndex,
  projectOrderForExistingRegistration,
  projectOrderReducer,
  reorderProjectRoots
} from "./agentProjectOrdering";

function root(path: string, lastUsedMs = 1): RecentProjectRoot {
  return { path, name: path.split("/").at(-1) || path, lastUsedMs };
}

function session(id: string, projectRoot: string, updatedMs: number): AgentSessionSummary {
  return {
    id,
    title: id,
    projectRoot,
    createdMs: updatedMs,
    updatedMs,
    messageCount: 1,
    mode: "smart_approve"
  };
}

describe("mergeAgentProjectRoots", () => {
  test("preserves saved order, deduplicates, and appends unseen active and session roots deterministically", () => {
    const merged = mergeAgentProjectRoots(
      [root("/saved/b", 20), root("/saved/a", 10), root("/saved/b", 30)],
      "/active/c",
      [session("z", "/session/z", 500), session("d", "/session/d", 100)]
    );

    expect(merged.map((item) => item.path)).toEqual([
      "/saved/b",
      "/saved/a",
      "/active/c",
      "/session/d",
      "/session/z"
    ]);
    expect(
      merged
        .filter((item) => !item.path.startsWith("/saved/"))
        .every((item) => item.lastUsedMs === 0)
    ).toBe(true);
  });

  test("keeps an active saved root in its manual position", () => {
    const merged = mergeAgentProjectRoots([root("/a"), root("/b")], "/b", []);
    expect(merged.map((item) => item.path)).toEqual(["/a", "/b"]);
  });

  test("does not move unsaved session projects when the active session changes", () => {
    const saved = [root("/saved")];
    const sessions = [session("x", "/x", 20), session("y", "/y", 10)];

    expect(mergeAgentProjectRoots(saved, "/x", sessions).map((item) => item.path)).toEqual([
      "/saved",
      "/x",
      "/y"
    ]);
    expect(mergeAgentProjectRoots(saved, "/y", sessions).map((item) => item.path)).toEqual([
      "/saved",
      "/x",
      "/y"
    ]);
  });
});

describe("projectOrderForExistingRegistration", () => {
  const confirmed = [root("/saved/a"), root("/saved/b")];
  const visible = mergeAgentProjectRoots(confirmed, "/saved/a", [
    session("legacy", "/session/legacy", 10),
    session("unrelated", "/session/unrelated", 20)
  ]);

  test("registers only the selected visible session-derived project", () => {
    expect(projectOrderForExistingRegistration(visible, confirmed, "/session/legacy")).toEqual([
      "/saved/a",
      "/saved/b",
      "/session/legacy"
    ]);
    expect(projectOrderForExistingRegistration(visible, confirmed, "/genuinely/new")).toBeNull();
  });

  test("does not materialize session-derived projects when a saved project is selected", () => {
    expect(projectOrderForExistingRegistration(visible, confirmed, "/saved/a")).toBeNull();
  });
});

describe("groupAgentSessionsByRoot", () => {
  test("keeps sessions newest-first inside each project", () => {
    const grouped = groupAgentSessionsByRoot([
      session("old", "/a", 1),
      session("other", "/b", 3),
      session("new", "/a", 2)
    ]);
    expect(grouped.get("/a")?.map((item) => item.id)).toEqual(["new", "old"]);
    expect(grouped.get("/b")?.map((item) => item.id)).toEqual(["other"]);
  });
});

describe("project drag helpers", () => {
  const roots = [root("/a"), root("/b"), root("/c")];

  test("uses a six pixel movement threshold", () => {
    expect(hasExceededProjectDragThreshold(0, 0, 3, 4)).toBe(false);
    expect(hasExceededProjectDragThreshold(0, 0, 6, 0)).toBe(true);
  });

  test("calculates insertion positions before, between, and after non-dragged rows", () => {
    const centers = [
      { path: "/a", centerY: 10 },
      { path: "/b", centerY: 30 },
      { path: "/c", centerY: 50 }
    ];
    expect(projectInsertionIndex(0, centers, "/b")).toBe(0);
    expect(projectInsertionIndex(25, centers, "/b")).toBe(1);
    expect(projectInsertionIndex(60, centers, "/b")).toBe(2);
    expect(projectInsertionIndex(Number.NaN, centers, "/b")).toBeNull();
  });

  test("moves projects to first, middle, and last positions", () => {
    expect(reorderProjectRoots(roots, "/b", 0).map((item) => item.path)).toEqual([
      "/b",
      "/a",
      "/c"
    ]);
    expect(reorderProjectRoots(roots, "/a", 1).map((item) => item.path)).toEqual([
      "/b",
      "/a",
      "/c"
    ]);
    expect(reorderProjectRoots(roots, "/a", 2).map((item) => item.path)).toEqual([
      "/b",
      "/c",
      "/a"
    ]);
  });

  test("returns the original order for no-op, cancelled, missing, and invalid drops", () => {
    expect(reorderProjectRoots(roots, "/b", 1)).toBe(roots);
    expect(reorderProjectRoots(roots, "/b", null)).toBe(roots);
    expect(reorderProjectRoots(roots, "/missing", 0)).toBe(roots);
    expect(reorderProjectRoots(roots, "/b", 3)).toBe(roots);
  });
});

describe("projectOrderReducer", () => {
  const original = [root("/a"), root("/b")];
  const reordered = [original[1], original[0]];

  test("rolls an optimistic order back to the last confirmed order", () => {
    const pending = projectOrderReducer(createProjectOrderState(original), {
      type: "optimistic",
      requestId: 1,
      roots: reordered
    });
    const rolledBack = projectOrderReducer(pending, { type: "rejected", requestId: 1 });
    expect(rolledBack.visible).toBe(original);
    expect(rolledBack.pendingRequestId).toBeNull();
  });

  test("ignores stale confirmations and failures", () => {
    const first = projectOrderReducer(createProjectOrderState(original), {
      type: "optimistic",
      requestId: 1,
      roots: reordered
    });
    const second = projectOrderReducer(first, {
      type: "optimistic",
      requestId: 2,
      roots: original
    });
    expect(projectOrderReducer(second, { type: "confirmed", requestId: 1, roots: reordered })).toBe(
      second
    );
    expect(projectOrderReducer(second, { type: "rejected", requestId: 1 })).toBe(second);

    const confirmed = projectOrderReducer(second, {
      type: "confirmed",
      requestId: 2,
      roots: original
    });
    expect(confirmed.visible).toBe(original);
    expect(confirmed.pendingRequestId).toBeNull();
  });
});
