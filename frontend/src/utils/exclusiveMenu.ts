export function applyExclusiveMenuChange(
  closingKeys: Set<string>,
  replacedKeys: Set<string>,
  currentKey: string | null,
  changedKey: string,
  open: boolean
): string | null {
  if (!open) {
    closingKeys.add(changedKey);
    return currentKey === changedKey ? null : currentKey;
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

  return changedKey;
}

export function consumeReplacedMenuFocusRestore(
  closingKeys: Set<string>,
  replacedKeys: Set<string>,
  closingKey: string
): boolean {
  closingKeys.delete(closingKey);
  return replacedKeys.delete(closingKey);
}
