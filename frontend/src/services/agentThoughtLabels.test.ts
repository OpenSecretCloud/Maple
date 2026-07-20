import { describe, expect, test } from "bun:test";
import OpenAI from "openai";
import {
  AGENT_THOUGHT_LABEL_MAX_CONCURRENT_PROVISIONAL_REQUESTS,
  AGENT_THOUGHT_LABEL_FALLBACK_DELAY_MS,
  AGENT_THOUGHT_LABEL_FALLBACK_TEXT,
  AGENT_THOUGHT_LABEL_MAX_LENGTH,
  AGENT_THOUGHT_LABEL_PENDING_TEXT,
  AGENT_THOUGHT_LABEL_PROVISIONAL_DEADLINE_MS,
  AGENT_THOUGHT_LABEL_PROVISIONAL_DELAY_MS,
  AGENT_THOUGHT_LABEL_PROVISIONAL_MILESTONES_MS,
  AGENT_THOUGHT_LABEL_PROVISIONAL_MIN_LENGTH,
  AGENT_THOUGHT_LABEL_REASONING_MAX_LENGTH,
  AGENT_THOUGHT_LABEL_USER_REQUEST_MAX_LENGTH,
  AgentThoughtLabelProvisionalScheduler,
  agentThoughtLabelStorageKey,
  buildAgentThoughtLabelInput,
  clearAgentThoughtLabelsForSession,
  clearAgentThoughtLabelsForTurn,
  clearAgentThoughtLabelsForUser,
  getAgentThoughtLabel,
  parseAgentThoughtLabel,
  persistAgentThoughtLabel,
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
    finish_reason: string;
    message: { content: string | null; reasoning?: string | null };
  }[];
}

function fakeClient(
  create: (request: Record<string, unknown>) => Promise<FakeChatCompletion>
): AgentThoughtLabelClient {
  return { chat: { completions: { create } } } as unknown as AgentThoughtLabelClient;
}

function completion(content: string | null): FakeChatCompletion {
  return { choices: [{ finish_reason: "stop", message: { content } }] };
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

function manualTimers(): {
  schedule: (callback: () => void, delayMs: number) => () => void;
  run: (delayMs: number) => void;
  pending: (delayMs: number) => number;
} {
  const timers: Array<{ callback: () => void; delayMs: number; cancelled: boolean }> = [];
  return {
    schedule: (callback, delayMs) => {
      const timer = { callback, delayMs, cancelled: false };
      timers.push(timer);
      return () => {
        timer.cancelled = true;
      };
    },
    run: (delayMs) => {
      const runnable = timers.filter((timer) => !timer.cancelled && timer.delayMs === delayMs);
      runnable.forEach((timer) => {
        timer.cancelled = true;
        timer.callback();
      });
    },
    pending: (delayMs) =>
      timers.filter((timer) => !timer.cancelled && timer.delayMs === delayMs).length
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

  test("keeps a retained provisional label while the final request settles", async () => {
    const successfulResponse = deferred<string | null>();
    const successCommits: [string, string | undefined][] = [];
    let fallbackScheduled = false;
    startAgentThoughtLabelDisplay({
      cachedLabel: null,
      retainedLabel: "Investigating login failures",
      request: () => successfulResponse.promise,
      commit: (label, expectedLabel) => successCommits.push([label, expectedLabel]),
      scheduleFallback: () => {
        fallbackScheduled = true;
        return () => {};
      }
    });

    expect(successCommits).toEqual([]);
    expect(fallbackScheduled).toBe(false);
    successfulResponse.resolve("Explaining authentication findings");
    await successfulResponse.promise;
    expect(successCommits).toEqual([["Explaining authentication findings", undefined]]);

    const failureCommits: [string, string | undefined][] = [];
    startAgentThoughtLabelDisplay({
      cachedLabel: null,
      retainedLabel: "Investigating login failures",
      request: async () => null,
      commit: (label, expectedLabel) => failureCommits.push([label, expectedLabel])
    });
    await Promise.resolve();
    expect(failureCommits).toEqual([]);
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
    ) as {
      phase_state: string;
      overall_task_context: string;
      current_reasoning_step: string;
    };

    expect(Array.from(input.overall_task_context)).toHaveLength(
      AGENT_THOUGHT_LABEL_USER_REQUEST_MAX_LENGTH
    );
    expect(Array.from(input.current_reasoning_step)).toHaveLength(
      AGENT_THOUGHT_LABEL_REASONING_MAX_LENGTH
    );
    expect(input.overall_task_context).not.toContain("USER_SECRET");
    expect(input.current_reasoning_step).not.toContain("REASONING_SECRET");
    expect(input.phase_state).toBe("complete");
    expect(Object.keys(input).sort()).toEqual([
      "current_reasoning_step",
      "overall_task_context",
      "phase_state"
    ]);
    expect(
      JSON.parse(
        buildAgentThoughtLabelInput(
          { userRequest: "Fix login", reasoningText: "Inspect auth." },
          "streaming"
        )
      ).phase_state
    ).toBe("streaming");
  });
});

