import type { ToolActivityStatus } from "./toolPresentation";

type ChatToolOutputLike = {
  output: string;
  status?: string;
};

function parseToolArguments(argumentsText: string): Record<string, unknown> | null {
  try {
    const parsed = JSON.parse(argumentsText);
    return parsed && typeof parsed === "object" && !Array.isArray(parsed)
      ? (parsed as Record<string, unknown>)
      : null;
  } catch {
    return null;
  }
}

function stringArgument(
  argumentsValue: Record<string, unknown> | null,
  key: string
): string | null {
  const value = argumentsValue?.[key];
  return typeof value === "string" && value.trim() ? value : null;
}

function stringArrayArgument(
  argumentsValue: Record<string, unknown> | null,
  key: string
): string[] {
  const value = argumentsValue?.[key];
  if (!Array.isArray(value)) return [];
  return value.filter((item): item is string => typeof item === "string" && Boolean(item.trim()));
}

function numberArgument(
  argumentsValue: Record<string, unknown> | null,
  key: string
): number | null {
  const value = argumentsValue?.[key];
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

export function chatToolTitle(name: string, argumentsText: string): string {
  const argumentsValue = parseToolArguments(argumentsText);

  if (name === "web_search") {
    const query = stringArgument(argumentsValue, "query");
    return query ? `Web Search: "${query}"` : "Web Search";
  }

  if (name === "open_url") {
    const url = stringArgument(argumentsValue, "url");
    return url ? `Open URL: ${url}` : "Open URL";
  }

  if (name === "open_urls") {
    const urls = stringArrayArgument(argumentsValue, "urls");
    if (urls.length === 1) return `Open URL: ${urls[0]}`;
    if (urls.length > 1) return `Open URLs: ${urls.length} pages`;
    return "Open URLs";
  }

  if (name === "read_image") {
    const imageNumber = numberArgument(argumentsValue, "image_number");
    return imageNumber === null ? "Read image" : `Read image: Image ${imageNumber}`;
  }

  const toolName = name.trim();
  if (!toolName || toolName === "function") return "Tool call";
  const query = stringArgument(argumentsValue, "query");
  return query ? `${toolName}: "${query}"` : toolName;
}

export function formatChatToolArguments(argumentsText: string): string {
  const trimmed = argumentsText.trim();
  if (!trimmed) return "";

  try {
    return JSON.stringify(JSON.parse(trimmed), null, 2);
  } catch {
    return argumentsText;
  }
}

export function chatToolCallStatus(
  callStatus: string | undefined,
  relatedOutputs: readonly ChatToolOutputLike[]
): ToolActivityStatus {
  const statuses = [callStatus, ...relatedOutputs.map((output) => output.status)];
  if (statuses.some((status) => status === "error" || status === "failed")) return "error";
  if (statuses.includes("incomplete")) return "incomplete";
  if (relatedOutputs.some((output) => output.status === "completed")) return "completed";
  if (
    relatedOutputs.some(
      (output) => output.status === "in_progress" || output.status === "streaming"
    )
  ) {
    return "active";
  }
  if (relatedOutputs.length > 0) return "completed";
  return "active";
}

export function chatToolOutputStatus(status: string | undefined): ToolActivityStatus {
  if (status === "error" || status === "failed") return "error";
  if (status === "incomplete") return "incomplete";
  if (status === "in_progress" || status === "streaming") return "active";
  return "completed";
}

export function chatWebSearchStatus(status: string | undefined): ToolActivityStatus {
  if (status === "in_progress" || status === "searching") return "active";
  if (status === "completed") return "completed";
  if (status === "incomplete") return "incomplete";
  return "error";
}
