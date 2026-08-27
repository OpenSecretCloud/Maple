import { expect, test } from "bun:test";
import trustedReleaseSnapshotJson from "../../trusted-enclave-releases.generated.json";
import {
  assertTrustedReleaseSnapshotIntegrity,
  getTrustedReleaseSnapshot,
  normalizeApiBaseUrl,
  normalizeApiOrigin,
  resolveAttestationEnvironment,
  validatePcr0Hash,
  validatePcrsAgainstSnapshot,
  type AttestationEnvironment,
  type TrustedEnclaveRelease,
  type TrustedEnclaveReleaseSnapshot
} from "../../pcr";

const PCR0 = "01".repeat(48);
const PCR1 = "02".repeat(48);
const PCR2 = "03".repeat(48);

function hexToBytes(value: string): Uint8Array {
  return new Uint8Array(value.match(/../g)!.map((byte) => Number.parseInt(byte, 16)));
}

function pcrMap(values = { "0": PCR0, "1": PCR1, "2": PCR2 }) {
  return new Map<number, Uint8Array>([
    [0, hexToBytes(values["0"])],
    [1, hexToBytes(values["1"])],
    [2, hexToBytes(values["2"])]
  ]);
}

function release(tag: string, environment: AttestationEnvironment = "prod"): TrustedEnclaveRelease {
  const sourceRef = `refs/tags/${tag}`;
  return {
    manifestSha256: "10".repeat(32),
    bundleSha256: "11".repeat(32),
    signer: {
      oidcIssuer: "https://token.actions.githubusercontent.com",
      identity: `https://github.com/OpenSecretCloud/opensecret/.github/workflows/release-nitro-eif.yml@${sourceRef}`
    },
    transparencyLog: {
      logIndex: "1234",
      logId: "12".repeat(32)
    },
    manifest: {
      schema: "https://opensecret.cloud/attestations/nitro-eif-release/v1",
      environment,
      source: {
        repository: "OpenSecretCloud/opensecret",
        repositoryId: 921901924,
        ownerId: 185423582,
        ref: sourceRef,
        commit: "13".repeat(20)
      },
      release: { tag },
      artifact: {
        name: `opensecret-${tag}-${environment}.eif`,
        mediaType: "application/vnd.aws.nitro.eif",
        sha256: "14".repeat(32),
        size: 123
      },
      measurements: {
        algorithm: "sha384",
        requiredPcrs: [0, 1, 2],
        pcrs: { "0": PCR0, "1": PCR1, "2": PCR2 }
      },
      build: {
        system: "nix",
        flakeLockSha256: "15".repeat(32),
        derivation: `eif-${environment}`,
        workflowRun: "https://github.com/OpenSecretCloud/opensecret/actions/runs/1234/attempts/1"
      }
    }
  };
}

function snapshot(releases: TrustedEnclaveRelease[]): TrustedEnclaveReleaseSnapshot {
  return {
    ...getTrustedReleaseSnapshot(),
    snapshotId: "16".repeat(32),
    releases
  };
}

function sortJson(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(sortJson);
  if (value !== null && typeof value === "object") {
    const object = value as Record<string, unknown>;
    return Object.fromEntries(
      Object.keys(object)
        .sort()
        .map((key) => [key, sortJson(object[key])])
    );
  }
  return value;
}

test("generated trusted-release snapshot ID covers the canonical policy and releases", async () => {
  const { snapshotId, ...snapshotPayload } = trustedReleaseSnapshotJson;
  const canonical = `${JSON.stringify(sortJson(snapshotPayload), null, 2)}\n`;
  const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(canonical));
  const actual = Array.from(new Uint8Array(digest), (byte) =>
    byte.toString(16).padStart(2, "0")
  ).join("");

  expect(actual).toBe(snapshotId);
  await expect(assertTrustedReleaseSnapshotIntegrity()).resolves.toBeUndefined();
});

