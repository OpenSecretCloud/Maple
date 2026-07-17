import { describe, expect, test } from "bun:test";
import {
  agentTaskErrorMessage,
  conciseMcpError,
  isMcpConnectionErrorEvent,
  mcpConnectionErrorMessage,
  userFacingAgentError
} from "./agentMcpErrors";

const RAW_HTTP_ERROR =
  "failed to initialize MCP client: Send message error Transport [rmcp::transport::worker::WorkerTransport<rmcp::transport::streamable_http_client::StreamableHttpClientTransport<reqwest::async_impl::client::Client>>] error: Client error: error sending request for url (http://127.0.0.1:33001/mcp), when send initialize request";

describe("MCP connection errors", () => {
  test("replaces HTTP transport internals with an actionable message", () => {
    const message = conciseMcpError(RAW_HTTP_ERROR);

    expect(message).toBe(
      "Could not reach the Streamable HTTP endpoint. Check that the server is running and the URL is correct."
    );
    expect(message).not.toContain("rmcp");
    expect(message).not.toContain("WorkerTransport");
  });

  test("keeps the server name in structured task errors", () => {
    const message = mcpConnectionErrorMessage([{ name: "fixture_http", error: RAW_HTTP_ERROR }]);

    expect(message).toContain("fixture_http");
    expect(message).toContain("Could not reach the Streamable HTTP endpoint");
    expect(message).not.toContain("rmcp");
  });

  test("sanitizes the unstructured runtime event path", () => {
    const message = userFacingAgentError(
      `Some MCP servers could not connect: fixture_http: ${RAW_HTTP_ERROR}`
    );

    expect(message).toContain("Could not reach the Streamable HTTP endpoint");
    expect(message).not.toContain("rmcp");
    expect(message).not.toContain("WorkerTransport");
  });

  test("preserves session terminology returned by an external MCP server", () => {
    expect(
      userFacingAgentError("Some MCP servers could not connect: calendar: Session not found")
    ).toBe("Some MCP servers could not connect. calendar: Session not found");
    expect(agentTaskErrorMessage("Failed to connect MCP server: Session not found")).toBe(
      "Failed to connect MCP server: Session not found"
    );
  });

  test("leaves unrelated Agent errors unchanged", () => {
    expect(userFacingAgentError("The selected model is unavailable")).toBe(
      "The selected model is unavailable"
    );
    expect(isMcpConnectionErrorEvent("The selected model is unavailable")).toBe(false);
  });

  test("translates known runtime session errors without rewriting unrelated text", () => {
    expect(userFacingAgentError("Failed to load Agent task: Session not found")).toBe(
      "Failed to load Agent task: Task not found"
    );
    expect(
      agentTaskErrorMessage("Goose stream failed: Please try again or create a new session.")
    ).toBe("Goose stream failed: Please try again or create a new task.");
    expect(agentTaskErrorMessage("MCP server session-cache failed")).toBe(
      "MCP server session-cache failed"
    );
  });

  test("classifies authentication, timeout, and STDIO startup failures", () => {
    expect(conciseMcpError("HTTP 401 Unauthorized")).toBe(
      "The server rejected the connection. Check its authentication headers."
    );
    expect(conciseMcpError("HTTP 403 Forbidden")).toBe(
      "The server rejected the connection. Check its authentication headers."
    );
    expect(conciseMcpError("request timed out during initialize")).toBe(
      "The connection timed out. Check that the server is running and reachable."
    );
    expect(conciseMcpError("failed to spawn process: executable not found")).toBe(
      "Could not start the STDIO command. Check the executable path and arguments."
    );
  });

  test("bounds aggregated errors and handles empty responses", () => {
    const message = mcpConnectionErrorMessage([
      { name: "first", error: "first failure" },
      { name: "second", error: "second failure" },
      { name: "third", error: "third failure" },
      { name: "fourth", error: "fourth failure" }
    ]);

    expect(message).toContain("first: first failure");
    expect(message).toContain("third: third failure");
    expect(message).not.toContain("fourth: fourth failure");
    expect(message).toContain("and 1 more");
    expect(mcpConnectionErrorMessage([])).toBeNull();
    expect(mcpConnectionErrorMessage(null)).toBeNull();
    expect(isMcpConnectionErrorEvent(message!)).toBe(true);
  });
});
