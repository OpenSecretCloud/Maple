import { describe, expect, test } from "bun:test";
import OpenAI from "openai";
import {
  AGENT_THOUGHT_LABEL_FALLBACK_DELAY_MS,
  AGENT_THOUGHT_LABEL_FALLBACK_TEXT,
  AGENT_THOUGHT_LABEL_MAX_LENGTH,
  AGENT_THOUGHT_LABEL_PENDING_TEXT,
  AGENT_THOUGHT_LABEL_REASONING_MAX_LENGTH,
  AGENT_THOUGHT_LABEL_USER_REQUEST_MAX_LENGTH,
  agentThoughtLabelStorageKey,
  buildAgentThoughtLabelInput,
  clearAgentThoughtLabelsForSession,
  clearAgentThoughtLabelsForTurn,
  clearAgentThoughtLabelsForUser,
  getAgentThoughtLabel,
  parseAgentThoughtLabel,
  requestAgentThoughtLabel,
  startAgentThoughtLabelDisplay,
  type AgentThoughtLabelClient,
  type AgentThoughtLabelStorage
} from "./agentThoughtLabels";

class MemoryStorage implements AgentThoughtLabelStorage {
  private readonly values = new Map<string, string>();

  get length(): number {
    return this.values.size;
  }

  getItem(key: string): string | null {
    return this.values.get(key) ?? null;
  }

  key(index: number): string | null {
    return Array.from(this.values.keys())[index] ?? null;
  }

  removeItem(key: string): void {
    this.values.delete(key);
  }

  setItem(key: string, value: string): void {
    this.values.set(key, value);
  }
}

interface FakeChatCompletion {
  choices: {
    message: { content: string | null; reasoning?: string | null };
  }[];
}

function fakeClient(
  create: (request: Record<string, unknown>) => Promise<FakeChatCompletion>
): AgentThoughtLabelClient {
  return { chat: { completions: { create } } } as unknown as AgentThoughtLabelClient;
}

function completion(content: string | null): FakeChatCompletion {
  return { choices: [{ message: { content } }] };
}

function deferred<T>(): {
  promise: Promise<T>;
  resolve: (value: T) => void;
} {
  let resolve: ((value: T) => void) | undefined;
  const promise = new Promise<T>((nextResolve) => {
    resolve = nextResolve;
  });
  return {
    promise,
    resolve: (value) => resolve?.(value)
  };
}

const identifiers = {
  userId: "user/account",
  sessionId: "task:one",
  phaseId: "assistant-after-user:thought-0"
};

