import { describe, expect, test } from "bun:test";
import {
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

  test("keeps the server name in structured session errors", () => {
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

  test("leaves unrelated Agent errors unchanged", () => {
    expect(userFacingAgentError("The selected model is unavailable")).toBe(
      "The selected model is unavailable"
    );
    expect(isMcpConnectionErrorEvent("The selected model is unavailable")).toBe(false);
  });
});