describe("AgentThoughtLabelProvisionalScheduler", () => {
  const phase = (phaseId: string, reasoningLength: number) => ({
    sessionId: "session",
    phaseId,
    userRequest: "Fix login",
    reasoningText: "r".repeat(reasoningLength)
  });

  test("requires both the one-second delay and 100 reasoning characters", async () => {
    const firstTimers = manualTimers();
    const firstRequests: string[] = [];
    const firstScheduler = new AgentThoughtLabelProvisionalScheduler({
      schedule: firstTimers.schedule,
      request: async (source) => {
        firstRequests.push(source.reasoningText);
        return "Investigating login failures";
      },
      commit: () => {}
    });

    firstScheduler.observe(
      phase("assistant:thought-0", AGENT_THOUGHT_LABEL_PROVISIONAL_MIN_LENGTH - 1)
    );
    firstTimers.run(AGENT_THOUGHT_LABEL_PROVISIONAL_DELAY_MS);
    expect(firstRequests).toHaveLength(0);
    firstScheduler.observe(
      phase("assistant:thought-0", AGENT_THOUGHT_LABEL_PROVISIONAL_MIN_LENGTH)
    );
    expect(firstRequests).toHaveLength(1);
    await Promise.resolve();

    const secondTimers = manualTimers();
    let secondRequestCount = 0;
    const secondScheduler = new AgentThoughtLabelProvisionalScheduler({
      schedule: secondTimers.schedule,
      request: async () => {
        secondRequestCount += 1;
        return null;
      },
      commit: () => {}
    });
    secondScheduler.observe(
      phase("assistant:thought-1", AGENT_THOUGHT_LABEL_PROVISIONAL_MIN_LENGTH)
    );
    expect(secondRequestCount).toBe(0);
    secondTimers.run(AGENT_THOUGHT_LABEL_PROVISIONAL_DELAY_MS);
    expect(secondRequestCount).toBe(1);
    await Promise.resolve();
  });

  test("refreshes from the latest reasoning snapshot at one, five, and fifteen seconds", async () => {
    const timers = manualTimers();
    const requestedReasoning: string[] = [];
    const commits: string[] = [];
    const scheduler = new AgentThoughtLabelProvisionalScheduler({
      schedule: timers.schedule,
      request: async (source) => {
        requestedReasoning.push(source.reasoningText);
        return `Label ${requestedReasoning.length}`;
      },
      commit: (_source, label) => commits.push(label)
    });
    const [firstMilestone, secondMilestone, thirdMilestone] =
      AGENT_THOUGHT_LABEL_PROVISIONAL_MILESTONES_MS;

    scheduler.observe(phase("assistant:thought-0", 100));
    timers.run(firstMilestone);
    await Promise.resolve();
    scheduler.observe(phase("assistant:thought-0", 200));
    timers.run(secondMilestone);
    await Promise.resolve();
    scheduler.observe(phase("assistant:thought-0", 300));
    timers.run(thirdMilestone);
    await Promise.resolve();

    expect(requestedReasoning.map((reasoning) => reasoning.length)).toEqual([100, 200, 300]);
    expect(commits).toEqual(["Label 1", "Label 2", "Label 3"]);
  });

  test("skips a milestone while the phase request is pending but allows a later refresh", async () => {
    const timers = manualTimers();
    const firstResponse = deferred<string | null>();
    const requestedLengths: number[] = [];
    const scheduler = new AgentThoughtLabelProvisionalScheduler({
      schedule: timers.schedule,
      request: (source) => {
        requestedLengths.push(source.reasoningText.length);
        return requestedLengths.length === 1
          ? firstResponse.promise
          : Promise.resolve("Reviewing later evidence");
      },
      commit: () => {}
    });
    const [firstMilestone, secondMilestone, thirdMilestone] =
      AGENT_THOUGHT_LABEL_PROVISIONAL_MILESTONES_MS;

    scheduler.observe(phase("assistant:thought-0", 100));
    timers.run(firstMilestone);
    scheduler.observe(phase("assistant:thought-0", 200));
    timers.run(secondMilestone);
    expect(requestedLengths).toEqual([100]);

    firstResponse.resolve("Reviewing initial evidence");
    await firstResponse.promise;
    scheduler.observe(phase("assistant:thought-0", 300));
    timers.run(thirdMilestone);
    expect(requestedLengths).toEqual([100, 300]);
  });

  test("makes one request when enough reasoning arrives after multiple milestones", async () => {
    const timers = manualTimers();
    const requestedLengths: number[] = [];
    const scheduler = new AgentThoughtLabelProvisionalScheduler({
      schedule: timers.schedule,
      request: async (source) => {
        requestedLengths.push(source.reasoningText.length);
        return null;
      },
      commit: () => {}
    });
    const [firstMilestone, secondMilestone] = AGENT_THOUGHT_LABEL_PROVISIONAL_MILESTONES_MS;

    scheduler.observe(phase("assistant:thought-0", 99));
    timers.run(firstMilestone);
    timers.run(secondMilestone);
    scheduler.observe(phase("assistant:thought-0", 100));
    scheduler.observe(phase("assistant:thought-0", 101));
    await Promise.resolve();

    expect(requestedLengths).toEqual([100]);
  });

  test("allows later milestones after declined and failed requests", async () => {
    const timers = manualTimers();
    const results = [null, AGENT_THOUGHT_LABEL_PENDING_TEXT, "Comparing final options"];
    const commits: string[] = [];
    let requestCount = 0;
    const scheduler = new AgentThoughtLabelProvisionalScheduler({
      schedule: timers.schedule,
      request: async () => {
        requestCount += 1;
        return results.shift() ?? null;
      },
      commit: (_source, label) => commits.push(label)
    });

    for (const [index, milestone] of AGENT_THOUGHT_LABEL_PROVISIONAL_MILESTONES_MS.entries()) {
      scheduler.observe(phase("assistant:thought-0", 100 + index));
      timers.run(milestone);
      await Promise.resolve();
    }

    expect(requestCount).toBe(3);
    expect(commits).toEqual(["Comparing final options"]);
  });

  test("skips unchanged snapshots and does not recommit an exact duplicate label", async () => {
    const timers = manualTimers();
    const commits: string[] = [];
    let requestCount = 0;
    const scheduler = new AgentThoughtLabelProvisionalScheduler({
      schedule: timers.schedule,
      request: async () => {
        requestCount += 1;
        return "Comparing authentication options";
      },
      commit: (_source, label) => commits.push(label)
    });
    const [firstMilestone, secondMilestone, thirdMilestone] =
      AGENT_THOUGHT_LABEL_PROVISIONAL_MILESTONES_MS;
    const source = phase("assistant:thought-0", 100);

    scheduler.observe(source);
    timers.run(firstMilestone);
    await Promise.resolve();
    timers.run(secondMilestone);
    scheduler.observe({ ...source, reasoningText: `${source.reasoningText} changed` });
    timers.run(thirdMilestone);
    await Promise.resolve();

    expect(requestCount).toBe(2);
    expect(commits).toEqual(["Comparing authentication options"]);
  });

  test("abandons a provisional request at three seconds and allows the next milestone", async () => {
    const timers = manualTimers();
    const response = deferred<string | null>();
    const commits: string[] = [];
    let requestCount = 0;
    const requestSignal: { current?: AbortSignal } = {};
    const scheduler = new AgentThoughtLabelProvisionalScheduler({
      schedule: timers.schedule,
      request: (_source, signal) => {
        requestCount += 1;
        requestSignal.current = signal;
        return requestCount === 1 ? response.promise : Promise.resolve("Reviewing newer evidence");
      },
      commit: (_source, label) => commits.push(label)
    });

    const source = phase("assistant:thought-0", AGENT_THOUGHT_LABEL_PROVISIONAL_MIN_LENGTH);
    expect(AGENT_THOUGHT_LABEL_PROVISIONAL_DEADLINE_MS).toBe(3_000);
    expect(
      AGENT_THOUGHT_LABEL_PROVISIONAL_DELAY_MS + AGENT_THOUGHT_LABEL_PROVISIONAL_DEADLINE_MS
    ).toBeLessThan(AGENT_THOUGHT_LABEL_PROVISIONAL_MILESTONES_MS[1]);
    scheduler.observe(source);
    timers.run(AGENT_THOUGHT_LABEL_PROVISIONAL_DELAY_MS);
    expect(timers.pending(AGENT_THOUGHT_LABEL_PROVISIONAL_DEADLINE_MS)).toBe(1);
    timers.run(AGENT_THOUGHT_LABEL_PROVISIONAL_DEADLINE_MS);
    expect(requestSignal.current?.aborted).toBe(true);

    response.resolve("Investigating login failures");
    await response.promise;
    scheduler.observe({ ...source, reasoningText: `${source.reasoningText} more` });
    expect(commits).toEqual([]);
    expect(requestCount).toBe(1);

    timers.run(AGENT_THOUGHT_LABEL_PROVISIONAL_MILESTONES_MS[1]);
    await Promise.resolve();
    expect(requestCount).toBe(2);
    expect(commits).toEqual(["Reviewing newer evidence"]);
  });

  test("limits concurrent provisionals and gives a cap-skipped phase its next milestone", async () => {
    const timers = manualTimers();
    const responses = Array.from(
      { length: AGENT_THOUGHT_LABEL_MAX_CONCURRENT_PROVISIONAL_REQUESTS + 1 },
      () => deferred<string | null>()
    );
    const requestedPhaseIds: string[] = [];
    const scheduler = new AgentThoughtLabelProvisionalScheduler({
      schedule: timers.schedule,
      request: (source) => {
        requestedPhaseIds.push(source.phaseId);
        return responses[requestedPhaseIds.length - 1].promise;
      },
      commit: () => {}
    });

    for (
      let index = 0;
      index < AGENT_THOUGHT_LABEL_MAX_CONCURRENT_PROVISIONAL_REQUESTS + 1;
      index += 1
    ) {
      scheduler.observe(
        phase(`assistant:thought-${index}`, AGENT_THOUGHT_LABEL_PROVISIONAL_MIN_LENGTH)
      );
    }
    timers.run(AGENT_THOUGHT_LABEL_PROVISIONAL_DELAY_MS);
    expect(requestedPhaseIds).toEqual(["assistant:thought-0", "assistant:thought-1"]);

    responses[0].resolve(null);
    await responses[0].promise;
    scheduler.observe(
      phase("assistant:thought-2", AGENT_THOUGHT_LABEL_PROVISIONAL_MIN_LENGTH + 50)
    );
    expect(requestedPhaseIds).toEqual(["assistant:thought-0", "assistant:thought-1"]);

    timers.run(AGENT_THOUGHT_LABEL_PROVISIONAL_MILESTONES_MS[1]);
    expect(requestedPhaseIds).toEqual([
      "assistant:thought-0",
      "assistant:thought-1",
      "assistant:thought-2"
    ]);

    scheduler.observe(phase("assistant:thought-3", AGENT_THOUGHT_LABEL_PROVISIONAL_MIN_LENGTH));
    timers.run(AGENT_THOUGHT_LABEL_PROVISIONAL_DELAY_MS);
    expect(requestedPhaseIds).toEqual([
      "assistant:thought-0",
      "assistant:thought-1",
      "assistant:thought-2"
    ]);
  });

  test("cancels an in-flight provisional when its phase completes", async () => {
    const timers = manualTimers();
    const response = deferred<string | null>();
    const commits: string[] = [];
    const requestSignal: { current?: AbortSignal } = {};
    const scheduler = new AgentThoughtLabelProvisionalScheduler({
      schedule: timers.schedule,
      request: (_source, signal) => {
        requestSignal.current = signal;
        return response.promise;
      },
      commit: (_source, label) => commits.push(label)
    });
    const source = phase("assistant:thought-0", AGENT_THOUGHT_LABEL_PROVISIONAL_MIN_LENGTH);
    scheduler.observe(source);
    timers.run(AGENT_THOUGHT_LABEL_PROVISIONAL_DELAY_MS);

    expect(scheduler.complete(source.sessionId, source.phaseId)).toBeNull();
    expect(requestSignal.current?.aborted).toBe(true);
    response.resolve("Investigating login failures");
    await response.promise;
    expect(commits).toEqual([]);
  });

  test("cancels matching pending provisionals during turn invalidation", async () => {
    const timers = manualTimers();
    const response = deferred<string | null>();
    const commits: string[] = [];
    const requestSignal: { current?: AbortSignal } = {};
    const scheduler = new AgentThoughtLabelProvisionalScheduler({
      schedule: timers.schedule,
      request: (_source, signal) => {
        requestSignal.current = signal;
        return response.promise;
      },
      commit: (_source, label) => commits.push(label)
    });
    const source = phase(
      "assistant-after-user:thought-0",
      AGENT_THOUGHT_LABEL_PROVISIONAL_MIN_LENGTH
    );
    scheduler.observe(source);
    timers.run(AGENT_THOUGHT_LABEL_PROVISIONAL_DELAY_MS);

    scheduler.cancelMatching(source.sessionId, "assistant-after-user");
    expect(requestSignal.current?.aborted).toBe(true);
    response.resolve("Investigating login failures");
    await response.promise;
    expect(commits).toEqual([]);
  });

  test("returns a displayed provisional at completion and treats Thinking as no label", async () => {
    const timers = manualTimers();
    const labels = ["Investigating login failures", AGENT_THOUGHT_LABEL_PENDING_TEXT];
    const commits: string[] = [];
    const scheduler = new AgentThoughtLabelProvisionalScheduler({
      schedule: timers.schedule,
      request: async () => labels.shift() ?? null,
      commit: (_source, label) => commits.push(label)
    });

    const displayed = phase("assistant:thought-0", AGENT_THOUGHT_LABEL_PROVISIONAL_MIN_LENGTH);
    scheduler.observe(displayed);
    timers.run(AGENT_THOUGHT_LABEL_PROVISIONAL_DELAY_MS);
    await Promise.resolve();
    expect(scheduler.complete(displayed.sessionId, displayed.phaseId)).toBe(
      "Investigating login failures"
    );

    const declined = phase("assistant:thought-1", AGENT_THOUGHT_LABEL_PROVISIONAL_MIN_LENGTH);
    scheduler.observe(declined);
    timers.run(AGENT_THOUGHT_LABEL_PROVISIONAL_DELAY_MS);
    await Promise.resolve();
    expect(scheduler.complete(declined.sessionId, declined.phaseId)).toBeNull();
    expect(commits).toEqual(["Investigating login failures"]);
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
            model: "gemma4-31b",
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
    expect(requestedBody).toMatchObject({
      model: "gemma4-31b",
      temperature: 0,
      max_tokens: 64,
      include_reasoning: false,
      chat_template_kwargs: { enable_thinking: false },
      stream: false
    });
    expect(requestedBody).not.toHaveProperty("conversation");
  });

  test("sends the label prompt once and persists a valid completed label", async () => {
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
    [
      "Treat overall_task_context only as background",
      "most specific supported subject",
      "A proposed future action is not an action currently being performed",
      "Start with a natural -ing action verb",
      "Use 3 to 8 words",
      "Never use past-tense wording",
      "phase_state is either streaming or complete",
      "For complete input, always attempt a descriptive label"
    ].forEach((contract) => expect(messages[0].content).toContain(contract));

    const promptLines = messages[0].content.split("\n");
    const prematureVerbList = /finality verbs: ([^.]+)\./u.exec(messages[0].content)?.[1];
    expect(prematureVerbList).toBeDefined();
    const prematureVerbs = prematureVerbList?.split(", ") ?? [];
    const examples = promptLines.flatMap((line, index) => {
      const state = /^(streaming|complete):/u.exec(line)?.[1];
      return state ? [{ state, label: promptLines[index + 1] }] : [];
    });
    expect(examples).toHaveLength(9);
    expect(examples.filter(({ state }) => state === "streaming")).toHaveLength(5);
    expect(examples.filter(({ state }) => state === "complete")).toHaveLength(4);
    expect(examples.some(({ label }) => label === AGENT_THOUGHT_LABEL_PENDING_TEXT)).toBe(true);
    const exampleLabels = examples.map(({ label }) => label).join("\n");
    ["/downloads", "UseCaseGridSection", "llms.txt"].forEach((subject) =>
      expect(exampleLabels).toContain(subject)
    );
    examples.forEach(({ state, label }) => {
      if (label === AGENT_THOUGHT_LABEL_PENDING_TEXT) return;
      expect(label.trim().split(/\s+/u).length).toBeGreaterThanOrEqual(3);
      expect(label.trim().split(/\s+/u).length).toBeLessThanOrEqual(8);
      expect(Array.from(label).length).toBeLessThanOrEqual(60);
      if (state === "streaming") {
        expect(prematureVerbs).not.toContain(label.trim().split(/\s+/u)[0]);
      }
    });
    ["conversation", "store"].forEach((field) => expect(requests[0]).not.toHaveProperty(field));
    expect(getAgentThoughtLabel(identifiers, storage)).toBe("Inspecting authentication flow");
    expect(storage.getItem(agentThoughtLabelStorageKey(identifiers))).toBe(
      "Inspecting authentication flow"
    );
  });

  test("keeps streaming and completed requests separate and persists only durable labels", async () => {
    const storage = new MemoryStorage();
    const requestedStates: string[] = [];
    const client = fakeClient(async (request) => {
      const messages = request.messages as { content: string }[];
      const { phase_state: phaseState } = JSON.parse(messages[1].content) as {
        phase_state: string;
      };
      requestedStates.push(phaseState);
      return completion(
        phaseState === "streaming"
          ? "Investigating login failures"
          : "Explaining authentication findings"
      );
    });
    const source = { userRequest: "Fix login", reasoningText: "Inspect auth." };

    await expect(
      requestAgentThoughtLabel(client, identifiers, source, storage, {
        phaseState: "streaming"
      })
    ).resolves.toBe("Investigating login failures");
    expect(getAgentThoughtLabel(identifiers, storage)).toBeNull();

    persistAgentThoughtLabel(identifiers, "Investigating login failures", storage);
    await expect(
      requestAgentThoughtLabel(client, identifiers, source, storage, {
        phaseState: "complete",
        bypassStoredLabel: true
      })
    ).resolves.toBe("Explaining authentication findings");

    expect(requestedStates).toEqual(["streaming", "complete"]);
    expect(getAgentThoughtLabel(identifiers, storage)).toBe("Explaining authentication findings");
  });

  test("keeps independently cancellable streaming snapshots separate", async () => {
    const storage = new MemoryStorage();
    const signals: Array<AbortSignal | undefined> = [];
    const client = {
      chat: {
        completions: {
          create: async (_request: unknown, options?: { signal?: AbortSignal }) => {
            signals.push(options?.signal);
            return completion(`Reviewing request ${signals.length}`);
          }
        }
      }
    } as unknown as AgentThoughtLabelClient;
    const source = { userRequest: "Fix login", reasoningText: "Initial reasoning" };
    const firstController = new AbortController();
    const secondController = new AbortController();

    const first = requestAgentThoughtLabel(client, identifiers, source, storage, {
      phaseState: "streaming",
      signal: firstController.signal
    });
    const second = requestAgentThoughtLabel(client, identifiers, source, storage, {
      phaseState: "streaming",
      signal: secondController.signal
    });

    expect(second).not.toBe(first);
    await expect(Promise.all([first, second])).resolves.toEqual([
      "Reviewing request 1",
      "Reviewing request 2"
    ]);
    expect(signals).toEqual([firstController.signal, secondController.signal]);
  });

  test("leaves a persisted provisional in place when final generation fails", async () => {
    const storage = new MemoryStorage();
    persistAgentThoughtLabel(identifiers, "Investigating login failures", storage);

    await expect(
      requestAgentThoughtLabel(
        fakeClient(async () => completion(null)),
        identifiers,
        { userRequest: "Fix login", reasoningText: "Inspect auth." },
        storage,
        { phaseState: "complete", bypassStoredLabel: true }
      )
    ).resolves.toBeNull();
    expect(getAgentThoughtLabel(identifiers, storage)).toBe("Investigating login failures");
  });

  test("allows Thinking only as a streaming decline and rejects clipped output", async () => {
    const storage = new MemoryStorage();
    const source = { userRequest: "Fix login", reasoningText: "I need to look into this more." };

    await expect(
      requestAgentThoughtLabel(
        fakeClient(async () => completion(AGENT_THOUGHT_LABEL_PENDING_TEXT)),
        { ...identifiers, phaseId: "streaming-decline:thought-0" },
        source,
        storage,
        { phaseState: "streaming" }
      )
    ).resolves.toBe(AGENT_THOUGHT_LABEL_PENDING_TEXT);
    await expect(
      requestAgentThoughtLabel(
        fakeClient(async () => completion(AGENT_THOUGHT_LABEL_PENDING_TEXT)),
        { ...identifiers, phaseId: "completed-decline:thought-0" },
        source,
        storage,
        { phaseState: "complete" }
      )
    ).resolves.toBeNull();
    await expect(
      requestAgentThoughtLabel(
        fakeClient(async () => ({
          choices: [
            {
              finish_reason: "length",
              message: { content: "Investigating login" }
            }
          ]
        })),
        { ...identifiers, phaseId: "clipped:thought-0" },
        source,
        storage
      )
    ).resolves.toBeNull();
    expect(storage.length).toBe(0);
  });

  test("blocks premature streaming verbs without restricting other active wording", async () => {
    const storage = new MemoryStorage();
    const source = {
      userRequest: "Recommend SEO improvements",
      reasoningText: "Review the remaining evidence before reporting recommendations."
    };
    const prematureLabels = [
      "Formulating SEO recommendations",
      "Writing final SEO recommendations",
      "Presenting the completed SEO audit",
      "Delivering SEO migration findings",
      "delivering SEO migration findings"
    ];

    for (const [index, label] of prematureLabels.entries()) {
      await expect(
        requestAgentThoughtLabel(
          fakeClient(async () => completion(label)),
          { ...identifiers, phaseId: `streaming-deliverable-${index}:thought-0` },
          source,
          storage,
          { phaseState: "streaming" }
        )
      ).resolves.toBeNull();
    }
    const activeLabels = [
      "Synthesizing remaining SEO evidence",
      "Generating parser edge cases",
      "Sharing state across components"
    ];
    for (const [index, label] of activeLabels.entries()) {
      await expect(
        requestAgentThoughtLabel(
          fakeClient(async () => completion(label)),
          { ...identifiers, phaseId: `streaming-active-${index}:thought-0` },
          source,
          storage,
          { phaseState: "streaming" }
        )
      ).resolves.toBe(label);
    }
    await expect(
      requestAgentThoughtLabel(
        fakeClient(async () => completion("Formulating SEO recommendations")),
        { ...identifiers, phaseId: "complete-deliverable:thought-0" },
        source,
        storage,
        { phaseState: "complete" }
      )
    ).resolves.toBe("Formulating SEO recommendations");

    prematureLabels.forEach((_label, index) => {
      expect(
        getAgentThoughtLabel(
          { ...identifiers, phaseId: `streaming-deliverable-${index}:thought-0` },
          storage
        )
      ).toBeNull();
    });
    expect(
      getAgentThoughtLabel({ ...identifiers, phaseId: "complete-deliverable:thought-0" }, storage)
    ).toBe("Formulating SEO recommendations");
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

  test("rejects missing and unfinished completion choices", async () => {
    const storage = new MemoryStorage();
    const source = { userRequest: "Fix login", reasoningText: "Inspect auth." };
    const cases = [
      {
        phaseId: "missing-choice:thought-0",
        client: fakeClient(async () => ({ choices: [] }))
      },
      {
        phaseId: "filtered:thought-0",
        client: fakeClient(async () => ({
          choices: [
            {
              finish_reason: "content_filter",
              message: { content: "Reviewing authentication flow" }
            }
          ]
        }))
      }
    ] as const;

    for (const { phaseId, client } of cases) {
      await expect(
        requestAgentThoughtLabel(client, { ...identifiers, phaseId }, source, storage)
      ).resolves.toBeNull();
    }

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
          finish_reason: "stop",
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
