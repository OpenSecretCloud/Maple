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
  "hosted enclave establishes a session only after embedded trusted-release validation",
  async () => {
    const requests: string[] = [];
    globalThis.fetch = async (input, init) => {
      requests.push(requestUrl(input));
      return originalFetch(input, init);
    };

    const attestation = await getAttestation(true, liveApiUrl, {
      environment: "dev"
    });

    expect(attestation.sessionKey).toHaveLength(32);
    expect(attestation.sessionId).toBeTruthy();

    const attestationIndex = requests.findIndex((url) => url.includes("/attestation/"));
    const keyExchangeIndex = requests.findIndex((url) => url.endsWith("/key_exchange"));

    expect(attestationIndex).toBeGreaterThanOrEqual(0);
    expect(requests.some((url) => url.includes("pcrDevHistory.json"))).toBe(false);
    expect(requests.some((url) => url.includes("pcrProdHistory.json"))).toBe(false);
    expect(keyExchangeIndex).toBeGreaterThan(attestationIndex);
  }
);

test.skipIf(!runLive)(
  "hosted enclave cannot reach key exchange under the wrong trusted-release environment",
  async () => {
    const requests: string[] = [];
    globalThis.fetch = async (input, init) => {
      requests.push(requestUrl(input));
      return originalFetch(input, init);
    };

    await expect(
      getAttestation(true, liveApiUrl, {
        environment: "prod"
      })
    ).rejects.toThrow(/environment|PCR/i);

    expect(requests.filter((url) => url.includes("/attestation/"))).toHaveLength(1);
    expect(requests.filter((url) => url.endsWith("/key_exchange"))).toHaveLength(0);
  }
);

test.skipIf(!runLive)(
  "hosted development enclave is rejected by an incompatible explicit policy",
  async () => {
    const requests: string[] = [];
    globalThis.fetch = async (input, init) => {
      requests.push(requestUrl(input));
      return originalFetch(input, init);
    };

    await expect(getAttestation(true, liveApiUrl, { environment: "prod" })).rejects.toThrow(
      /environment|PCR/i
    );

    expect(requests.filter((url) => url.includes("/attestation/"))).toHaveLength(1);
    expect(requests.filter((url) => url.includes("History.json"))).toHaveLength(0);
    expect(requests.filter((url) => url.endsWith("/key_exchange"))).toHaveLength(0);
  }
);