describe("startAgentThoughtLabelDisplay", () => {
  test("uses a cached label without showing or requesting a pending state", () => {
    const commits: [string, string | undefined][] = [];
    let requestCount = 0;

    const cancel = startAgentThoughtLabelDisplay({
      cachedLabel: "Inspecting cached work",
      request: async () => {
        requestCount += 1;
        return "unused";
      },
      commit: (label, expectedLabel) => commits.push([label, expectedLabel])
    });

    expect(cancel).toBeNull();
    expect(requestCount).toBe(0);
    expect(commits).toEqual([["Inspecting cached work", undefined]]);
  });

  test("shows Thinking until a valid label arrives and cancels the fallback", async () => {
    const response = deferred<string | null>();
    const commits: [string, string | undefined][] = [];
    let scheduledDelay = 0;
    let fallbackCancelled = false;

    startAgentThoughtLabelDisplay({
      cachedLabel: null,
      request: () => response.promise,
      commit: (label, expectedLabel) => commits.push([label, expectedLabel]),
      scheduleFallback: (_callback, delayMs) => {
        scheduledDelay = delayMs;
        return () => {
          fallbackCancelled = true;
        };
      }
    });

    expect(commits).toEqual([[AGENT_THOUGHT_LABEL_PENDING_TEXT, undefined]]);
    expect(scheduledDelay).toBe(AGENT_THOUGHT_LABEL_FALLBACK_DELAY_MS);

    response.resolve("Reviewing authentication flow");
    await response.promise;
    expect(fallbackCancelled).toBe(true);
    expect(commits).toEqual([
      [AGENT_THOUGHT_LABEL_PENDING_TEXT, undefined],
      ["Reviewing authentication flow", undefined]
    ]);
  });

  test("falls back after the deadline and still accepts a late label", async () => {
    const response = deferred<string | null>();
    const commits: [string, string | undefined][] = [];
    let showFallback = () => {};

    startAgentThoughtLabelDisplay({
      cachedLabel: null,
      request: () => response.promise,
      commit: (label, expectedLabel) => commits.push([label, expectedLabel]),
      scheduleFallback: (callback) => {
        showFallback = callback;
        return () => {};
      }
    });

    showFallback();
    expect(commits).toEqual([
      [AGENT_THOUGHT_LABEL_PENDING_TEXT, undefined],
      [AGENT_THOUGHT_LABEL_FALLBACK_TEXT, AGENT_THOUGHT_LABEL_PENDING_TEXT]
    ]);

    response.resolve("Tracing a slower response");
    await response.promise;
    expect(commits.at(-1)).toEqual(["Tracing a slower response", undefined]);
  });

  test("shows Thought immediately on failure and ignores a cancelled request", async () => {
    const failedCommits: [string, string | undefined][] = [];
    startAgentThoughtLabelDisplay({
      cachedLabel: null,
      request: async () => null,
      commit: (label, expectedLabel) => failedCommits.push([label, expectedLabel]),
      scheduleFallback: () => () => {}
    });
    await Promise.resolve();
    expect(failedCommits).toEqual([
      [AGENT_THOUGHT_LABEL_PENDING_TEXT, undefined],
      [AGENT_THOUGHT_LABEL_FALLBACK_TEXT, undefined]
    ]);

    const response = deferred<string | null>();
    const cancelledCommits: [string, string | undefined][] = [];
    let showFallback = () => {};
    const cancel = startAgentThoughtLabelDisplay({
      cachedLabel: null,
      request: () => response.promise,
      commit: (label, expectedLabel) => cancelledCommits.push([label, expectedLabel]),
      scheduleFallback: (callback) => {
        showFallback = callback;
        return () => {};
      }
    });

    cancel?.();
    showFallback();
    response.resolve("Ignoring stale label");
    await response.promise;
    expect(cancelledCommits).toEqual([[AGENT_THOUGHT_LABEL_PENDING_TEXT, undefined]]);
  });
});

describe("parseAgentThoughtLabel", () => {
  test("trims and accepts a single line of at most 80 Unicode characters", () => {
    expect(parseAgentThoughtLabel("  Inspecting authentication flow  ")).toBe(
      "Inspecting authentication flow"
    );
    expect(parseAgentThoughtLabel("🍁".repeat(AGENT_THOUGHT_LABEL_MAX_LENGTH))).toBe(
      "🍁".repeat(AGENT_THOUGHT_LABEL_MAX_LENGTH)
    );
  });

  test("rejects empty, multiline, non-text, and overlong responses", () => {
    expect(parseAgentThoughtLabel("   ")).toBeNull();
    expect(parseAgentThoughtLabel("Inspecting code\nRunning tests")).toBeNull();
    expect(parseAgentThoughtLabel("Inspecting code\u2028Running tests")).toBeNull();
    expect(parseAgentThoughtLabel(42)).toBeNull();
    expect(parseAgentThoughtLabel("x".repeat(AGENT_THOUGHT_LABEL_MAX_LENGTH + 1))).toBeNull();
  });
});

