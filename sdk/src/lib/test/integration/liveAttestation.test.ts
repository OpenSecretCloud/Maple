import { afterEach, beforeEach, expect, test } from "bun:test";
import { clearAttestationSessions, getAttestation } from "../../getAttestation";

const liveApiUrl = process.env.VITE_LIVE_ATTESTATION_API_URL;
const runLive = process.env.RUN_LIVE_ATTESTATION === "1" && Boolean(liveApiUrl);
const originalFetch = globalThis.fetch;

function requestUrl(input: RequestInfo | URL): string {
  return typeof input === "string" ? input : input instanceof URL ? input.toString() : input.url;
}

beforeEach(() => {
  clearAttestationSessions();
});

afterEach(() => {
  globalThis.fetch = originalFetch;
  clearAttestationSessions();
});

test.skipIf(!runLive)(
  "hosted enclave establishes a session only after signed-history PCR0 validation",
  async () => {
    const requests: string[] = [];
    globalThis.fetch = async (input, init) => {
      requests.push(requestUrl(input));
      return originalFetch(input, init);
    };

    const attestation = await getAttestation(true, liveApiUrl, {
      environment: "development"
    });

    expect(attestation.sessionKey).toHaveLength(32);
    expect(attestation.sessionId).toBeTruthy();

    const attestationIndex = requests.findIndex((url) => url.includes("/attestation/"));
    const prodHistoryIndex = requests.findIndex((url) => url.endsWith("/pcrProdHistory.json"));
    const devHistoryIndex = requests.findIndex((url) => url.endsWith("/pcrDevHistory.json"));
    const keyExchangeIndex = requests.findIndex((url) => url.endsWith("/key_exchange"));

    expect(attestationIndex).toBeGreaterThanOrEqual(0);
    expect(prodHistoryIndex).toBe(-1);
    expect(devHistoryIndex).toBeGreaterThan(attestationIndex);
    expect(keyExchangeIndex).toBeGreaterThan(devHistoryIndex);
  }
);

test.skipIf(!runLive)(
  "hosted enclave cannot reach key exchange when its PCR0 policy is deliberately unknown",
  async () => {
    const requests: string[] = [];
    globalThis.fetch = async (input, init) => {
      requests.push(requestUrl(input));
      return originalFetch(input, init);
    };

    await expect(
      getAttestation(true, liveApiUrl, {
        environment: "development",
        remoteAttestation: false
      })
    ).rejects.toThrow(/PCR0/i);

    expect(requests.filter((url) => url.includes("/attestation/"))).toHaveLength(1);
    expect(requests.filter((url) => url.endsWith("/key_exchange"))).toHaveLength(0);
  }
);

test.skipIf(!runLive)(
  "hosted development enclave is rejected by the default production policy",
  async () => {
    const requests: string[] = [];
    globalThis.fetch = async (input, init) => {
      requests.push(requestUrl(input));
      return originalFetch(input, init);
    };

    await expect(getAttestation(true, liveApiUrl)).rejects.toThrow(/PCR0/i);

    expect(requests.filter((url) => url.includes("/attestation/"))).toHaveLength(1);
    expect(requests.filter((url) => url.endsWith("/pcrProdHistory.json"))).toHaveLength(1);
    expect(requests.filter((url) => url.endsWith("/pcrDevHistory.json"))).toHaveLength(0);
    expect(requests.filter((url) => url.endsWith("/key_exchange"))).toHaveLength(0);
  }
);