test("authorizes only a complete PCR0/PCR1/PCR2 tuple in the selected environment", () => {
  const trusted = snapshot([release("v1.0.0")]);
  const valid = validatePcrsAgainstSnapshot(pcrMap(), "prod", trusted);
  expect(valid.isMatch).toBe(true);
  expect(valid.releaseTag).toBe("v1.0.0");
  expect(valid.environment).toBe("prod");
  expect(valid.transparencyLog).toEqual({
    logIndex: "1234",
    logId: "12".repeat(32)
  });

  const changedPcr1 = validatePcrsAgainstSnapshot(
    pcrMap({ "0": PCR0, "1": "04".repeat(48), "2": PCR2 }),
    "prod",
    trusted
  );
  expect(changedPcr1.isMatch).toBe(false);
  expect(validatePcrsAgainstSnapshot(pcrMap(), "dev", trusted).isMatch).toBe(false);
});

test("requires all three 48-byte PCR values", () => {
  const trusted = snapshot([release("v1.0.0")]);
  const missing = pcrMap();
  missing.delete(2);
  expect(validatePcrsAgainstSnapshot(missing, "prod", trusted).isMatch).toBe(false);

  const short = pcrMap();
  short.set(1, new Uint8Array(47));
  expect(validatePcrsAgainstSnapshot(short, "prod", trusted).isMatch).toBe(false);
});

test("identical reproducible tuples across tags select the highest semantic version", () => {
  const trusted = snapshot([release("v1.0.9"), release("v1.10.0"), release("v1.2.0")]);
  const result = validatePcrsAgainstSnapshot(pcrMap(), "prod", trusted);
  expect(result.isMatch).toBe(true);
  expect(result.releaseTag).toBe("v1.10.0");
});

test("PCR0 compatibility helper cannot authorize the empty production snapshot", async () => {
  const result = await validatePcr0Hash(PCR0, { environment: "prod" });
  expect(result.isMatch).toBe(false);
  expect(result.text).toContain("full PCR0/PCR1/PCR2 verification is required");
});

test("binds exact official origins to an environment", () => {
  expect(resolveAttestationEnvironment("https://api.opensecret.cloud")).toBe("prod");
  expect(resolveAttestationEnvironment("https://enclave.trymaple.ai")).toBe("prod");
  expect(resolveAttestationEnvironment("https://enclave.secretgpt.ai")).toBe("dev");
  expect(() => resolveAttestationEnvironment("https://enclave.trymaple.ai", "dev")).toThrow();
  expect(resolveAttestationEnvironment("https://custom.example", "dev")).toBe("dev");
  expect(() => resolveAttestationEnvironment("https://custom.example")).toThrow();
});

test("accepts HTTPS and exact HTTP loopback URLs only", () => {
  expect(normalizeApiOrigin("https://api.opensecret.cloud/v1")).toBe(
    "https://api.opensecret.cloud"
  );
  expect(normalizeApiOrigin("http://localhost:31110")).toBe("http://localhost:31110");
  expect(normalizeApiOrigin("http://127.0.0.1:31110")).toBe("http://127.0.0.1:31110");
  expect(normalizeApiOrigin("http://[::1]:31110")).toBe("http://[::1]:31110");

  for (const unsafeUrl of [
    "http://api.opensecret.cloud",
    "http://0.0.0.0:31110",
    "http://localhost.example.com:31110",
    "https://api.opensecret.cloud?environment=dev",
    "https://api.opensecret.cloud#dev"
  ]) {
    expect(() => normalizeApiOrigin(unsafeUrl)).toThrow();
  }
});

test("normalizes API base paths without collapsing distinct services", () => {
  expect(normalizeApiBaseUrl("https://example.com/prod/")).toBe("https://example.com/prod");
  expect(normalizeApiBaseUrl("https://example.com/dev")).toBe("https://example.com/dev");
  expect(normalizeApiBaseUrl("https://example.com")).toBe("https://example.com");
});
