import type { AgentMcpConnectionError } from "./agentRuntimeService";

const HTTP_UNREACHABLE_MESSAGE =
  "Could not reach the Streamable HTTP endpoint. Check that the server is running and the URL is correct.";
const AUTHENTICATION_MESSAGE =
  "The server rejected the connection. Check its authentication headers.";
const TIMEOUT_MESSAGE = "The connection timed out. Check that the server is running and reachable.";
const COMMAND_MESSAGE =
  "Could not start the STDIO command. Check the executable path and arguments.";
const GENERIC_CONNECTION_MESSAGE =
  "Connection failed. Check the server configuration and Maple logs.";
const MCP_EVENT_PREFIX = "Some MCP servers could not connect:";

export function mcpConnectionErrorMessage(
  errors?: AgentMcpConnectionError[] | null
): string | null {
  if (!errors?.length) return null;
  const visibleErrors = errors
    .slice(0, 3)
    .map(({ name, error }) => `${name}: ${conciseMcpError(error)}`);
  const remainingCount = errors.length - visibleErrors.length;
  return `Some MCP servers could not connect: ${visibleErrors.join("; ")}${
    remainingCount > 0 ? `; and ${remainingCount} more` : ""
  }`;
}

export function userFacingAgentError(message: string): string {
  const singleLine = message.replace(/\s+/g, " ").trim();
  if (!isMcpConnectionErrorEvent(singleLine)) return agentTaskErrorMessage(message);

  const detail = singleLine.slice(MCP_EVENT_PREFIX.length).trim();
  return `Some MCP servers could not connect. ${conciseMcpError(detail)}`;
}

export function agentTaskErrorMessage(message: string): string {
  const isMapleTaskError = [
    "Agent task",
    "Failed to create Agent task",
    "Failed to list Agent tasks",
    "Failed to load Agent task",
    "Failed to reload Agent task",
    "Failed to find Agent task",
    "Failed to delete Agent task",
    "Failed to inspect cancelled Agent task",
    "Failed to restore cancelled Agent task",
    "Failed to name Agent task",
    "Failed to load named Agent task",
    "Failed to load updated Agent task",
    "Failed to load Agent for task",
    "Failed to save task MCP settings",
    "Goose reply failed:",
    "Goose stream failed:"
  ].some((prefix) => message.startsWith(prefix));

  if (!isMapleTaskError) return message;

  return message
    .replace(/\bSession not found\b/g, "Task not found")
    .replace(/\bsession not found\b/g, "task not found")
    .replace(/\bCreate a new session\b/g, "Create a new task")
    .replace(/\bcreate a new session\b/g, "create a new task")
    .replace(/\bStart a new session\b/g, "Start a new task")
    .replace(/\bstart a new session\b/g, "start a new task");
}

export function isMcpConnectionErrorEvent(message: string): boolean {
  return message.replace(/\s+/g, " ").trim().startsWith(MCP_EVENT_PREFIX);
}

export function conciseMcpError(error: string): string {
  const singleLine = error.replace(/\s+/g, " ").trim();
  const normalized = singleLine.toLowerCase();

  if (/\b(?:401|403)\b|unauthori[sz]ed|forbidden/.test(normalized)) {
    return AUTHENTICATION_MESSAGE;
  }
  if (/timed? out|timeout/.test(normalized)) {
    return TIMEOUT_MESSAGE;
  }
  if (
    /no such file or directory|failed to spawn|unable to spawn|executable (?:was )?not found/.test(
      normalized
    )
  ) {
    return COMMAND_MESSAGE;
  }
  if (
    /error sending request for url|connection refused|failed to connect|tcp connect|connection reset/.test(
      normalized
    )
  ) {
    return HTTP_UNREACHABLE_MESSAGE;
  }

  const cleaned = singleLine.replace(/^failed to initialize mcp client:\s*/i, "");
  if (!cleaned || /rmcp::|workertransport|transport\s*\[/i.test(cleaned)) {
    return GENERIC_CONNECTION_MESSAGE;
  }
  return cleaned.length > 180 ? `${cleaned.slice(0, 177)}…` : cleaned;
}
