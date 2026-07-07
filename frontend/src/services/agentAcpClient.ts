import {
  ClientSideConnection,
  PROTOCOL_VERSION,
  type Client,
  type ContentBlock,
  type InitializeResponse,
  type RequestPermissionRequest,
  type RequestPermissionResponse,
  type SessionNotification,
  type Stream
} from "@agentclientprotocol/sdk";

export type PermissionDecision = "allow_once" | "allow_always" | "reject_once" | "reject_always";

export interface PermissionRequestHandle {
  id: string;
  request: RequestPermissionRequest;
  decide: (decision: PermissionDecision) => void;
  cancel: () => void;
}

export interface AgentAcpDiagnostic {
  phase:
    | "connect:start"
    | "connect:open"
    | "connect:initialized"
    | "connect:error"
    | "connect:close"
    | "message:malformed";
  message: string;
  url?: string;
  readyState?: number;
  closeCode?: number;
  closeReason?: string;
  wasClean?: boolean;
}

export interface AgentAcpClientCallbacks {
  onSessionUpdate: (notification: SessionNotification) => void;
  onPermissionRequest: (request: PermissionRequestHandle) => void;
  onDiagnostic?: (diagnostic: AgentAcpDiagnostic) => void;
  onExtensionNotification?: (method: string, params: Record<string, unknown>) => void;
  onClosed?: () => void;
}

export interface ConnectedAgentAcpClient {
  initializeResponse: InitializeResponse;
  client: AgentAcpClient;
}

type ClosableAcpStream = Stream & {
  close: () => void;
};

export class AgentAcpClient {
  private connection: ClientSideConnection | null = null;
  private stream: ClosableAcpStream | null = null;
  private readonly callbacks: AgentAcpClientCallbacks;

  constructor(callbacks: AgentAcpClientCallbacks) {
    this.callbacks = callbacks;
  }

  async connect(acpUrl: string): Promise<ConnectedAgentAcpClient> {
    this.close();

    const redactedUrl = redactAcpUrl(acpUrl);
    this.callbacks.onDiagnostic?.({
      phase: "connect:start",
      message: `Opening ACP WebSocket ${redactedUrl}`,
      url: redactedUrl
    });

    const stream = createWebSocketStream(acpUrl, (diagnostic) => {
      this.callbacks.onDiagnostic?.(diagnostic);
    });
    const acpClient = this.createClientCallbacks();
    const connection = new ClientSideConnection(() => acpClient, stream);
    this.stream = stream;
    this.connection = connection;

    connection.closed
      .then(() => this.callbacks.onClosed?.())
      .catch(() => this.callbacks.onClosed?.());

    try {
      const initializeResponse = await connection.initialize({
        protocolVersion: PROTOCOL_VERSION,
        clientCapabilities: {
          _meta: {
            maple: { agentMode: true },
            goose: { customNotifications: true }
          }
        },
        clientInfo: {
          name: "Maple Agent Mode",
          version: "0.1.0"
        }
      });

      this.callbacks.onDiagnostic?.({
        phase: "connect:initialized",
        message: `ACP initialize completed for ${redactedUrl}`,
        url: redactedUrl
      });

      return { initializeResponse, client: this };
    } catch (error) {
      this.callbacks.onDiagnostic?.({
        phase: "connect:error",
        message: `ACP initialize failed for ${redactedUrl}: ${errorMessage(error)}`,
        url: redactedUrl
      });
      this.close();
      throw error;
    }
  }

  async newSession(cwd: string): Promise<string> {
    const connection = this.requireConnection();
    const response = await connection.newSession({ cwd, mcpServers: [] });
    return response.sessionId;
  }

  async prompt(sessionId: string, text: string): Promise<void> {
    const connection = this.requireConnection();
    const prompt: ContentBlock[] = [{ type: "text", text }];
    await connection.prompt({
      sessionId,
      messageId: crypto.randomUUID(),
      prompt
    });
  }

  async cancel(sessionId: string): Promise<void> {
    await this.requireConnection().cancel({ sessionId });
  }

  close(): void {
    this.stream?.close();
    this.stream = null;
    this.connection = null;
  }

  private requireConnection(): ClientSideConnection {
    if (!this.connection) {
      throw new Error("ACP client is not connected");
    }
    return this.connection;
  }

