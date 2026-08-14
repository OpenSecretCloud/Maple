export function nextExclusiveMenuKey(
  currentKey: string | null,
  changedKey: string,
  open: boolean
): string | null {
  if (open) return changedKey;
  return currentKey === changedKey ? null : currentKey;
}

export function conversationMenuKey(conversationId: string): string {
  return `chat:${conversationId}`;
}

export function projectMenuKey(projectId: string): string {
  return `project-menu:${projectId}`;
}

export function trackExclusiveMenuFocusChange(
  closingKeys: Set<string>,
  replacedKeys: Set<string>,
  currentKey: string | null,
  changedKey: string,
  open: boolean
): void {
  if (!open) {
    closingKeys.add(changedKey);
    return;
  }

  // Reopening the same Root cancels Radix Presence's pending unmount, so its prior
  // close-auto-focus callback will never consume the stale tracking entry.
  closingKeys.delete(changedKey);
  replacedKeys.delete(changedKey);

  if (currentKey !== null && currentKey !== changedKey) {
    replacedKeys.add(currentKey);
  }
  for (const closingKey of closingKeys) {
    replacedKeys.add(closingKey);
  }
}

export function consumeReplacedMenuFocusRestore(
  closingKeys: Set<string>,
  replacedKeys: Set<string>,
  closingKey: string
): boolean {
  closingKeys.delete(closingKey);
  return replacedKeys.delete(closingKey);
}
