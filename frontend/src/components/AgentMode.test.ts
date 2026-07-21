import { describe, expect, test } from "bun:test";
import { handleAgentModeThoughtRunFinished } from "./agent/agentModeThoughtRun";
import type { AgentEventEnvelope, AgentTimelineItem } from "@/services/agentRuntimeService";
import {
  AGENT_THOUGHT_LABEL_PENDING_TEXT,
  AGENT_THOUGHT_LABEL_PROVISIONAL_DELAY_MS,
  AGENT_THOUGHT_LABEL_PROVISIONAL_MILESTONES_MS,
  AgentThoughtLabelFinalRequestRegistry,
  AgentThoughtLabelProvisionalScheduler,
  startAgentThoughtLabelDisplay
} from "@/services/agentThoughtLabels";
import { AgentLiveThoughtPhaseTracker, type AgentThoughtPhase } from "@/services/agentTimeline";

const SESSION_ID = "session";
const PROVISIONAL_LABEL = "Investigating login flow";
const REASONING =
  "Evaluating the login flow and tracing session state across runtime events before choosing the safest correction ".repeat(
    2
  );

function deferred<T>(): { promise: Promise<T>; resolve: (value: T) => void } {
  let resolve: ((value: T) => void) | undefined;
  const promise = new Promise<T>((nextResolve) => {
    resolve = nextResolve;
  });
  return { promise, resolve: (value) => resolve?.(value) };
}

function manualTimers(): {
  schedule: (callback: () => void, delayMs: number) => () => void;
  run: (delayMs: number) => void;
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
      timers
        .filter((timer) => !timer.cancelled && timer.delayMs === delayMs)
        .forEach((timer) => {
          timer.cancelled = true;
          timer.callback();
        });
    }
  };
}

function timeline(reasoningText = REASONING): AgentTimelineItem[] {
  return [
    {
      id: "user",
      itemType: "message",
      role: "user",
      text: "Inspect login",
      createdMs: 0,
      merge: "replace"
    },
    {
      id: "thought",
      itemType: "thinking",
      role: "thought",
      text: reasoningText,
      createdMs: 1,
      merge: "replace"
    }
  ];
}

function liveTracker(items = timeline()): AgentLiveThoughtPhaseTracker {
  const tracker = new AgentLiveThoughtPhaseTracker();
  items.forEach((item) => tracker.observeTimelineItem(SESSION_ID, item));
  return tracker;
}

async function flushPromises(): Promise<void> {
  await Promise.resolve();
  await Promise.resolve();
}

