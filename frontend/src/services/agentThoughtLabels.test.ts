import { describe, expect, test } from "bun:test";
import OpenAI from "openai";
import {
  AGENT_THOUGHT_LABEL_FALLBACK_DELAY_MS,
  AGENT_THOUGHT_LABEL_FALLBACK_TEXT,
  AGENT_THOUGHT_LABEL_MAX_CONCURRENT_PROVISIONAL_REQUESTS,
  AGENT_THOUGHT_LABEL_MAX_LENGTH,
  AGENT_THOUGHT_LABEL_PENDING_TEXT,
  AGENT_THOUGHT_LABEL_PROVISIONAL_DEADLINE_MS,
  AGENT_THOUGHT_LABEL_PROVISIONAL_DELAY_MS,
  AGENT_THOUGHT_LABEL_PROVISIONAL_MILESTONES_MS,
  AGENT_THOUGHT_LABEL_PROVISIONAL_MIN_LENGTH,
  AGENT_THOUGHT_LABEL_REASONING_MAX_LENGTH,
  AGENT_THOUGHT_LABEL_USER_REQUEST_MAX_LENGTH,
  AgentThoughtLabelFinalRequestRegistry,
  AgentThoughtLabelProvisionalScheduler,
  buildAgentThoughtLabelInput,
  parseAgentThoughtLabel,
  requestAgentThoughtLabel,
  startAgentThoughtLabelDisplay,
  type AgentThoughtLabelClient
} from "./agentThoughtLabels";

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

function nextTask(): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, 0));
}