describe("buildAgentThoughtLabelInput", () => {
  test("separates bounded overall task context from the current reasoning step", () => {
    const input = JSON.parse(
      buildAgentThoughtLabelInput({
        userRequest: `${"u".repeat(AGENT_THOUGHT_LABEL_USER_REQUEST_MAX_LENGTH)}USER_SECRET`,
        reasoningText: `REASONING_SECRET${"r".repeat(AGENT_THOUGHT_LABEL_REASONING_MAX_LENGTH)}`
      })
    ) as { overall_task_context: string; current_reasoning_step: string };

    expect(Array.from(input.overall_task_context)).toHaveLength(
      AGENT_THOUGHT_LABEL_USER_REQUEST_MAX_LENGTH
    );
    expect(Array.from(input.current_reasoning_step)).toHaveLength(
      AGENT_THOUGHT_LABEL_REASONING_MAX_LENGTH
    );
    expect(input.overall_task_context).not.toContain("USER_SECRET");
    expect(input.current_reasoning_step).not.toContain("REASONING_SECRET");
    expect(Object.keys(input).sort()).toEqual(["current_reasoning_step", "overall_task_context"]);
  });
});

describe("requestAgentThoughtLabel", () => {
  test("uses the installed OpenAI client to call the stateless chat endpoint", async () => {
    const storage = new MemoryStorage();
    let requestedUrl = "";
    let requestedBody: Record<string, unknown> | undefined;
    const client = new OpenAI({
      apiKey: "test-key",
      baseURL: "https://maple.test/v1/",
      fetch: async (input, init) => {
        requestedUrl = String(input);
        requestedBody = JSON.parse(String(init?.body)) as Record<string, unknown>;
        return new Response(
          JSON.stringify({
            id: "chatcmpl-test",
            object: "chat.completion",
            created: 0,
            model: "auto:quick",
            choices: [
              {
                index: 0,
                finish_reason: "stop",
                logprobs: null,
                message: {
                  role: "assistant",
                  content: "Inspecting authentication flow"
                }
              }
            ]
          }),
          { status: 200, headers: { "Content-Type": "application/json" } }
        );
      },
      maxRetries: 0
    });

    await expect(
      requestAgentThoughtLabel(
        client,
        { ...identifiers, phaseId: "sdk-contract:thought-0" },
        { userRequest: "Fix login", reasoningText: "Inspect auth." },
        storage
      )
    ).resolves.toBe("Inspecting authentication flow");

    expect(requestedUrl).toBe("https://maple.test/v1/chat/completions");
    expect(requestedBody).toMatchObject({ model: "auto:quick", stream: false });
    expect(requestedBody).not.toHaveProperty("conversation");
  });

  test("uses one stateless auto:quick chat request and saves only a valid label", async () => {
    const storage = new MemoryStorage();
    const requests: Record<string, unknown>[] = [];
    const client = fakeClient(async (request) => {
      requests.push(request);
      return completion("  Inspecting authentication flow  ");
    });

    const source = { userRequest: "Fix login", reasoningText: "I traced the auth state." };
    const firstRequest = requestAgentThoughtLabel(client, identifiers, source, storage);
    await expect(firstRequest).resolves.toBe("Inspecting authentication flow");
    await expect(requestAgentThoughtLabel(client, identifiers, source, storage)).resolves.toBe(
      "Inspecting authentication flow"
    );

    expect(requests).toHaveLength(1);
    expect(requests[0]).toMatchObject({
      model: "auto:quick",
      stream: false
    });
    expect(Object.keys(requests[0]).sort()).toEqual([
      "max_completion_tokens",
      "messages",
      "model",
      "reasoning_effort",
      "stream"
    ]);
    expect(requests[0].reasoning_effort).toBe("low");
    expect(requests[0].max_completion_tokens).toBe(1024);
    expect(requests[0]).not.toHaveProperty("max_tokens");
    const messages = requests[0].messages as { role: string; content: string }[];
    expect(messages).toEqual([
      {
        role: "system",
        content: expect.any(String)
      },
      {
        role: "user",
        content: buildAgentThoughtLabelInput(source)
      }
    ]);
    expect(messages[0].content).toContain("work happening right now");
    expect(messages[0].content).toContain("Treat overall_task_context only as background");
    expect(messages[0].content).toContain(
      "most specific concrete action and object or artifact in current_reasoning_step"
    );
    expect(messages[0].content).toContain("describe the latest concrete action");
    expect(messages[0].content).toContain(
      'over generic "Analyzing" or "Generating", but do not invent variety'
    );
    expect(messages[0].content).toContain("Start with a natural -ing action verb");
    expect(messages[0].content).toContain("Use 3 to 7 words");
    expect(messages[0].content).toContain("aim for 60 characters or fewer");
    expect(messages[0].content).toContain("Never use past-tense wording");
    expect(requests[0]).not.toHaveProperty("conversation");
    expect(requests[0]).not.toHaveProperty("store");
    expect(requests[0]).not.toHaveProperty("instructions");
    expect(requests[0]).not.toHaveProperty("metadata");
    expect(requests[0]).not.toHaveProperty("tools");
    expect(getAgentThoughtLabel(identifiers, storage)).toBe("Inspecting authentication flow");
    expect(storage.getItem(agentThoughtLabelStorageKey(identifiers))).toBe(
      "Inspecting authentication flow"
    );
  });

  test("returns null when the provider request fails", async () => {
    const storage = new MemoryStorage();
    const failedClient = fakeClient(async () => {
      throw new Error("Provider unavailable");
    });
    const source = { userRequest: "Fix login", reasoningText: "Inspect auth." };

    await expect(
      requestAgentThoughtLabel(failedClient, identifiers, source, storage)
    ).resolves.toBeNull();
    expect(storage.length).toBe(0);
  });

  test("rejects multiline, overlong, and empty visible outputs", async () => {
    const storage = new MemoryStorage();
    const multiline = "First line\nSecond line";
    const overlong = `Label_${"x".repeat(AGENT_THOUGHT_LABEL_MAX_LENGTH)}`;
    const cases = [
      { phaseId: "multiline:thought-0", output: multiline },
      { phaseId: "overlong:thought-0", output: overlong },
      { phaseId: "empty:thought-0", output: null }
    ] as const;

    for (const { phaseId, output } of cases) {
      await expect(
        requestAgentThoughtLabel(
          fakeClient(async () => completion(output)),
          { ...identifiers, phaseId },
          { userRequest: "Fix login", reasoningText: "Inspect auth." },
          storage
        )
      ).resolves.toBeNull();
    }

    expect(storage.length).toBe(0);
  });

  test("never reads hidden message reasoning when visible output is absent", async () => {
    const storage = new MemoryStorage();
    const message: FakeChatCompletion["choices"][number]["message"] = { content: null };
    Object.defineProperty(message, "reasoning", {
      get() {
        throw new Error("Hidden reasoning must not be read");
      }
    });
    const client = fakeClient(async () => ({
      choices: [
        {
          message
        }
      ]
    }));

    await expect(
      requestAgentThoughtLabel(
        client,
        { ...identifiers, phaseId: "hidden-reasoning:thought-0" },
        { userRequest: "Fix login", reasoningText: "Inspect auth." },
        storage
      )
    ).resolves.toBeNull();

    expect(storage.length).toBe(0);
  });

  test("coalesces concurrent requests for the same reasoning phase", async () => {
    const storage = new MemoryStorage();
    const response = deferred<FakeChatCompletion>();
    let requestCount = 0;
    const client = fakeClient(() => {
      requestCount += 1;
      return response.promise;
    });
    const source = { userRequest: "Fix login", reasoningText: "Inspect auth." };

    const first = requestAgentThoughtLabel(client, identifiers, source, storage);
    const second = requestAgentThoughtLabel(client, identifiers, source, storage);
    expect(second).toBe(first);
    await new Promise<void>((resolve) => setTimeout(resolve, 0));
    expect(requestCount).toBe(1);
    response.resolve(completion("Inspecting authentication flow"));

    await expect(Promise.all([first, second])).resolves.toEqual([
      "Inspecting authentication flow",
      "Inspecting authentication flow"
    ]);
  });

  test("does not contact the provider when the turn is invalidated immediately", async () => {
    const storage = new MemoryStorage();
    let requestCount = 0;
    const client = fakeClient(async () => {
      requestCount += 1;
      return completion("Inspecting authentication flow");
    });

    const pending = requestAgentThoughtLabel(
      client,
      { ...identifiers, phaseId: "immediate-invalidation:thought-0" },
      { userRequest: "Fix login", reasoningText: "Inspect auth." },
      storage
    );
    clearAgentThoughtLabelsForTurn(
      identifiers.userId,
      identifiers.sessionId,
      "immediate-invalidation",
      storage
    );

    await expect(pending).resolves.toBeNull();
    expect(requestCount).toBe(0);
    expect(storage.length).toBe(0);
  });

  test("does not save a response that arrives after its turn is replaced", async () => {
    const storage = new MemoryStorage();
    const response = deferred<FakeChatCompletion>();
    let requestCount = 0;
    const client = fakeClient(() => {
      requestCount += 1;
      return response.promise;
    });
    const turnIdentifiers = { ...identifiers, phaseId: "replaced-turn:thought-0" };

    const pending = requestAgentThoughtLabel(
      client,
      turnIdentifiers,
      { userRequest: "Fix login", reasoningText: "Inspect auth." },
      storage
    );
    await new Promise<void>((resolve) => setTimeout(resolve, 0));
    expect(requestCount).toBe(1);

    clearAgentThoughtLabelsForTurn(
      turnIdentifiers.userId,
      turnIdentifiers.sessionId,
      "replaced-turn",
      storage
    );
    response.resolve(completion("Inspecting authentication flow"));

    await expect(pending).resolves.toBeNull();
    expect(getAgentThoughtLabel(turnIdentifiers, storage)).toBeNull();
  });

  test("does not save a response that arrives after its task is deleted", async () => {
    const storage = new MemoryStorage();
    let resolveResponse: ((response: FakeChatCompletion) => void) | undefined;
    const client = fakeClient(
      () =>
        new Promise((resolve) => {
          resolveResponse = resolve;
        })
    );

    const pending = requestAgentThoughtLabel(
      client,
      identifiers,
      { userRequest: "Fix login", reasoningText: "Inspect auth." },
      storage
    );
    await new Promise<void>((resolve) => setTimeout(resolve, 0));
    expect(resolveResponse).toBeDefined();
    clearAgentThoughtLabelsForSession(identifiers.userId, identifiers.sessionId, storage);
    resolveResponse?.(completion("Inspecting authentication flow"));

    await expect(pending).resolves.toBeNull();
    expect(getAgentThoughtLabel(identifiers, storage)).toBeNull();
  });

  test("allows a reused task ID to start fresh while its deleted task request is pending", async () => {
    const storage = new MemoryStorage();
    const responses = [deferred<FakeChatCompletion>(), deferred<FakeChatCompletion>()];
    let requestCount = 0;
    const client = fakeClient(() => responses[requestCount++].promise);
    const source = { userRequest: "Fix login", reasoningText: "Inspect auth." };

    const deletedTaskRequest = requestAgentThoughtLabel(client, identifiers, source, storage);
    await new Promise<void>((resolve) => setTimeout(resolve, 0));
    clearAgentThoughtLabelsForSession(identifiers.userId, identifiers.sessionId, storage);
    const reusedTaskRequest = requestAgentThoughtLabel(client, identifiers, source, storage);
    await new Promise<void>((resolve) => setTimeout(resolve, 0));
    expect(requestCount).toBe(2);

    responses[1].resolve(completion("New task label"));
    await expect(reusedTaskRequest).resolves.toBe("New task label");
    responses[0].resolve(completion("Deleted task label"));
    await expect(deletedTaskRequest).resolves.toBeNull();
    expect(getAgentThoughtLabel(identifiers, storage)).toBe("New task label");
  });

  test("rejects a pending response after all Agent history for its account is deleted", async () => {
    const storage = new MemoryStorage();
    const response = deferred<FakeChatCompletion>();
    const client = fakeClient(() => response.promise);
    const accountIdentifiers = { ...identifiers, userId: "history-user" };
    const pending = requestAgentThoughtLabel(
      client,
      accountIdentifiers,
      { userRequest: "Fix login", reasoningText: "Inspect auth." },
      storage
    );
    await new Promise<void>((resolve) => setTimeout(resolve, 0));

    clearAgentThoughtLabelsForUser(accountIdentifiers.userId, storage);
    response.resolve(completion("Deleted history label"));

    await expect(pending).resolves.toBeNull();
    expect(getAgentThoughtLabel(accountIdentifiers, storage)).toBeNull();
  });
});

