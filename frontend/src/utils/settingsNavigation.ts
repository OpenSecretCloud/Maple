type SettingsBackTarget = { type: "history"; delta: number } | { type: "root" };

type CompactSettingsCloseOptions = {
  interactive: boolean;
  hasHomeParent: boolean;
  animate: () => Promise<void>;
  canCommit: () => boolean;
  goBack: () => void;
  replaceWithHome: () => void | Promise<void>;
};

export const SETTINGS_HOME_PARENT_STATE_KEY = "__mapleSettingsHomeParent";
export const SETTINGS_SHELL_POP_EVENT = "maple:settings-shell-pop";
export const SETTINGS_SHELL_POP_CANCEL_EVENT = "maple:settings-shell-pop-cancel";
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

/**
 * Finish compact Settings navigation without exposing a native history entry
 * while the shell is still animating. Interactive swipes have already settled
 * visually, so only button-driven closes need the explicit pop animation.
 */
export async function closeCompactSettings({
  interactive,
  hasHomeParent,
  animate,
  canCommit,
  goBack,
  replaceWithHome
}: CompactSettingsCloseOptions) {
  if (!interactive) await animate();
  if (!canCommit()) return false;

  if (hasHomeParent) {
    goBack();
    return true;
  }

  await replaceWithHome();
  return true;
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

export function shouldSuspendCoveredHomeChats({
  isSettingsRoute,
  hasSettingsShellEntered,
  isSettingsPopping,
  isSettingsShellSwipeActive
}: {
  isSettingsRoute: boolean;
  hasSettingsShellEntered: boolean;
  isSettingsPopping: boolean;
  isSettingsShellSwipeActive: boolean;
}) {
  return (
    isSettingsRoute && hasSettingsShellEntered && !isSettingsPopping && !isSettingsShellSwipeActive
  );
}

export function settingsMenuOwnsDocumentCanvas({
  compact,
  isSettingsRoot,
  isSettingsDetailEntering
}: {
  compact: boolean;
  isSettingsRoot: boolean;
  isSettingsDetailEntering: boolean;
}) {
  return compact && (isSettingsRoot || isSettingsDetailEntering);
}
