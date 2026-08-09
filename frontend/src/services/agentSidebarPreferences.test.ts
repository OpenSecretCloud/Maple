import { describe, expect, test } from "bun:test";

import type { RecentProjectRoot } from "./agentRuntimeService";
import {
  AGENT_SIDEBAR_PREFERENCES_STORAGE_PREFIX,
  agentSidebarPreferencesStorageKey,
  createEmptyAgentSidebarPreferences,
  loadAgentSidebarPreferences,
  projectRootsWithDisplayNames,
  renameAgentProjectDisplayName,
  saveAgentSidebarPreferences,
  toggleAgentProjectCollapsed,
  type AgentSidebarPreferencesStorage
} from "./agentSidebarPreferences";

class MemoryStorage implements AgentSidebarPreferencesStorage {
  readonly values = new Map<string, string>();
  writes = 0;

  getItem(key: string): string | null {
    return this.values.get(key) ?? null;
  }

  setItem(key: string, value: string): void {
    this.writes += 1;
    this.values.set(key, value);
  }

  seed(key: string, value: string): void {
    this.values.set(key, value);
  }
}

function root(path: string, name = path.split(/[\\/]/).filter(Boolean).at(-1) || path) {
  return { path, name, lastUsedMs: 1 } satisfies RecentProjectRoot;
}

describe("Agent sidebar preference persistence", () => {
  test("uses a versioned account-scoped key and keeps accounts isolated", () => {
    const storage = new MemoryStorage();
    const first = toggleAgentProjectCollapsed(createEmptyAgentSidebarPreferences(), "/first");
    const second = renameAgentProjectDisplayName(
      createEmptyAgentSidebarPreferences(),
      root("/second"),
      "Second account"
    );

    expect(agentSidebarPreferencesStorageKey("account/a")).toBe(
      `${AGENT_SIDEBAR_PREFERENCES_STORAGE_PREFIX}account%2Fa`
    );
    expect(saveAgentSidebarPreferences("account/a", first, storage)).toBe(true);
    expect(saveAgentSidebarPreferences("account/b", second, storage)).toBe(true);
    expect(storage.values.size).toBe(2);

    expect([...loadAgentSidebarPreferences("account/a", storage).collapsedProjectRoots]).toEqual([
      "/first"
    ]);
    expect([
      ...loadAgentSidebarPreferences("account/a", storage).projectDisplayNameOverrides
    ]).toEqual([]);
    expect([...loadAgentSidebarPreferences("account/b", storage).collapsedProjectRoots]).toEqual(
      []
    );
    expect([
      ...loadAgentSidebarPreferences("account/b", storage).projectDisplayNameOverrides
    ]).toEqual([["/second", "Second account"]]);
  });

  test("defaults safely for missing, malformed, wrong-version, and unavailable storage", () => {
    const storage = new MemoryStorage();
    const key = agentSidebarPreferencesStorageKey("account");

    expect(loadAgentSidebarPreferences("account", storage)).toEqual({
      collapsedProjectRoots: new Set(),
      projectDisplayNameOverrides: new Map()
    });

    storage.seed(key, "not json");
    expect(loadAgentSidebarPreferences("account", storage)).toEqual({
      collapsedProjectRoots: new Set(),
      projectDisplayNameOverrides: new Map()
    });

    storage.seed(
      key,
      JSON.stringify({
        version: 2,
        collapsedProjectRoots: ["/wrong-version"],
        projectDisplayNameOverrides: [{ path: "/wrong-version", displayName: "Wrong" }]
      })
    );
    expect(loadAgentSidebarPreferences("account", storage)).toEqual({
      collapsedProjectRoots: new Set(),
      projectDisplayNameOverrides: new Map()
    });

    const unavailable: AgentSidebarPreferencesStorage = {
      getItem: () => {
        throw new Error("storage unavailable");
      },
      setItem: () => {
        throw new Error("storage unavailable");
      }
    };
    expect(loadAgentSidebarPreferences("account", null)).toEqual({
      collapsedProjectRoots: new Set(),
      projectDisplayNameOverrides: new Map()
    });
    expect(loadAgentSidebarPreferences("account", unavailable)).toEqual({
      collapsedProjectRoots: new Set(),
      projectDisplayNameOverrides: new Map()
    });
    expect(saveAgentSidebarPreferences("account", createEmptyAgentSidebarPreferences(), null)).toBe(
      false
    );
    expect(
      saveAgentSidebarPreferences("account", createEmptyAgentSidebarPreferences(), unavailable)
    ).toBe(false);
  });

  test("restores valid entries while filtering malformed values and deduplicating by canonical path", () => {
    const storage = new MemoryStorage();
    storage.seed(
      agentSidebarPreferencesStorageKey("account"),
      JSON.stringify({
        version: 1,
        collapsedProjectRoots: ["/alpha", "", 42, "/alpha", "/path with space "],
        projectDisplayNameOverrides: [
          { path: "/alpha", displayName: " First name " },
          null,
          { path: "", displayName: "Missing path" },
          { path: "/blank", displayName: "   " },
          { path: "/number", displayName: 42 },
          { path: "/alpha", displayName: " Last name wins " },
          { path: "/path with space ", displayName: " Spaced path " }
        ]
      })
    );

    const restored = loadAgentSidebarPreferences("account", storage);
    expect(restored.collapsedProjectRoots).toEqual(new Set(["/alpha", "/path with space "]));
    expect(restored.projectDisplayNameOverrides).toEqual(
      new Map([
        ["/alpha", "Last name wins"],
        ["/path with space ", "Spaced path"]
      ])
    );
    expect(restored.collapsedProjectRoots).toBeInstanceOf(Set);
    expect(restored.projectDisplayNameOverrides).toBeInstanceOf(Map);
  });

  test("round-trips the exact v1 array schema with an explicit save", () => {
    const storage = new MemoryStorage();
    let preferences = toggleAgentProjectCollapsed(createEmptyAgentSidebarPreferences(), "/alpha");
    preferences = renameAgentProjectDisplayName(
      preferences,
      root("/alpha", "alpha"),
      " Alpha display "
    );

    expect(saveAgentSidebarPreferences("account", preferences, storage)).toBe(true);
    expect(
      JSON.parse(storage.getItem(agentSidebarPreferencesStorageKey("account")) || "null")
    ).toEqual({
      version: 1,
      collapsedProjectRoots: ["/alpha"],
      projectDisplayNameOverrides: [{ path: "/alpha", displayName: "Alpha display" }]
    });

    const restored = loadAgentSidebarPreferences("account", storage);
    expect(restored.collapsedProjectRoots).toEqual(new Set(["/alpha"]));
    expect(restored.projectDisplayNameOverrides).toEqual(new Map([["/alpha", "Alpha display"]]));
  });
});