describe("Agent thought label storage", () => {
  test("cleans only the requested task or account and preserves unrelated data", () => {
    const storage = new MemoryStorage();
    const otherTask = { ...identifiers, sessionId: "task-two" };
    const otherUser = { ...identifiers, userId: "other-user" };
    storage.setItem(agentThoughtLabelStorageKey(identifiers), "First task");
    storage.setItem(agentThoughtLabelStorageKey(otherTask), "Second task");
    storage.setItem(agentThoughtLabelStorageKey(otherUser), "Other account");
    storage.setItem("unrelated", "keep me");

    clearAgentThoughtLabelsForSession(identifiers.userId, identifiers.sessionId, storage);
    expect(getAgentThoughtLabel(identifiers, storage)).toBeNull();
    expect(getAgentThoughtLabel(otherTask, storage)).toBe("Second task");
    expect(getAgentThoughtLabel(otherUser, storage)).toBe("Other account");

    clearAgentThoughtLabelsForUser(identifiers.userId, storage);
    expect(getAgentThoughtLabel(otherTask, storage)).toBeNull();
    expect(getAgentThoughtLabel(otherUser, storage)).toBe("Other account");
    expect(storage.getItem("unrelated")).toBe("keep me");
  });

  test("cleans only phases from a replaced user turn", () => {
    const storage = new MemoryStorage();
    const nextPhase = {
      ...identifiers,
      phaseId: "assistant-after-user:thought-1"
    };
    const otherTurn = {
      ...identifiers,
      phaseId: "assistant-after-other-user:thought-0"
    };
    storage.setItem(agentThoughtLabelStorageKey(identifiers), "First phase");
    storage.setItem(agentThoughtLabelStorageKey(nextPhase), "Second phase");
    storage.setItem(agentThoughtLabelStorageKey(otherTurn), "Other turn");

    clearAgentThoughtLabelsForTurn(
      identifiers.userId,
      identifiers.sessionId,
      "assistant-after-user",
      storage
    );

    expect(getAgentThoughtLabel(identifiers, storage)).toBeNull();
    expect(getAgentThoughtLabel(nextPhase, storage)).toBeNull();
    expect(getAgentThoughtLabel(otherTurn, storage)).toBe("Other turn");
  });

  test("ignores invalid stored labels and unavailable storage", async () => {
    const storage = new MemoryStorage();
    storage.setItem(agentThoughtLabelStorageKey(identifiers), "Invalid\nlabel");
    expect(getAgentThoughtLabel(identifiers, storage)).toBeNull();

    const unavailableStorage: AgentThoughtLabelStorage = {
      get length(): number {
        throw new Error("unavailable");
      },
      getItem() {
        throw new Error("unavailable");
      },
      key() {
        throw new Error("unavailable");
      },
      removeItem() {
        throw new Error("unavailable");
      },
      setItem() {
        throw new Error("unavailable");
      }
    };

    expect(getAgentThoughtLabel(identifiers, unavailableStorage)).toBeNull();
    await expect(
      requestAgentThoughtLabel(
        fakeClient(async () => completion("Inspecting unavailable storage")),
        { ...identifiers, phaseId: "unavailable-storage:thought-0" },
        { userRequest: "Inspect storage", reasoningText: "Checking browser storage." },
        unavailableStorage
      )
    ).resolves.toBe("Inspecting unavailable storage");
    expect(() =>
      clearAgentThoughtLabelsForSession(
        identifiers.userId,
        identifiers.sessionId,
        unavailableStorage
      )
    ).not.toThrow();
  });
});
