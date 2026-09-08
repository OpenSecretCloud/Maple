export type AgentTaskRenameValidation = { ok: true; title: string } | { ok: false; error: string };

export function validateAgentTaskRename(
  title: string,
  currentTitle: string
): AgentTaskRenameValidation {
  const trimmedTitle = title.trim();
  if (!trimmedTitle) {
    return { ok: false, error: "Task title cannot be empty." };
  }
  if (trimmedTitle === currentTitle.trim()) {
    return { ok: false, error: "Please enter a different title." };
  }
  return { ok: true, title: trimmedTitle };
}