describe("startAgentThoughtLabelDisplay", () => {
  test("shows Thinking until a valid label arrives and cancels the fallback", async () => {
    const response = deferred<string | null>();
    const commits: [string, string | undefined][] = [];
    let scheduledDelay = 0;
    let fallbackCancelled = false;

    startAgentThoughtLabelDisplay({
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
    await Promise.resolve();
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
    await Promise.resolve();
    expect(commits.at(-1)).toEqual(["Tracing a slower response", undefined]);
  });

  test("shows Thought immediately when generation fails", async () => {
    const commits: [string, string | undefined][] = [];

    startAgentThoughtLabelDisplay({
      request: async () => null,
      commit: (label, expectedLabel) => commits.push([label, expectedLabel]),
      scheduleFallback: () => () => {}
    });
    await Promise.resolve();

    expect(commits).toEqual([
      [AGENT_THOUGHT_LABEL_PENDING_TEXT, undefined],
      [AGENT_THOUGHT_LABEL_FALLBACK_TEXT, undefined]
    ]);
  });

  test("aborts a cancelled request and ignores its fallback and late result", async () => {
    const response = deferred<string | null>();
    const commits: [string, string | undefined][] = [];
    let requestSignal: AbortSignal | undefined;
    let showFallback = () => {};
    const cancel = startAgentThoughtLabelDisplay({
      request: (signal) => {
        requestSignal = signal;
        return response.promise;
      },
      commit: (label, expectedLabel) => commits.push([label, expectedLabel]),
      scheduleFallback: (callback) => {
        showFallback = callback;
        return () => {};
      }
    });

    cancel();
    expect(requestSignal?.aborted).toBe(true);
    showFallback();
    response.resolve("Ignoring stale label");
    await response.promise;
    await Promise.resolve();

    expect(commits).toEqual([[AGENT_THOUGHT_LABEL_PENDING_TEXT, undefined]]);
  });

  test("keeps a retained provisional label while final generation settles", async () => {
    const successfulResponse = deferred<string | null>();
    const successCommits: [string, string | undefined][] = [];
    let fallbackScheduled = false;
    startAgentThoughtLabelDisplay({
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
    await Promise.resolve();
    expect(successCommits).toEqual([["Explaining authentication findings", undefined]]);

    const failedResponse = deferred<string | null>();
    const failureCommits: [string, string | undefined][] = [];
    startAgentThoughtLabelDisplay({
      retainedLabel: "Investigating login failures",
      request: () => failedResponse.promise,
      commit: (label, expectedLabel) => failureCommits.push([label, expectedLabel])
    });
    failedResponse.resolve(null);
    await failedResponse.promise;
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

describe("AgentThoughtLabelFinalRequestRegistry", () => {
  test("aborts an obsolete final snapshot and ignores its late result", async () => {
    const registry = new AgentThoughtLabelFinalRequestRegistry();
    const phaseA = {
      sessionId: "session",
      phaseId: "assistant:thought-0",
      userRequest: "Fix login",
      reasoningText: "Review the first snapshot"
    };
    const phaseB = { ...phaseA, reasoningText: "Review the authoritative snapshot" };
    const responseA = deferred<string | null>();
    const responseB = deferred<string | null>();
    let signalA: AbortSignal | undefined;
    const state: { visibleLabel: string | null } = { visibleLabel: null };

    const start = (
      phase: typeof phaseA,
      response: { promise: Promise<string | null>; resolve: (value: string | null) => void },
      captureSignal?: (signal: AbortSignal) => void
    ) => {
      const request = registry.begin(phase);
      if (!request) return null;
      const cancel = startAgentThoughtLabelDisplay({
        retainedLabel: request.retainedLabel,
        request: (signal) => {
          captureSignal?.(signal);
          return response.promise.finally(request.finish);
        },
        commit: (label, expectedLabel) => {
          if (!request.isCurrent()) return;
          if (expectedLabel !== undefined && state.visibleLabel !== expectedLabel) return;
          request.recordLabel(label);
          state.visibleLabel = label;
        },
        scheduleFallback: () => () => {}
      });
      request.setCancel(cancel);
      return request;
    };

    expect(start(phaseA, responseA, (signal) => (signalA = signal))).not.toBeNull();
    expect(state.visibleLabel).toBe(AGENT_THOUGHT_LABEL_PENDING_TEXT);

    const currentRequest = start(phaseB, responseB);
    expect(currentRequest).not.toBeNull();
    expect(signalA?.aborted).toBe(true);
    expect(currentRequest?.retainedLabel).toBeNull();

    responseB.resolve("Reviewing authoritative final snapshot");
    await responseB.promise;
    await nextTask();
    expect(state.visibleLabel).toBe("Reviewing authoritative final snapshot");
    expect(registry.begin(phaseB)).toBeNull();

    responseA.resolve("Reviewing obsolete final snapshot");
    await responseA.promise;
    await Promise.resolve();
    expect(state.visibleLabel).toBe("Reviewing authoritative final snapshot");
  });

  test("retains a displayed final label while a newer snapshot settles", () => {
    const registry = new AgentThoughtLabelFinalRequestRegistry();
    const phase = {
      sessionId: "session",
      phaseId: "assistant:thought-0",
      userRequest: "Fix login",
      reasoningText: "Review the first snapshot"
    };
    const first = registry.begin(phase, "Reviewing first final snapshot");
    first?.finish();

    const next = registry.begin({ ...phase, reasoningText: "Review the newer snapshot" });
    expect(next?.retainedLabel).toBe("Reviewing first final snapshot");
  });

  test("cancels only matching final requests and ignores their late results", async () => {
    const registry = new AgentThoughtLabelFinalRequestRegistry();
    const targetResponse = deferred<string | null>();
    const otherResponse = deferred<string | null>();
    const descriptiveCommits: Array<{ phaseId: string; label: string }> = [];
    const signals = new Map<string, AbortSignal>();

    const start = (
      phaseId: string,
      response: { promise: Promise<string | null>; resolve: (value: string | null) => void }
    ) => {
      const phase = {
        sessionId: "session",
        phaseId,
        userRequest: "Fix login",
        reasoningText: `Review ${phaseId}`
      };
      const request = registry.begin(phase)!;
      const cancel = startAgentThoughtLabelDisplay({
        request: (signal) => {
          signals.set(phaseId, signal);
          return response.promise.finally(request.finish);
        },
        commit: (label) => {
          if (!request.isCurrent()) return;
          request.recordLabel(label);
          if (label !== AGENT_THOUGHT_LABEL_PENDING_TEXT) {
            descriptiveCommits.push({ phaseId, label });
          }
        },
        scheduleFallback: () => () => {}
      });
      request.setCancel(cancel);
    };

    start("assistant-a:thought-0", targetResponse);
    start("assistant-b:thought-0", otherResponse);
    registry.cancelMatching("session", "assistant-a");
    expect(signals.get("assistant-a:thought-0")?.aborted).toBe(true);
    expect(signals.get("assistant-b:thought-0")?.aborted).toBe(false);

    targetResponse.resolve("Reviewing cancelled final request");
    otherResponse.resolve("Reviewing current final request");
    await Promise.all([targetResponse.promise, otherResponse.promise]);
    await nextTask();
    expect(descriptiveCommits).toEqual([
      { phaseId: "assistant-b:thought-0", label: "Reviewing current final request" }
    ]);
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

  test("refreshes from the latest reasoning at one, five, and fifteen seconds", async () => {
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

  test("skips unchanged snapshots and exact duplicate labels", async () => {
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

  test("aborts an obsolete snapshot and commits only the next milestone snapshot", async () => {
    const timers = manualTimers();
    const responses = [deferred<string | null>(), deferred<string | null>()];
    const requests: Array<{ reasoningText: string; signal: AbortSignal }> = [];
    const commits: Array<{ reasoningText: string; label: string }> = [];
    const scheduler = new AgentThoughtLabelProvisionalScheduler({
      schedule: timers.schedule,
      request: (source, signal) => {
        requests.push({ reasoningText: source.reasoningText, signal });
        return responses[requests.length - 1].promise;
      },
      commit: (source, label) => commits.push({ reasoningText: source.reasoningText, label })
    });
    const phaseA = phase("assistant:thought-0", 100);
    const phaseB = { ...phaseA, reasoningText: "b".repeat(120) };

    scheduler.observe(phaseA);
    timers.run(AGENT_THOUGHT_LABEL_PROVISIONAL_MILESTONES_MS[0]);
    scheduler.observe(phaseB);
    expect(requests[0].signal.aborted).toBe(true);

    timers.run(AGENT_THOUGHT_LABEL_PROVISIONAL_MILESTONES_MS[1]);
    expect(requests.map(({ reasoningText }) => reasoningText)).toEqual([
      phaseA.reasoningText,
      phaseB.reasoningText
    ]);
    responses[1].resolve("Reviewing current snapshot");
    await responses[1].promise;
    await Promise.resolve();
    responses[0].resolve("Reviewing obsolete snapshot");
    await responses[0].promise;
    await Promise.resolve();

    expect(commits).toEqual([
      { reasoningText: phaseB.reasoningText, label: "Reviewing current snapshot" }
    ]);
    expect(scheduler.complete(phaseB.sessionId, phaseB.phaseId)).toBe("Reviewing current snapshot");
  });

  test("never commits a stale result that resolves before the replacement starts", async () => {
    const timers = manualTimers();
    const responses = [deferred<string | null>(), deferred<string | null>()];
    const commits: string[] = [];
    const signals: AbortSignal[] = [];
    let requestCount = 0;
    const scheduler = new AgentThoughtLabelProvisionalScheduler({
      schedule: timers.schedule,
      request: (_source, signal) => {
        signals.push(signal);
        return responses[requestCount++].promise;
      },
      commit: (_source, label) => commits.push(label)
    });
    const phaseA = phase("assistant:thought-0", 100);
    const phaseB = { ...phaseA, userRequest: "Fix logout" };

    scheduler.observe(phaseA);
    timers.run(AGENT_THOUGHT_LABEL_PROVISIONAL_MILESTONES_MS[0]);
    scheduler.observe(phaseB);
    expect(signals[0].aborted).toBe(true);
    responses[0].resolve("Reviewing stale login request");
    await responses[0].promise;
    await Promise.resolve();
    expect(commits).toEqual([]);

    timers.run(AGENT_THOUGHT_LABEL_PROVISIONAL_MILESTONES_MS[1]);
    responses[1].resolve("Reviewing current logout request");
    await responses[1].promise;
    await Promise.resolve();
    expect(commits).toEqual(["Reviewing current logout request"]);
  });

  test("uses snapshot generations when reasoning changes from A to B and back to A", async () => {
    const timers = manualTimers();
    const responses = [deferred<string | null>(), deferred<string | null>()];
    const commits: string[] = [];
    let requestCount = 0;
    const scheduler = new AgentThoughtLabelProvisionalScheduler({
      schedule: timers.schedule,
      request: () => responses[requestCount++].promise,
      commit: (_source, label) => commits.push(label)
    });
    const phaseA = phase("assistant:thought-0", 100);
    const phaseB = { ...phaseA, reasoningText: "b".repeat(100) };

    scheduler.observe(phaseA);
    timers.run(AGENT_THOUGHT_LABEL_PROVISIONAL_MILESTONES_MS[0]);
    scheduler.observe(phaseB);
    scheduler.observe(phaseA);
    responses[0].resolve("Reviewing old A snapshot");
    await responses[0].promise;
    await Promise.resolve();
    expect(commits).toEqual([]);

    timers.run(AGENT_THOUGHT_LABEL_PROVISIONAL_MILESTONES_MS[1]);
    expect(requestCount).toBe(2);
    responses[1].resolve("Reviewing current A snapshot");
    await responses[1].promise;
    await Promise.resolve();
    expect(commits).toEqual(["Reviewing current A snapshot"]);
  });

  test("abandons a provisional at its deadline and allows the next milestone", async () => {
    const timers = manualTimers();
    const response = deferred<string | null>();
    const commits: string[] = [];
    let requestCount = 0;
    let requestSignal: AbortSignal | undefined;
    const scheduler = new AgentThoughtLabelProvisionalScheduler({
      schedule: timers.schedule,
      request: (_source, signal) => {
        requestCount += 1;
        requestSignal = signal;
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
    expect(requestSignal?.aborted).toBe(true);

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

  test("limits concurrent provisionals and retries a cap-skipped phase at its next milestone", async () => {
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
    await Promise.resolve();
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
  });

  test("cancels an in-flight provisional when its phase completes", async () => {
    const timers = manualTimers();
    const response = deferred<string | null>();
    const commits: string[] = [];
    let requestSignal: AbortSignal | undefined;
    const scheduler = new AgentThoughtLabelProvisionalScheduler({
      schedule: timers.schedule,
      request: (_source, signal) => {
        requestSignal = signal;
        return response.promise;
      },
      commit: (_source, label) => commits.push(label)
    });
    const source = phase("assistant:thought-0", AGENT_THOUGHT_LABEL_PROVISIONAL_MIN_LENGTH);
    scheduler.observe(source);
    timers.run(AGENT_THOUGHT_LABEL_PROVISIONAL_DELAY_MS);

    expect(scheduler.complete(source.sessionId, source.phaseId)).toBeNull();
    expect(requestSignal?.aborted).toBe(true);
    response.resolve("Investigating login failures");
    await response.promise;
    await Promise.resolve();
    expect(commits).toEqual([]);
  });

  test("cancels matching pending provisionals during turn invalidation", async () => {
    const timers = manualTimers();
    const response = deferred<string | null>();
    const commits: string[] = [];
    let requestSignal: AbortSignal | undefined;
    const scheduler = new AgentThoughtLabelProvisionalScheduler({
      schedule: timers.schedule,
      request: (_source, signal) => {
        requestSignal = signal;
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
    expect(requestSignal?.aborted).toBe(true);
    response.resolve("Investigating login failures");
    await response.promise;
    await Promise.resolve();
    expect(commits).toEqual([]);
  });

  test("returns the displayed provisional at completion and treats Thinking as no label", async () => {
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
  const source = { userRequest: "Fix login", reasoningText: "Inspect auth." };

  test("uses the installed OpenAI client and disables Gemma reasoning", async () => {
    let requestedUrl = "";
    let requestedBody: Record<string, unknown> | undefined;
    const controller = new AbortController();
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
                message: { role: "assistant", content: "Inspecting authentication flow" }
              }
            ]
          }),
          { status: 200, headers: { "Content-Type": "application/json" } }
        );
      },
      maxRetries: 0
    });

    await expect(
      requestAgentThoughtLabel(client, source, { signal: controller.signal })
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

  test("sends the focused prompt and validates its examples", async () => {
    const requests: Record<string, unknown>[] = [];
    const client = fakeClient(async (request) => {
      requests.push(request);
      return completion("  Inspecting authentication flow  ");
    });
    const detailedSource = {
      userRequest: "Fix login",
      reasoningText: "I traced the auth state."
    };

    await expect(requestAgentThoughtLabel(client, detailedSource)).resolves.toBe(
      "Inspecting authentication flow"
    );
    expect(requests).toHaveLength(1);

    const messages = requests[0].messages as { role: string; content: string }[];
    expect(messages).toEqual([
      { role: "system", content: expect.any(String) },
      { role: "user", content: buildAgentThoughtLabelInput(detailedSource) }
    ]);
    [
      "Treat overall_task_context only as background",
      "most specific supported subject",
      "not authoritative tool state",
      "Never begin a label with any of these tool-execution verbs",
      "Start with a natural -ing action verb",
      "Use 3 to 8 words",
      "Never use past-tense wording",
      "phase_state is either streaming or complete",
      "For complete input, always attempt a descriptive label"
    ].forEach((contract) => expect(messages[0].content).toContain(contract));

    const promptLines = messages[0].content.split("\n");
    const prematureVerbList = /finality verbs: ([^.]+)\./u.exec(messages[0].content)?.[1];
    const prematureVerbs = prematureVerbList?.split(", ") ?? [];
    const examples = promptLines.flatMap((line, index) => {
      const state = /^(streaming|complete):/u.exec(line)?.[1];
      return state ? [{ state, label: promptLines[index + 1] }] : [];
    });
    expect(examples).toHaveLength(9);
    expect(examples.filter(({ state }) => state === "streaming").length).toBeGreaterThanOrEqual(4);
    expect(examples.filter(({ state }) => state === "complete").length).toBeGreaterThanOrEqual(4);
    expect(examples.some(({ label }) => label === AGENT_THOUGHT_LABEL_PENDING_TEXT)).toBe(true);
    const exampleLabels = examples.map(({ label }) => label).join("\n");
    ["agentThoughtLabels", "package", "/downloads", "UseCaseGridSection", "llms.txt"].forEach(
      (subject) => expect(exampleLabels).toContain(subject)
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
  });

  test("sends streaming and complete phase states independently", async () => {
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

    await expect(
      requestAgentThoughtLabel(client, source, { phaseState: "streaming" })
    ).resolves.toBe("Investigating login failures");
    await expect(
      requestAgentThoughtLabel(client, source, { phaseState: "complete" })
    ).resolves.toBe("Explaining authentication findings");
    expect(requestedStates).toEqual(["streaming", "complete"]);
  });

  test("keeps concurrent snapshots independently cancellable", async () => {
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
    const firstController = new AbortController();
    const secondController = new AbortController();

    const first = requestAgentThoughtLabel(client, source, {
      phaseState: "streaming",
      signal: firstController.signal
    });
    const second = requestAgentThoughtLabel(client, source, {
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

  test("does not contact the provider when already aborted", async () => {
    let requestCount = 0;
    const controller = new AbortController();
    controller.abort();
    const client = fakeClient(async () => {
      requestCount += 1;
      return completion("Inspecting authentication flow");
    });

    await expect(
      requestAgentThoughtLabel(client, source, { signal: controller.signal })
    ).resolves.toBeNull();
    expect(requestCount).toBe(0);
  });

  test("returns null when aborted while the provider response is pending", async () => {
    const response = deferred<FakeChatCompletion>();
    let requestSignal: AbortSignal | undefined;
    const client = {
      chat: {
        completions: {
          create: (_request: unknown, options?: { signal?: AbortSignal }) => {
            requestSignal = options?.signal;
            return response.promise;
          }
        }
      }
    } as unknown as AgentThoughtLabelClient;
    const controller = new AbortController();
    const pending = requestAgentThoughtLabel(client, source, { signal: controller.signal });
    await nextTask();
    expect(requestSignal).toBe(controller.signal);

    controller.abort();
    response.resolve(completion("Inspecting obsolete authentication flow"));
    await expect(pending).resolves.toBeNull();
  });

  test("allows Thinking only as a streaming decline and rejects clipped output", async () => {
    await expect(
      requestAgentThoughtLabel(
        fakeClient(async () => completion(AGENT_THOUGHT_LABEL_PENDING_TEXT)),
        source,
        { phaseState: "streaming" }
      )
    ).resolves.toBe(AGENT_THOUGHT_LABEL_PENDING_TEXT);
    await expect(
      requestAgentThoughtLabel(
        fakeClient(async () => completion(AGENT_THOUGHT_LABEL_PENDING_TEXT)),
        source,
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
        source
      )
    ).resolves.toBeNull();
  });

  test("blocks premature streaming verbs but permits active wording", async () => {
    const seoSource = {
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
    for (const label of prematureLabels) {
      await expect(
        requestAgentThoughtLabel(
          fakeClient(async () => completion(label)),
          seoSource,
          {
            phaseState: "streaming"
          }
        )
      ).resolves.toBeNull();
    }

    const activeLabels = [
      "Synthesizing remaining SEO evidence",
      "Generating parser edge cases",
      "Sharing state across components"
    ];
    for (const label of activeLabels) {
      await expect(
        requestAgentThoughtLabel(
          fakeClient(async () => completion(label)),
          seoSource,
          {
            phaseState: "streaming"
          }
        )
      ).resolves.toBe(label);
    }
    await expect(
      requestAgentThoughtLabel(
        fakeClient(async () => completion("Formulating SEO recommendations")),
        seoSource,
        { phaseState: "complete" }
      )
    ).resolves.toBe("Formulating SEO recommendations");
  });

  test("rejects tool-execution leading verbs for streaming and complete labels", async () => {
    const toolExecutionLabels = [
      "Calling the authentication endpoint",
      "Executing the migration command",
      "Fetching current sitemap records",
      "Listing route configuration files",
      "Opening the package manifest",
      "Querying the session database",
      "Reading agentThoughtLabels and package configuration",
      '"Reading agentThoughtLabels and package configuration"',
      "• Reading agentThoughtLabels and package configuration",
      "Running AgentMode lifecycle tests",
      "Searching localStorage persistence paths",
      "Testing failed-run label retention"
    ];

    for (const phaseState of ["streaming", "complete"] as const) {
      for (const label of toolExecutionLabels) {
        await expect(
          requestAgentThoughtLabel(
            fakeClient(async () => completion(label)),
            source,
            {
              phaseState
            }
          )
        ).resolves.toBeNull();
      }
    }
    await expect(
      requestAgentThoughtLabel(
        fakeClient(async () => completion("Reviewing agentThoughtLabels configuration")),
        source,
        { phaseState: "complete" }
      )
    ).resolves.toBe("Reviewing agentThoughtLabels configuration");
  });

  test("returns null when the provider request fails", async () => {
    const failedClient = fakeClient(async () => {
      throw new Error("Provider unavailable");
    });

    await expect(requestAgentThoughtLabel(failedClient, source)).resolves.toBeNull();
  });

  test("rejects missing and unfinished completion choices", async () => {
    const clients = [
      fakeClient(async () => ({ choices: [] })),
      fakeClient(async () => ({
        choices: [
          {
            finish_reason: "content_filter",
            message: { content: "Reviewing authentication flow" }
          }
        ]
      }))
    ];

    for (const client of clients) {
      await expect(requestAgentThoughtLabel(client, source)).resolves.toBeNull();
    }
  });

  test("rejects invalid visible output without rewriting it", async () => {
    const outputs = [
      "First line\nSecond line",
      `Label_${"x".repeat(AGENT_THOUGHT_LABEL_MAX_LENGTH)}`,
      "Reviewing authentication",
      "Reviewing one two three four five six seven eight",
      "Review authentication flow",
      '"Reviewing authentication flow"',
      "• Reviewing authentication flow",
      "Reviewing authentication flow.",
      "Reviewing authentication flow,",
      "Reviewing authentication flow…",
      "Reviewing authentication flow)",
      null
    ];

    for (const output of outputs) {
      await expect(
        requestAgentThoughtLabel(
          fakeClient(async () => completion(output)),
          source
        )
      ).resolves.toBeNull();
    }
  });

  test("never reads hidden message reasoning when visible output is absent", async () => {
    const message: FakeChatCompletion["choices"][number]["message"] = { content: null };
    Object.defineProperty(message, "reasoning", {
      get() {
        throw new Error("Hidden reasoning must not be read");
      }
    });
    const client = fakeClient(async () => ({
      choices: [{ finish_reason: "stop", message }]
    }));

    await expect(requestAgentThoughtLabel(client, source)).resolves.toBeNull();
  });
});