describe("Agent project display preferences", () => {
  test("keeps stale preferences inert, defaults new roots to expanded, and never writes during load or projection", () => {
    const storage = new MemoryStorage();
    storage.seed(
      agentSidebarPreferencesStorageKey("account"),
      JSON.stringify({
        version: 1,
        collapsedProjectRoots: ["/stale"],
        projectDisplayNameOverrides: [{ path: "/stale", displayName: "Old project" }]
      })
    );

    const preferences = loadAgentSidebarPreferences("account", storage);
    expect(projectRootsWithDisplayNames([], preferences)).toEqual([]);
    const projected = projectRootsWithDisplayNames([root("/new", "new")], preferences);

    expect(projected[0].displayName).toBe("new");
    expect(preferences.collapsedProjectRoots.has("/new")).toBe(false);
    expect(preferences.collapsedProjectRoots.has("/stale")).toBe(true);
    expect(preferences.projectDisplayNameOverrides.get("/stale")).toBe("Old project");
    expect(storage.writes).toBe(0);
  });

  test("trims renames, rejects blank names, and deletes an override equal to the native fallback", () => {
    const project = root("/work/native", "native");
    const empty = createEmptyAgentSidebarPreferences();
    const renamed = renameAgentProjectDisplayName(empty, project, "  Custom name  ");

    expect(renamed.projectDisplayNameOverrides.get(project.path)).toBe("Custom name");
    expect(() => renameAgentProjectDisplayName(renamed, project, "   ")).toThrow(
      "Project name cannot be empty."
    );
    expect(renamed.projectDisplayNameOverrides.get(project.path)).toBe("Custom name");

    const reverted = renameAgentProjectDisplayName(renamed, project, " native ");
    expect(reverted.projectDisplayNameOverrides.has(project.path)).toBe(false);
    expect(projectRootsWithDisplayNames([project], reverted)[0].displayName).toBe("native");
  });

  test("allows duplicate display names because canonical paths remain the identity", () => {
    const first = root("/projects/first", "first");
    const second = root("/projects/second", "second");
    let preferences = renameAgentProjectDisplayName(
      createEmptyAgentSidebarPreferences(),
      first,
      "Shared"
    );
    preferences = renameAgentProjectDisplayName(preferences, second, " Shared ");

    expect(
      projectRootsWithDisplayNames([first, second], preferences).map((item) => item.displayName)
    ).toEqual(["Shared", "Shared"]);
    expect([...preferences.projectDisplayNameOverrides]).toEqual([
      [first.path, "Shared"],
      [second.path, "Shared"]
    ]);
  });

  test("preserves project order, canonical identity, native fields, and basename fallback", () => {
    const roots = [
      root("/projects/beta", "beta"),
      root("C:\\work\\alpha", "   "),
      root("/projects/gamma", "gamma")
    ];
    roots[0].lastUsedMs = 20;
    roots[1].lastUsedMs = 10;
    roots[2].lastUsedMs = 5;
    const preferences = renameAgentProjectDisplayName(
      createEmptyAgentSidebarPreferences(),
      roots[0],
      "B"
    );

    const projected = projectRootsWithDisplayNames(roots, preferences);
    expect(projected.map((item) => item.path)).toEqual(roots.map((item) => item.path));
    expect(projected.map((item) => item.name)).toEqual(roots.map((item) => item.name));
    expect(projected.map((item) => item.lastUsedMs)).toEqual([20, 10, 5]);
    expect(projected.map((item) => item.displayName)).toEqual(["B", "alpha", "gamma"]);
    expect(roots.some((item) => "displayName" in item)).toBe(false);
  });

  test("toggles only the exact canonical path without mutating prior state", () => {
    const empty = createEmptyAgentSidebarPreferences();
    const collapsed = toggleAgentProjectCollapsed(empty, "/Project");
    const alsoCollapsed = toggleAgentProjectCollapsed(collapsed, "/project");
    const expandedAgain = toggleAgentProjectCollapsed(alsoCollapsed, "/Project");

    expect(empty.collapsedProjectRoots).toEqual(new Set());
    expect(collapsed.collapsedProjectRoots).toEqual(new Set(["/Project"]));
    expect(alsoCollapsed.collapsedProjectRoots).toEqual(new Set(["/Project", "/project"]));
    expect(expandedAgain.collapsedProjectRoots).toEqual(new Set(["/project"]));
  });
});
