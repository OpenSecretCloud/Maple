import type { RecentProjectRoot } from "@/services/agentRuntimeService";

export const AGENT_SIDEBAR_PREFERENCES_VERSION = 1 as const;
export const AGENT_SIDEBAR_PREFERENCES_STORAGE_PREFIX = "maple:agent-sidebar-preferences:v1:";

export type AgentSidebarPreferencesStorage = Pick<Storage, "getItem" | "setItem">;

export interface AgentSidebarPreferences {
  collapsedProjectRoots: ReadonlySet<string>;
  projectDisplayNameOverrides: ReadonlyMap<string, string>;
}

export interface AgentSidebarPreferencesV1 {
  version: typeof AGENT_SIDEBAR_PREFERENCES_VERSION;
  collapsedProjectRoots: string[];
  projectDisplayNameOverrides: Array<{
    path: string;
    displayName: string;
  }>;
}

export type AgentProjectRootView<TRoot extends RecentProjectRoot = RecentProjectRoot> = TRoot & {
  displayName: string;
};

export function createEmptyAgentSidebarPreferences(): AgentSidebarPreferences {
  return {
    collapsedProjectRoots: new Set(),
    projectDisplayNameOverrides: new Map()
  };
}

export function agentSidebarPreferencesStorageKey(userId: string): string {
  if (!isNonBlankString(userId)) {
    throw new Error("Agent sidebar preferences require an authenticated user");
  }
  return `${AGENT_SIDEBAR_PREFERENCES_STORAGE_PREFIX}${encodeURIComponent(userId)}`;
}

export function loadAgentSidebarPreferences(
  userId: string,
  storage: AgentSidebarPreferencesStorage | null = getBrowserStorage()
): AgentSidebarPreferences {
  const empty = createEmptyAgentSidebarPreferences();
  if (!storage) return empty;

  let stored: string | null;
  try {
    stored = storage.getItem(agentSidebarPreferencesStorageKey(userId));
  } catch {
    return empty;
  }
  if (!stored) return empty;

  let parsed: unknown;
  try {
    parsed = JSON.parse(stored);
  } catch {
    return empty;
  }
  if (!isRecord(parsed) || parsed.version !== AGENT_SIDEBAR_PREFERENCES_VERSION) {
    return empty;
  }

  const collapsedProjectRoots = new Set<string>();
  if (Array.isArray(parsed.collapsedProjectRoots)) {
    for (const path of parsed.collapsedProjectRoots) {
      if (isNonBlankString(path)) collapsedProjectRoots.add(path);
    }
  }

  const projectDisplayNameOverrides = new Map<string, string>();
  if (Array.isArray(parsed.projectDisplayNameOverrides)) {
    for (const entry of parsed.projectDisplayNameOverrides) {
      if (
        !isRecord(entry) ||
        !isNonBlankString(entry.path) ||
        typeof entry.displayName !== "string"
      ) {
        continue;
      }
      const displayName = entry.displayName.trim();
      if (displayName) projectDisplayNameOverrides.set(entry.path, displayName);
    }
  }

  return { collapsedProjectRoots, projectDisplayNameOverrides };
}

export function saveAgentSidebarPreferences(
  userId: string,
  preferences: AgentSidebarPreferences,
  storage: AgentSidebarPreferencesStorage | null = getBrowserStorage()
): boolean {
  if (!storage) return false;

  const collapsedProjectRoots = new Set<string>();
  for (const path of preferences.collapsedProjectRoots) {
    if (isNonBlankString(path)) collapsedProjectRoots.add(path);
  }

  const projectDisplayNameOverrides = new Map<string, string>();
  for (const [path, candidate] of preferences.projectDisplayNameOverrides) {
    if (!isNonBlankString(path) || typeof candidate !== "string") continue;
    const displayName = candidate.trim();
    if (displayName) projectDisplayNameOverrides.set(path, displayName);
  }

  const serialized: AgentSidebarPreferencesV1 = {
    version: AGENT_SIDEBAR_PREFERENCES_VERSION,
    collapsedProjectRoots: [...collapsedProjectRoots],
    projectDisplayNameOverrides: [...projectDisplayNameOverrides].map(([path, displayName]) => ({
      path,
      displayName
    }))
  };

  try {
    storage.setItem(agentSidebarPreferencesStorageKey(userId), JSON.stringify(serialized));
    return true;
  } catch {
    return false;
  }
}

export function projectRootsWithDisplayNames<TRoot extends RecentProjectRoot>(
  roots: readonly TRoot[],
  preferences: AgentSidebarPreferences
): AgentProjectRootView<TRoot>[] {
  return roots.map((root) => ({
    ...root,
    displayName: projectDisplayName(root, preferences.projectDisplayNameOverrides)
  }));
}

export function toggleAgentProjectCollapsed(
  preferences: AgentSidebarPreferences,
  path: string
): AgentSidebarPreferences {
  assertProjectPath(path);
  const collapsedProjectRoots = new Set(preferences.collapsedProjectRoots);
  if (collapsedProjectRoots.has(path)) {
    collapsedProjectRoots.delete(path);
  } else {
    collapsedProjectRoots.add(path);
  }
  return {
    collapsedProjectRoots,
    projectDisplayNameOverrides: preferences.projectDisplayNameOverrides
  };
}

export function renameAgentProjectDisplayName(
  preferences: AgentSidebarPreferences,
  root: Pick<RecentProjectRoot, "path" | "name">,
  nextName: string
): AgentSidebarPreferences {
  assertProjectPath(root.path);
  const displayName = nextName.trim();
  if (!displayName) throw new Error("Project name cannot be empty.");

  const projectDisplayNameOverrides = new Map(preferences.projectDisplayNameOverrides);
  if (displayName === nativeProjectDisplayName(root)) {
    projectDisplayNameOverrides.delete(root.path);
  } else {
    projectDisplayNameOverrides.set(root.path, displayName);
  }

  return {
    collapsedProjectRoots: preferences.collapsedProjectRoots,
    projectDisplayNameOverrides
  };
}

function projectDisplayName(
  root: Pick<RecentProjectRoot, "path" | "name">,
  overrides: ReadonlyMap<string, string>
): string {
  const override = overrides.get(root.path);
  if (typeof override === "string" && override.trim()) return override.trim();
  return nativeProjectDisplayName(root);
}

function nativeProjectDisplayName(root: Pick<RecentProjectRoot, "path" | "name">): string {
  if (isNonBlankString(root.name)) return root.name;
  return basename(root.path);
}

function basename(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() || path;
}

function assertProjectPath(path: string): void {
  if (!isNonBlankString(path)) throw new Error("Project path cannot be empty.");
}

function isNonBlankString(value: unknown): value is string {
  return typeof value === "string" && value.trim().length > 0;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function getBrowserStorage(): AgentSidebarPreferencesStorage | null {
  if (typeof window === "undefined") return null;
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}