describe("AgentMode runFinished thought labels", () => {
  for (const status of ["completed", "failed"] as const) {
    test(`${status} preserves its provisional through the authoritative reload`, async () => {
      const authoritativeTimeline = timeline();
      const tracker = liveTracker(authoritativeTimeline);
      const timers = manualTimers();
      const finalResponse = deferred<string | null>();
      const finalRegistry = new AgentThoughtLabelFinalRequestRegistry();
      const visibleLabels: Array<string | null> = [AGENT_THOUGHT_LABEL_PENDING_TEXT];
      let visibleLabel: string | null = AGENT_THOUGHT_LABEL_PENDING_TEXT;
      let finalRequestCount = 0;
      let replaceCount = 0;
      const setVisibleLabel = (label: string | null) => {
        if (visibleLabel === label) return;
        visibleLabel = label;
        visibleLabels.push(label);
      };
      const provisionalScheduler = new AgentThoughtLabelProvisionalScheduler({
        schedule: timers.schedule,
        request: async () => PROVISIONAL_LABEL,
        commit: (_phase, label) => setVisibleLabel(label)
      });
      provisionalScheduler.observe(tracker.activePhase(SESSION_ID)!);
      timers.run(AGENT_THOUGHT_LABEL_PROVISIONAL_DELAY_MS);
      await flushPromises();
      expect(visibleLabel).toBe(PROVISIONAL_LABEL);

      const finalizePhase = (phase: AgentThoughtPhase) => {
        const retainedLabel = provisionalScheduler.complete(phase.sessionId, phase.phaseId);
        const finalRequest = finalRegistry.begin(phase, retainedLabel);
        if (!finalRequest) return;
        finalRequestCount += 1;
        const cancel = startAgentThoughtLabelDisplay({
          retainedLabel: finalRequest.retainedLabel,
          request: () => finalResponse.promise.finally(finalRequest.finish),
          commit: (label, expectedLabel) => {
            if (!finalRequest.isCurrent()) return;
            if (expectedLabel !== undefined && visibleLabel !== expectedLabel) return;
            finalRequest.recordLabel(label);
            setVisibleLabel(label);
          }
        });
        finalRequest.setCancel(cancel);
      };

      const finished = handleAgentModeThoughtRunFinished({
        event: {
          eventType: "runFinished",
          sessionId: SESSION_ID,
          runId: "run",
          message: status
        },
        timelineRevision: 7,
        tracker,
        finalizePhase,
        releaseProvisional: (phase) => {
          provisionalScheduler.complete(phase.sessionId, phase.phaseId);
        },
        cancelAndInvalidateLabels: () => {
          throw new Error(`${status} must not invalidate labels`);
        },
        loadTimeline: async () => authoritativeTimeline,
        canApplyTimeline: () => true,
        replaceTimeline: (sessionId, loadedTimeline, revision) => {
          expect(sessionId).toBe(SESSION_ID);
          expect(loadedTimeline).toBe(authoritativeTimeline);
          expect(revision).toBe(7);
          replaceCount += 1;
          return true;
        }
      });

      expect(finished).not.toBeNull();
      expect(visibleLabel).toBe(PROVISIONAL_LABEL);
      expect(finalRequestCount).toBe(1);
      await finished;
      expect(replaceCount).toBe(1);
      expect(finalRequestCount).toBe(1);
      expect(visibleLabels).toEqual([AGENT_THOUGHT_LABEL_PENDING_TEXT, PROVISIONAL_LABEL]);

      const finalLabel = status === "completed" ? "Explaining final login findings" : null;
      finalResponse.resolve(finalLabel);
      await finalResponse.promise;
      await flushPromises();
      expect(visibleLabel).toBe(finalLabel ?? PROVISIONAL_LABEL);
      expect(visibleLabels).toEqual(
        finalLabel
          ? [AGENT_THOUGHT_LABEL_PENDING_TEXT, PROVISIONAL_LABEL, finalLabel]
          : [AGENT_THOUGHT_LABEL_PENDING_TEXT, PROVISIONAL_LABEL]
      );
    });
  }

  test("cancelled discards its provisional and ignores an obsolete request", async () => {
    const tracker = liveTracker();
    const timers = manualTimers();
    const obsoleteResponse = deferred<string | null>();
    const finalRegistry = new AgentThoughtLabelFinalRequestRegistry();
    const provisionalSignals: AbortSignal[] = [];
    let provisionalRequestCount = 0;
    let provisionalCommitCount = 0;
    let finalRequestCount = 0;
    let visibleLabel: string | null = AGENT_THOUGHT_LABEL_PENDING_TEXT;
    const provisionalScheduler = new AgentThoughtLabelProvisionalScheduler({
      schedule: timers.schedule,
      request: (_phase, signal) => {
        provisionalSignals.push(signal);
        provisionalRequestCount += 1;
        return provisionalRequestCount === 1
          ? Promise.resolve(PROVISIONAL_LABEL)
          : obsoleteResponse.promise;
      },
      commit: (_phase, label) => {
        provisionalCommitCount += 1;
        visibleLabel = label;
      }
    });
    provisionalScheduler.observe(tracker.activePhase(SESSION_ID)!);
    timers.run(AGENT_THOUGHT_LABEL_PROVISIONAL_DELAY_MS);
    await flushPromises();
    expect(visibleLabel).toBe(PROVISIONAL_LABEL);

    tracker.observeTimelineItem(SESSION_ID, {
      id: "thought",
      itemType: "thinking",
      role: "thought",
      text: " while checking redirect handling",
      createdMs: 2,
      merge: "append"
    });
    provisionalScheduler.observe(tracker.activePhase(SESSION_ID)!);
    timers.run(AGENT_THOUGHT_LABEL_PROVISIONAL_MILESTONES_MS[1]);
    expect(provisionalSignals).toHaveLength(2);

    const finished = handleAgentModeThoughtRunFinished({
      event: {
        eventType: "runFinished",
        sessionId: SESSION_ID,
        runId: "run",
        message: "cancelled"
      },
      tracker,
      finalizePhase: () => {
        finalRequestCount += 1;
      },
      releaseProvisional: () => {},
      cancelAndInvalidateLabels: (sessionId, assistantTurnId) => {
        provisionalScheduler.cancelMatching(sessionId, assistantTurnId ?? undefined);
        finalRegistry.cancelMatching(sessionId, assistantTurnId ?? undefined);
        visibleLabel = null;
      },
      loadTimeline: async () => timeline(`${REASONING} while checking redirect handling`),
      canApplyTimeline: () => true,
      replaceTimeline: () => true
    });

    expect(finished).not.toBeNull();
    expect(provisionalSignals[1].aborted).toBe(true);
    expect(visibleLabel).toBeNull();
    await finished;
    expect(finalRequestCount).toBe(0);

    obsoleteResponse.resolve("Investigating cancelled login flow");
    await obsoleteResponse.promise;
    await flushPromises();
    expect(provisionalCommitCount).toBe(1);
    expect(visibleLabel).toBeNull();
  });

  test("ignores events outside the terminal run boundary", () => {
    const event: AgentEventEnvelope = {
      eventType: "runFinished",
      sessionId: SESSION_ID,
      message: "unexpected"
    };
    let loadCount = 0;

    const finished = handleAgentModeThoughtRunFinished({
      event,
      tracker: liveTracker(),
      finalizePhase: () => {},
      releaseProvisional: () => {},
      cancelAndInvalidateLabels: () => {},
      loadTimeline: async () => {
        loadCount += 1;
        return [];
      },
      canApplyTimeline: () => true,
      replaceTimeline: () => true
    });

    expect(finished).toBeNull();
    expect(loadCount).toBe(0);
  });
});
