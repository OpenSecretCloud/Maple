import { describe, expect, test } from "bun:test";
import {
  AGENT_REMOTE_PERSISTED_TRANSCRIPT_CAPABILITIES,
  decodeAgentRemoteCapabilitySnapshot,
  isAgentRemotePersistedTranscriptReady,
  sameAgentRemoteCapabilitySnapshot
} from "@/services/agentRemoteCapabilities";

describe("Agent remote transcript capabilities", () => {
  test("admits only the exact persisted-transcript grant", () => {
    expect(
      isAgentRemotePersistedTranscriptReady(AGENT_REMOTE_PERSISTED_TRANSCRIPT_CAPABILITIES)
    ).toBe(true);
    expect(
      isAgentRemotePersistedTranscriptReady({
        ...AGENT_REMOTE_PERSISTED_TRANSCRIPT_CAPABILITIES,
        persistedRecordsPage: false
      })
    ).toBe(false);
    expect(
      isAgentRemotePersistedTranscriptReady({
        ...AGENT_REMOTE_PERSISTED_TRANSCRIPT_CAPABILITIES,
        synchronizedLiveTail: true
      })
    ).toBe(false);
    expect(
      isAgentRemotePersistedTranscriptReady({
        ...AGENT_REMOTE_PERSISTED_TRANSCRIPT_CAPABILITIES,
        mutations: true
      })
    ).toBe(false);
  });

  test("rejects missing, non-boolean, and extension fields", () => {
    const missing: Record<string, unknown> = {
      ...AGENT_REMOTE_PERSISTED_TRANSCRIPT_CAPABILITIES
    };
    Reflect.deleteProperty(missing, "mutations");
    expect(decodeAgentRemoteCapabilitySnapshot(missing)).toBeNull();
    expect(
      decodeAgentRemoteCapabilitySnapshot({
        ...AGENT_REMOTE_PERSISTED_TRANSCRIPT_CAPABILITIES,
        runtimeStatus: "yes"
      })
    ).toBeNull();
    expect(
      decodeAgentRemoteCapabilitySnapshot({
        ...AGENT_REMOTE_PERSISTED_TRANSCRIPT_CAPABILITIES,
        sendMessage: true
      })
    ).toBeNull();

    const hiddenExtension = { ...AGENT_REMOTE_PERSISTED_TRANSCRIPT_CAPABILITIES };
    Object.defineProperty(hiddenExtension, "sendMessage", { value: true });
    expect(decodeAgentRemoteCapabilitySnapshot(hiddenExtension)).toBeNull();

    const symbolExtension = { ...AGENT_REMOTE_PERSISTED_TRANSCRIPT_CAPABILITIES };
    Object.defineProperty(symbolExtension, Symbol("sendMessage"), { value: true });
    expect(decodeAgentRemoteCapabilitySnapshot(symbolExtension)).toBeNull();
  });

  test("returns a frozen closed copy instead of retaining provider data", () => {
    const input = { ...AGENT_REMOTE_PERSISTED_TRANSCRIPT_CAPABILITIES };
    const decoded = decodeAgentRemoteCapabilitySnapshot(input);
    expect(decoded).not.toBe(input);
    expect(Object.isFrozen(decoded)).toBe(true);
    expect(
      sameAgentRemoteCapabilitySnapshot(decoded!, AGENT_REMOTE_PERSISTED_TRANSCRIPT_CAPABILITIES)
    ).toBe(true);
  });
});
