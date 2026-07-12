export type SettingsBackTarget = { type: "history"; delta: number } | { type: "root" };

export function isSettingsRootPath(pathname: string) {
  return pathname === "/settings" || pathname === "/settings/";
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
