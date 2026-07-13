export type SettingsBackTarget = { type: "history"; delta: number } | { type: "root" };

export const SETTINGS_HOME_PARENT_STATE_KEY = "__mapleSettingsHomeParent";
export const SETTINGS_SHELL_POP_EVENT = "maple:settings-shell-pop";
export const SETTINGS_SHELL_SWIPE_BACK_EVENT = "maple:settings-shell-swipe-back";

export function isSettingsRootPath(pathname: string) {
  return pathname === "/settings" || pathname === "/settings/";
}

export function isSettingsPath(pathname: string) {
  return isSettingsRootPath(pathname) || pathname.startsWith("/settings/");
}

export function hasSettingsHomeParent(state: unknown) {
  return (
    !!state &&
    typeof state === "object" &&
    (state as Record<string, unknown>)[SETTINGS_HOME_PARENT_STATE_KEY] === true
  );
}

export function getSettingsBackTarget(
  currentHistoryIndex: unknown,
  rootHistoryIndex: number | null
): SettingsBackTarget {
  if (
    typeof currentHistoryIndex === "number" &&
    Number.isFinite(currentHistoryIndex) &&
    rootHistoryIndex !== null &&
    currentHistoryIndex > rootHistoryIndex
  ) {
    return { type: "history", delta: rootHistoryIndex - currentHistoryIndex };
  }

  return { type: "root" };
}

export function shouldAnimateSettingsPop({
  compact,
  currentPathname,
  nextPathname,
  action
}: {
  compact: boolean;
  currentPathname: string;
  nextPathname: string;
  action: string;
}) {
  return (
    compact &&
    !isSettingsRootPath(currentPathname) &&
    isSettingsRootPath(nextPathname) &&
    (action === "BACK" || action === "GO")
  );
}
