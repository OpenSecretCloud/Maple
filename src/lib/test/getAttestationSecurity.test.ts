import { beforeEach, describe, expect, mock, test } from "bun:test";
import { encode } from "@stablelib/base64";
import type { AttestationDocument } from "../attestation";
import {
  getAttestationSessionStorageKey,
  getAttestationWithDependencies,
  type GetAttestationDependencies
} from "../getAttestation";
import type { PcrConfig } from "../pcr";

const REMOTE_API_URL = "https://enclave.example.test/api";
const LOCAL_API_URL = "http://127.0.0.1:31110";
const ATTESTATION_NONCE = "00000000-0000-4000-8000-000000000001";
const TRUSTED_PCR0 = new Uint8Array(48).fill(0x2a);
const SESSION_KEY = new Uint8Array(32).fill(0x5a);
const PCR_CONFIG: PcrConfig = {
  pcr0Values: [bytesToHex(TRUSTED_PCR0)],
  remoteAttestation: false
};

function bytesToHex(bytes: Uint8Array): string {
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("");
}

function attestationDocument(pcr0?: Uint8Array): AttestationDocument {
  const pcrs = new Map<number, Uint8Array>();
  if (pcr0) pcrs.set(0, pcr0);

  return {
    module_id: "test-enclave",
    digest: "SHA384",
    timestamp: Date.now(),
    pcrs,
    certificate: new Uint8Array([1]),
    cabundle: [new Uint8Array([2])],
    public_key: new Uint8Array(32).fill(0x19),
    user_data: null,
    nonce: new TextEncoder().encode(ATTESTATION_NONCE)
  };
}

function dependencies(
  overrides: Partial<GetAttestationDependencies> = {}
): GetAttestationDependencies {
  return {
    verifyAttestation: async () => attestationDocument(TRUSTED_PCR0),
    validatePcr0Hash: async () => ({
      isMatch: true,
      text: "PCR0 matches a test trust root"
    }),
    keyExchange: async () => ({
      encrypted_session_key: "test-encrypted-session-key",
      session_id: "trusted-session"
    }),
    generateNaclKeyPair: () => ({
      publicKey: new Uint8Array(32).fill(0x11),
      secretKey: new Uint8Array(32).fill(0x12)
    }),
    decryptSessionKey: () => SESSION_KEY,
    randomUUID: () => ATTESTATION_NONCE,
    ...overrides
  };
}

async function establish(
  deps: GetAttestationDependencies,
  apiUrl = REMOTE_API_URL,
  pcrConfig: PcrConfig | undefined = PCR_CONFIG
) {
  return getAttestationWithDependencies(false, apiUrl, pcrConfig, deps);
}

