import type { AgentMcpServer } from "./agentRuntimeService";

// Mirror Goose's pinned name_to_key behavior so form feedback and pending-chat
// reconciliation use the same identity as the authoritative Rust boundary.
export function gooseMcpServerKey(name: string): string {
  let key = "";
  for (const character of name) {
    if (/[A-Za-z0-9_-]/.test(character)) {
      key += character;
    } else if (/\p{White_Space}/u.test(character)) {
      continue;
    } else {
      key += "_";
    }
  }
  return key.toLowerCase();
}

export function isValidMcpTimeoutSeconds(value: number): boolean {
  return Number.isSafeInteger(value) && value >= 1;
}

export function reconcileNewChatMcpServerNames(
  previousServers: AgentMcpServer[],
  savedServers: AgentMcpServer[],
  currentSelection: Set<string>
): Set<string> {
  const previousByKey = new Map(
    previousServers.map((server) => [gooseMcpServerKey(server.name), server])
  );
  const selectedKeys = new Set(Array.from(currentSelection, (name) => gooseMcpServerKey(name)));
  const reconciled = new Set<string>();

  for (const server of savedServers) {
    const key = gooseMcpServerKey(server.name);
    const previous = previousByKey.get(key);
    const wasSelected = selectedKeys.has(key);
    const shouldSelect = previous
      ? wasSelected === previous.enabled
        ? server.enabled
        : wasSelected
      : server.enabled;

    if (shouldSelect) reconciled.add(server.name);
  }

  return reconciled;
}
