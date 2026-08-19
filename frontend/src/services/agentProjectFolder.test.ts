import { describe, expect, test } from "bun:test";

import {
  canUseLocalAgentProjectFolderActions,
  revealAgentProjectFolder
} from "./agentProjectFolder";
import {
  LOCAL_AGENT_EXECUTION_TARGET,
  createRemoteAgentExecutionTarget
} from "./agentRuntimeService";

describe("canUseLocalAgentProjectFolderActions", () => {
  test("allows host folder actions only for the local desktop target", () => {
    const remoteTarget = createRemoteAgentExecutionTarget("paired-mac", "Paired Mac");

    expect(canUseLocalAgentProjectFolderActions(LOCAL_AGENT_EXECUTION_TARGET, true)).toBe(true);
    expect(canUseLocalAgentProjectFolderActions(LOCAL_AGENT_EXECUTION_TARGET, false)).toBe(false);
    expect(canUseLocalAgentProjectFolderActions(remoteTarget, true)).toBe(false);
    expect(canUseLocalAgentProjectFolderActions(remoteTarget, false)).toBe(false);
  });
});

describe("revealAgentProjectFolder", () => {
  test("reveals the exact canonical project path once", async () => {
    const projectPath = "/Users/example/My Project";
    const revealedPaths: string[] = [];

    await revealAgentProjectFolder(projectPath, async (path) => {
      revealedPaths.push(path);
    });

    expect(revealedPaths).toEqual([projectPath]);
  });

  test.each(["", "   \n\t"])("rejects a blank project path", async (projectPath) => {
    let revealCallCount = 0;

    await expect(
      revealAgentProjectFolder(projectPath, async () => {
        revealCallCount += 1;
      })
    ).rejects.toThrow("Project folder path is required.");

    expect(revealCallCount).toBe(0);
  });

  test("propagates reveal failures", async () => {
    const revealError = new Error("folder is unavailable");
    let revealCallCount = 0;
    let caughtError: unknown;

    try {
      await revealAgentProjectFolder("/missing/project", async () => {
        revealCallCount += 1;
        throw revealError;
      });
    } catch (error) {
      caughtError = error;
    }

    expect(revealCallCount).toBe(1);
    expect(caughtError).toBe(revealError);
  });
});