describe("attested session establishment", () => {
  beforeEach(() => {
    window.sessionStorage.clear();
  });

  test("rejects an attestation with no PCR0 before key exchange", async () => {
    const keyExchange = mock(async () => ({
      encrypted_session_key: "must-not-be-used",
      session_id: "must-not-be-created"
    }));
    const validatePcr0Hash = mock(async () => ({
      isMatch: true,
      text: "must not validate a missing PCR"
    }));

    await expect(
      establish(
        dependencies({
          verifyAttestation: async () => attestationDocument(),
          validatePcr0Hash,
          keyExchange
        })
      )
    ).rejects.toThrow(/PCR0/i);

    expect(validatePcr0Hash).not.toHaveBeenCalled();
    expect(keyExchange).not.toHaveBeenCalled();
    expect(window.sessionStorage.length).toBe(0);
  });

  test("rejects a PCR0 with the wrong SHA-384 length before key exchange", async () => {
    const keyExchange = mock(async () => ({
      encrypted_session_key: "must-not-be-used",
      session_id: "must-not-be-created"
    }));
    const validatePcr0Hash = mock(async () => ({
      isMatch: true,
      text: "must not validate a malformed PCR"
    }));

    await expect(
      establish(
        dependencies({
          verifyAttestation: async () => attestationDocument(new Uint8Array(47)),
          validatePcr0Hash,
          keyExchange
        })
      )
    ).rejects.toThrow(/PCR0/i);

    expect(validatePcr0Hash).not.toHaveBeenCalled();
    expect(keyExchange).not.toHaveBeenCalled();
    expect(window.sessionStorage.length).toBe(0);
  });

  test("rejects an all-zero PCR0 before validation or key exchange", async () => {
    const keyExchange = mock(async () => ({
      encrypted_session_key: "must-not-be-used",
      session_id: "must-not-be-created"
    }));
    const validatePcr0Hash = mock(async () => ({
      isMatch: true,
      text: "must not validate an all-zero PCR"
    }));

    await expect(
      establish(
        dependencies({
          verifyAttestation: async () => attestationDocument(new Uint8Array(48)),
          validatePcr0Hash,
          keyExchange
        })
      )
    ).rejects.toThrow(/PCR0/i);

    expect(validatePcr0Hash).not.toHaveBeenCalled();
    expect(keyExchange).not.toHaveBeenCalled();
    expect(window.sessionStorage.length).toBe(0);
  });

  test("rejects an unknown PCR0 before key exchange and leaves no session cache", async () => {
    const keyExchange = mock(async () => ({
      encrypted_session_key: "must-not-be-used",
      session_id: "must-not-be-created"
    }));
    const validatePcr0Hash = mock(async () => ({
      isMatch: false,
      text: "PCR0 does not match a known good value"
    }));

    await expect(establish(dependencies({ validatePcr0Hash, keyExchange }))).rejects.toThrow(
      /PCR0/i
    );

    expect(validatePcr0Hash).toHaveBeenCalledWith(
      bytesToHex(TRUSTED_PCR0),
      expect.objectContaining(PCR_CONFIG)
    );
    expect(keyExchange).not.toHaveBeenCalled();
    expect(window.sessionStorage.length).toBe(0);
  });

  test("an allowed PCR0 establishes and reuses a policy-scoped cached session", async () => {
    const verifyAttestation = mock(async () => attestationDocument(TRUSTED_PCR0));
    const validatePcr0Hash = mock(async () => ({
      isMatch: true,
      text: "PCR0 matches a test trust root"
    }));
    const keyExchange = mock(async () => ({
      encrypted_session_key: "test-encrypted-session-key",
      session_id: "trusted-session"
    }));
    const deps = dependencies({ verifyAttestation, validatePcr0Hash, keyExchange });

    const established = await establish(deps);
    const cached = await establish(deps);

    expect(established).toEqual({ sessionKey: SESSION_KEY, sessionId: "trusted-session" });
    expect(cached).toEqual(established);
    expect(verifyAttestation).toHaveBeenCalledTimes(1);
    expect(validatePcr0Hash).toHaveBeenCalledTimes(1);
    expect(validatePcr0Hash).toHaveBeenCalledWith(
      bytesToHex(TRUSTED_PCR0),
      expect.objectContaining(PCR_CONFIG)
    );
    expect(keyExchange).toHaveBeenCalledTimes(1);
    expect(
      window.sessionStorage.getItem(
        await getAttestationSessionStorageKey(REMOTE_API_URL, PCR_CONFIG)
      )
    ).not.toBeNull();
    expect(window.sessionStorage.getItem("sessionKey")).toBeNull();
    expect(window.sessionStorage.getItem("sessionId")).toBeNull();
  });

  test("a policy change cannot reuse a session established under the old trust roots", async () => {
    await establish(dependencies());
    const changedPolicy: PcrConfig = {
      pcr0Values: ["4c".repeat(48)],
      remoteAttestation: false
    };
    const keyExchange = mock(async () => ({
      encrypted_session_key: "must-not-be-used",
      session_id: "must-not-be-created"
    }));
    const verifyAttestation = mock(async () => attestationDocument(TRUSTED_PCR0));

    await expect(
      establish(
        dependencies({
          verifyAttestation,
          validatePcr0Hash: async () => ({ isMatch: false, text: "not in changed policy" }),
          keyExchange
        }),
        REMOTE_API_URL,
        changedPolicy
      )
    ).rejects.toThrow(/PCR0/i);

    expect(verifyAttestation).toHaveBeenCalledTimes(1);
    expect(keyExchange).not.toHaveBeenCalled();
  });

  test("ignores and removes a legacy unscoped cached session", async () => {
    window.sessionStorage.setItem("sessionKey", encode(new Uint8Array(32).fill(0xff)));
    window.sessionStorage.setItem("sessionId", "legacy-session");
    const verifyAttestation = mock(async () => attestationDocument(TRUSTED_PCR0));
    const keyExchange = mock(async () => ({
      encrypted_session_key: "test-encrypted-session-key",
      session_id: "fresh-session"
    }));

    const result = await establish(dependencies({ verifyAttestation, keyExchange }));

    expect(result).toEqual({ sessionKey: SESSION_KEY, sessionId: "fresh-session" });
    expect(verifyAttestation).toHaveBeenCalledTimes(1);
    expect(keyExchange).toHaveBeenCalledTimes(1);
    expect(window.sessionStorage.getItem("sessionKey")).toBeNull();
    expect(window.sessionStorage.getItem("sessionId")).toBeNull();
  });

  test("force refresh evicts the verified session before a failed re-attestation", async () => {
    await establish(dependencies());
    const cacheKey = await getAttestationSessionStorageKey(REMOTE_API_URL, PCR_CONFIG);
    expect(window.sessionStorage.getItem(cacheKey)).not.toBeNull();

    const verifyAttestation = mock(async () => {
      throw new Error("attestation unavailable");
    });
    await expect(
      getAttestationWithDependencies(
        true,
        REMOTE_API_URL,
        PCR_CONFIG,
        dependencies({ verifyAttestation })
      )
    ).rejects.toThrow("attestation unavailable");

    expect(window.sessionStorage.getItem(cacheKey)).toBeNull();
  });

  test("key exchange failure cannot leave a partial session cache", async () => {
    const keyExchange = mock(async () => {
      throw new Error("key exchange unavailable");
    });

    await expect(establish(dependencies({ keyExchange }))).rejects.toThrow(
      "key exchange unavailable"
    );

    expect(keyExchange).toHaveBeenCalledTimes(1);
    expect(window.sessionStorage.length).toBe(0);
  });

  test("bypasses PCR validation only for an exact HTTP loopback API URL", async () => {
    const validatePcr0Hash = mock(async () => {
      throw new Error("loopback must not invoke PCR validation");
    });
    const keyExchange = mock(async () => ({
      encrypted_session_key: "test-encrypted-session-key",
      session_id: "local-session"
    }));

    const result = await establish(
      dependencies({
        verifyAttestation: async () => attestationDocument(),
        validatePcr0Hash,
        keyExchange
      }),
      LOCAL_API_URL
    );

    expect(result).toEqual({ sessionKey: SESSION_KEY, sessionId: "local-session" });
    expect(validatePcr0Hash).not.toHaveBeenCalled();
    expect(keyExchange).toHaveBeenCalledTimes(1);
  });

  test.each(["https://localhost:31110", "http://localhost.example.test:31110"])(
    "does not treat %s as the loopback PCR bypass",
    async (apiUrl) => {
      const keyExchange = mock(async () => ({
        encrypted_session_key: "must-not-be-used",
        session_id: "must-not-be-created"
      }));

      await expect(
        establish(
          dependencies({
            verifyAttestation: async () => attestationDocument(),
            keyExchange
          }),
          apiUrl
        )
      ).rejects.toThrow(/PCR0/i);

      expect(keyExchange).not.toHaveBeenCalled();
      expect(window.sessionStorage.length).toBe(0);
    }
  );
});
