export type AgentToolKind = "shell" | "file-read" | "file-write" | "web" | "mcp" | "generic";

const SHELL_TOOL_NAMES = new Set(["shell"]);
const FILE_READ_TOOL_NAMES = new Set([
  "read",
  "read_file",
  "read_image",
  "list_files",
  "glob",
  "grep"
]);
const FILE_WRITE_TOOL_NAMES = new Set([
  "edit",
  "write",
  "edit_file",
  "write_file",
  "text_editor",
  "str_replace_editor",
  "str_replace_based_edit_tool",
  "apply_patch"
]);
const WEB_TOOL_NAMES = new Set(["web_search", "open_url"]);
const MAPLE_EXTENSION_NAMES = new Set(["developer"]);
const SHELL_TOOL_LABELS = new Set(["terminal", "shell"]);
const FILE_READ_TOOL_LABELS = new Set([
  "read",
  "read file",
  "read image",
  "list files",
  "find files",
  "search"
]);
const FILE_WRITE_TOOL_LABELS = new Set(["edit", "write", "editor", "edit file", "write file"]);
const WEB_TOOL_LABELS = new Set(["web search", "open url"]);

function toolNameFromTimelineId(id: string): string | null {
  const encodedName = id.startsWith("functions.") ? id.slice("functions.".length) : null;
  if (!encodedName) return null;

  const sequenceSeparator = encodedName.lastIndexOf(":");
  const name = sequenceSeparator >= 0 ? encodedName.slice(0, sequenceSeparator) : encodedName;
  return name.trim() || null;
}

function agentToolKindFromTitle(title: string | null | undefined): AgentToolKind {
  const label = title?.split(":", 1)[0]?.trim().toLowerCase();
  if (!label) return "generic";
  if (SHELL_TOOL_LABELS.has(label)) return "shell";
  if (FILE_READ_TOOL_LABELS.has(label)) return "file-read";
  if (FILE_WRITE_TOOL_LABELS.has(label)) return "file-write";
  if (WEB_TOOL_LABELS.has(label)) return "web";
  return "generic";
}

export function agentToolKind(id: string, title?: string | null): AgentToolKind {
  const encodedName = toolNameFromTimelineId(id);
  if (!encodedName) return agentToolKindFromTitle(title);

  const namespaceSeparator = encodedName.indexOf("__");
  const namespace = namespaceSeparator >= 0 ? encodedName.slice(0, namespaceSeparator) : null;
  const toolName =
    namespaceSeparator >= 0 ? encodedName.slice(namespaceSeparator + 2) : encodedName;

  if (namespace && !MAPLE_EXTENSION_NAMES.has(namespace)) return "mcp";
  if (SHELL_TOOL_NAMES.has(toolName)) return "shell";
  if (FILE_READ_TOOL_NAMES.has(toolName)) return "file-read";
  if (FILE_WRITE_TOOL_NAMES.has(toolName)) return "file-write";
  if (WEB_TOOL_NAMES.has(toolName)) return "web";
  return "generic";
}

export function agentToolKindLabel(kind: AgentToolKind): string {
  switch (kind) {
    case "shell":
      return "Shell command";
    case "file-read":
      return "File read";
    case "file-write":
      return "File change";
    case "web":
      return "Web tool";
    case "mcp":
      return "MCP tool";
    default:
      return "Tool call";
  }
}
