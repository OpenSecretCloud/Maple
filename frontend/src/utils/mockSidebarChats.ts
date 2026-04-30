import type { Conversation } from "@opensecret/react";

/** Prefix for fake sidebar rows (not real API conversations). */
export const MOCK_SIDEBAR_CHAT_ID_PREFIX = "__maple_dev_mock_chat__";

export function isMockSidebarChatId(id: string): boolean {
  return id.startsWith(MOCK_SIDEBAR_CHAT_ID_PREFIX);
}

/**
 * In dev, prepends fake “recent” chats so the sidebar can be reviewed with a full list.
 * Set `VITE_MOCK_SIDEBAR_CHAT_COUNT=0` in `frontend/.env.local` to turn off.
 */
export function getMockSidebarChatCount(): number {
  if (!import.meta.env.DEV) return 0;
  const raw = import.meta.env.VITE_MOCK_SIDEBAR_CHAT_COUNT;
  if (raw === "0" || raw === "false") return 0;
  if (raw !== undefined && raw !== "") {
    const n = parseInt(raw, 10);
    if (Number.isFinite(n)) return Math.min(Math.max(0, n), 100);
  }
  return 32;
}

const SUBJECT_ROTATION = [
  "Notes on API pagination",
  "Draft email to the team",
  "Rust error E0382 walkthrough",
  "Trip itinerary ideas",
  "Recipe tweaks (sourdough)",
  "Meeting summary — Q2 planning",
  "Regex for log parsing",
  "Tailwind spacing audit",
  "SQLite migration checklist",
  "Design feedback: sidebar density",
  "Tauri IPC sketch",
  "Billing edge cases",
  "iOS keyboard avoidance",
  "Dark mode token review",
  "Copy for empty states",
  "Performance: list virtualization",
  "Accessibility: focus rings",
  "Error copy for rate limits",
  "Onboarding checklist",
  "Webhook retry policy"
];

export function buildMockSidebarConversations(count: number): Conversation[] {
  if (count <= 0) return [];
  const nowSec = Math.floor(Date.now() / 1000);
  return Array.from({ length: count }, (_, i) => {
    const n = i + 1;
    const subject = SUBJECT_ROTATION[i % SUBJECT_ROTATION.length];
    const pass = Math.floor(i / SUBJECT_ROTATION.length);
    const title = pass === 0 ? subject : `${subject} (${pass + 1})`;
    return {
      id: `${MOCK_SIDEBAR_CHAT_ID_PREFIX}${n}`,
      object: "conversation",
      created_at: nowSec - n * 3600,
      metadata: { title },
      project_id: null,
      pinned: false,
      last_activity_at: nowSec - n * 90
    };
  });
}