  private createClientCallbacks(): Client {
    return {
      requestPermission: (request) => this.handlePermissionRequest(request),
      sessionUpdate: async (notification) => {
        this.callbacks.onSessionUpdate(notification);
      },
      extNotification: async (method, params) => {
        this.callbacks.onExtensionNotification?.(method, params);
        if (method === "_goose/unstable/session/update" && isSessionNotification(params)) {
          this.callbacks.onSessionUpdate(params);
        }
      }
    };
  }

  private async handlePermissionRequest(
    request: RequestPermissionRequest
  ): Promise<RequestPermissionResponse> {
    return await new Promise<RequestPermissionResponse>((resolve) => {
      const id = crypto.randomUUID();
      const finish = (response: RequestPermissionResponse) => resolve(response);

      this.callbacks.onPermissionRequest({
        id,
        request,
        decide: (decision) => {
          const option = request.options.find((candidate) => candidate.kind === decision);
          finish({
            outcome: option
              ? { outcome: "selected", optionId: option.optionId }
              : { outcome: "cancelled" }
          });
        },
        cancel: () => finish({ outcome: { outcome: "cancelled" } })
      });
    });
  }
}

function isSessionNotification(value: unknown): value is SessionNotification {
  if (!value || typeof value !== "object") return false;
  const candidate = value as { sessionId?: unknown; update?: unknown };
  return typeof candidate.sessionId === "string" && typeof candidate.update === "object";
}

function createWebSocketStream(
  wsUrl: string,
  onDiagnostic: (diagnostic: AgentAcpDiagnostic) => void
): ClosableAcpStream {
  const ws = new WebSocket(wsUrl);
  const redactedUrl = redactAcpUrl(wsUrl);
  const incoming: unknown[] = [];
  const waiters: Array<() => void> = [];
  let closed = false;

  const waitForMessage = (): Promise<void> => {
    if (incoming.length > 0 || closed) {
      return Promise.resolve();
    }
    return new Promise<void>((resolve) => waiters.push(resolve));
  };

  const openPromise = new Promise<void>((resolve, reject) => {
    ws.addEventListener(
      "open",
      () => {
        onDiagnostic({
          phase: "connect:open",
          message: `ACP WebSocket opened ${redactedUrl}`,
          url: redactedUrl,
          readyState: ws.readyState
        });
        resolve();
      },
      { once: true }
    );
    ws.addEventListener(
      "error",
      () => {
        onDiagnostic({
          phase: "connect:error",
          message: `ACP WebSocket error for ${redactedUrl}`,
          url: redactedUrl,
          readyState: ws.readyState
        });
        reject(new Error(`ACP WebSocket connection failed for ${redactedUrl}`));
      },
      { once: true }
    );
  });

  const wakeWaiters = () => {
    closed = true;
    for (const waiter of waiters) waiter();
    waiters.length = 0;
  };

  ws.addEventListener("message", (event) => {
    if (typeof event.data !== "string") return;
    try {
      incoming.push(JSON.parse(event.data));
      waiters.shift()?.();
    } catch {
      onDiagnostic({
        phase: "message:malformed",
        message: "Ignored malformed ACP transport message",
        url: redactedUrl,
        readyState: ws.readyState
      });
    }
  });
  ws.addEventListener("close", (event) => {
    onDiagnostic({
      phase: "connect:close",
      message: `ACP WebSocket closed for ${redactedUrl} code=${event.code} clean=${event.wasClean}`,
      url: redactedUrl,
      readyState: ws.readyState,
      closeCode: event.code,
      closeReason: event.reason || undefined,
      wasClean: event.wasClean
    });
    wakeWaiters();
  });
  ws.addEventListener("error", wakeWaiters);

  const readable = new ReadableStream({
    async pull(controller) {
      await waitForMessage();
      while (incoming.length > 0) {
        controller.enqueue(incoming.shift());
      }
      if (closed && incoming.length === 0) {
        controller.close();
      }
    }
  });

  const writable = new WritableStream({
    async write(message) {
      await openPromise;
      ws.send(JSON.stringify(message));
    },
    close() {
      ws.close();
    },
    abort() {
      ws.close();
    }
  });

  return {
    readable,
    writable,
    close: () => ws.close()
  };
}

function redactAcpUrl(wsUrl: string): string {
  try {
    const url = new URL(wsUrl);
    if (url.searchParams.has("token")) {
      url.searchParams.set("token", "REDACTED");
    }
    return url.toString();
  } catch {
    return wsUrl.replace(/([?&]token=)[^&]+/i, "$1REDACTED");
  }
}

function errorMessage(error: unknown): string {
  if (error instanceof Error) return error.message;
  return String(error);
}
