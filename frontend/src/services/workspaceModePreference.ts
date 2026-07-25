export type WorkspaceMode = "chat" | "agent";

export const WORKSPACE_MODE_STORAGE_KEY = "workspaceMode";

type WorkspaceModeStorage = Pick<Storage, "getItem" | "setItem">;

type LaunchLocation = {
  pathname: string;
  search: string;
  hash: string;
  agentModeAvailable: boolean;
};

function getBrowserStorage(): WorkspaceModeStorage | null {
  if (typeof window === "undefined") return null;

  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

export function getStoredWorkspaceMode(
  storage: WorkspaceModeStorage | null = getBrowserStorage()
): WorkspaceMode {
  if (!storage) return "chat";

  try {
    return storage.getItem(WORKSPACE_MODE_STORAGE_KEY) === "agent" ? "agent" : "chat";
  } catch {
    return "chat";
  }
}

export function rememberWorkspaceMode(
  mode: WorkspaceMode,
  storage: WorkspaceModeStorage | null = getBrowserStorage()
): void {
  if (!storage) return;

  try {
    storage.setItem(WORKSPACE_MODE_STORAGE_KEY, mode);
  } catch {
    // A storage failure should never prevent the user from changing modes.
  }
}

export function resetWorkspaceModePreference(
  storage: WorkspaceModeStorage | null = getBrowserStorage()
): void {
  rememberWorkspaceMode("chat", storage);
}

export function getLaunchWorkspacePath(
  location: LaunchLocation,
  storage: WorkspaceModeStorage | null = getBrowserStorage()
): "/agent" | null {
  if (
    !location.agentModeAvailable ||
    location.pathname !== "/" ||
    location.search !== "" ||
    location.hash !== ""
  ) {
    return null;
  }

  return getStoredWorkspaceMode(storage) === "agent" ? "/agent" : null;
}

export function restoreWorkspaceModeAtLaunch(agentModeAvailable: boolean): void {
  if (typeof window === "undefined") return;

  const path = getLaunchWorkspacePath({
    pathname: window.location.pathname,
    search: window.location.search,
    hash: window.location.hash,
    agentModeAvailable
  });
  if (!path) return;

  try {
    window.history.replaceState(window.history.state, "", path);
  } catch {
    // Keep the default home route if this webview does not allow history replacement.
  }
}
