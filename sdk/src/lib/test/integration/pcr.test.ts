import { describe, expect, mock, test } from "bun:test";
import {
  assertOfficialEmbeddedBootstrapForTesting,
  AttestationTufClient,
  type AttestationTufClientOptions,
  AttestationTrustError
} from "../../attestationTuf";
import {
  normalizeApiBaseUrl,
  normalizeApiOrigin,
  resolveAttestationEnvironment,
  validatePcrsAgainstSnapshot
} from "../../pcr";
import {
  buildTufFixture,
  FIXTURE_NOW,
  hexPcrMap,
  mockFetch,
  PCR0,
  PCR1,
  PCR2
} from "../tufFixtures";

mock.module("../../attestationSigstore", () => ({
  verifyTufAuthorizedSigstoreBundle: async () => ({
    logIndex: "0",
    logId: "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
    observerTimestamp: "2030-01-01T00:00:00.000Z"
  })
}));

function createAttestationTufClientForTesting(
  options: Required<Pick<AttestationTufClientOptions, "fetch" | "now" | "bootstrap">> & {
    storage?: Storage | null;
  }
): AttestationTufClient {
  return new AttestationTufClient(options);
}

function client(fixture: Awaited<ReturnType<typeof buildTufFixture>>) {
  return createAttestationTufClientForTesting({
    fetch: fixture.fetch,
    storage: fixture.storage,
    now: () => FIXTURE_NOW,
    bootstrap: fixture.bootstrap
  });
}

function replaceRoutes(destination: Map<string, unknown>, source: Map<string, unknown>): void {
  destination.clear();
  for (const [url, route] of source) destination.set(url, route);
}

function copyStorage(source: Storage, destination: Storage): void {
  const entries: Array<[string, string]> = [];
  for (let index = 0; index < source.length; index += 1) {
    const key = source.key(index);
    if (key) entries.push([key, source.getItem(key)!]);
  }
  for (const [key, value] of entries) destination.setItem(key, value);
}

function generationCacheKey(storage: Storage, environment = "prod"): string {
  for (let index = 0; index < storage.length; index += 1) {
    const key = storage.key(index);
    if (key?.startsWith(`opensecret:attestation-tuf:v4:${environment}:`)) return key;
  }
  throw new Error(`No ${environment} attestation generation was persisted.`);
}

function observationValues(storage: Storage): Array<Record<string, any>> {
  const observations: Array<Record<string, any>> = [];
  for (let index = 0; index < storage.length; index += 1) {
    const key = storage.key(index);
    if (!key?.startsWith("opensecret:attestation-tuf:v4:repository-observation:")) continue;
    observations.push(JSON.parse(storage.getItem(key)!) as Record<string, any>);
  }
  return observations;
}

function downgradePersistedGenerationToV2(storage: Storage, environment = "prod"): void {
  const key = generationCacheKey(storage, environment);
  const current = JSON.parse(storage.getItem(key)!) as Record<string, unknown>;
  const { repositoryHighWater: _repository, channelHighWater: _channel, ...legacy } = current;
  storage.setItem(key, JSON.stringify({ ...legacy, version: 2 }));
  for (let index = storage.length - 1; index >= 0; index -= 1) {
    const observationKey = storage.key(index);
    if (observationKey?.startsWith("opensecret:attestation-tuf:v4:repository-observation:")) {
      storage.removeItem(observationKey);
    }
  }
}

const roleSeeds = (
  timestamp: readonly number[],
  snapshot: readonly number[] = timestamp,
  targets: readonly number[] = timestamp
) => ({ timestamp, snapshot, targets });

const signingSeeds = (timestamp: number, snapshot = timestamp, targets = timestamp) => ({
  timestamp,
  snapshot,
  targets
});

function storageView(backing: Storage, onGet?: () => void): Storage {
  return {
    get length() {
      return backing.length;
    },
    clear: backing.clear.bind(backing),
    getItem(key) {
      onGet?.();
      return backing.getItem(key);
    },
    key: backing.key.bind(backing),
    removeItem: backing.removeItem.bind(backing),
    setItem: backing.setItem.bind(backing)
  };
}

function storageWithoutCleanup(backing: Storage): Storage {
  return {
    get length() {
      return backing.length;
    },
    clear: backing.clear.bind(backing),
    getItem: backing.getItem.bind(backing),
    key: backing.key.bind(backing),
    removeItem: () => undefined,
    setItem: backing.setItem.bind(backing)
  };
}

function storageKeepingLatestObservation(backing: Storage): Storage {
  const observationPrefix = "opensecret:attestation-tuf:v4:repository-observation:";
  return {
    get length() {
      return backing.length;
    },
    clear: backing.clear.bind(backing),
    getItem: backing.getItem.bind(backing),
    key: backing.key.bind(backing),
    removeItem: backing.removeItem.bind(backing),
    setItem(key, value) {
      if (key.startsWith(observationPrefix)) {
        for (let index = backing.length - 1; index >= 0; index -= 1) {
          const existing = backing.key(index);
          if (existing?.startsWith(observationPrefix) && existing !== key) {
            backing.removeItem(existing);
          }
        }
      }
      backing.setItem(key, value);
    }
  };
}

describe("browser TUF attestation policy", () => {
  test("fails closed before the generated production root is bootstrapped", async () => {
    let fetched = false;
    const tuf = createAttestationTufClientForTesting({
      fetch: (async () => {
        fetched = true;
        throw new Error("must not fetch");
      }) as typeof fetch,
      storage: null,
      now: () => FIXTURE_NOW,
      bootstrap: {
        schema: "https://attestations.trymaple.ai/schemas/unpublished-tuf-root/v1",
        status: "unpublished",
        message: "not published"
      }
    });

    await expect(tuf.refresh("prod")).rejects.toMatchObject({ code: "TUF_BOOTSTRAP_INVALID" });
    expect(fetched).toBe(false);
  });

  test("pins the official embedded bootstrap at root version one", async () => {
    const fixture = await buildTufFixture({ versions: { root: 2 } });

    expect(() => assertOfficialEmbeddedBootstrapForTesting(fixture.rootEnvelopes[2])).toThrow(
      "must remain root version 1"
    );

    const injected = createAttestationTufClientForTesting({
      fetch: fixture.fetch,
      storage: fixture.storage,
      now: () => FIXTURE_NOW,
      bootstrap: fixture.rootEnvelopes[2]
    });
    await expect(injected.refresh("prod")).resolves.toMatchObject({
      metadataVersions: { root: 2 }
    });
  });

  test("does not authorize official policy without persistent browser storage", async () => {
    const fixture = await buildTufFixture();
    let fetched = false;
    const tuf = createAttestationTufClientForTesting({
      fetch: (async (...args: Parameters<typeof fetch>) => {
        fetched = true;
        return fixture.fetch(...args);
      }) as typeof fetch,
      storage: null,
      now: () => FIXTURE_NOW,
      bootstrap: fixture.bootstrap
    });

    await expect(tuf.refresh("prod")).rejects.toMatchObject({ code: "TRUST_CACHE_INVALID" });
    expect(fetched).toBe(false);
  });

  test("authenticates metadata and targets from only the fixed Maple origin", async () => {
    const fixture = await buildTufFixture();
    const policy = await client(fixture).refresh("prod");

    expect(policy.environment).toBe("prod");
    expect(policy.releases).toHaveLength(1);
    expect(policy.releases[0].sigstore).toMatchObject({
      transparencyLog: { logIndex: "0" },
      observerTimestamp: "2030-01-01T00:00:00.000Z"
    });
    expect(
      fixture.requests.every(({ url }) => url.startsWith("https://attestations.trymaple.ai/tuf/"))
    ).toBe(true);
    expect(
      fixture.requests.some(({ url }) => /github|fulcio|rekor/i.test(new URL(url).hostname))
    ).toBe(false);
    for (const request of fixture.requests) {
      expect(request.init).toMatchObject({
        method: "GET",
        credentials: "omit",
        redirect: "error",
        cache: "no-store",
        referrerPolicy: "no-referrer"
      });
      expect(request.init?.signal).toBeInstanceOf(AbortSignal);
    }

    const persisted = JSON.parse(fixture.storage.getItem(generationCacheKey(fixture.storage))!) as {
      targetBytes: Record<string, string>;
    };
    expect(Object.keys(persisted.targetBytes).sort()).toEqual([
      "channels/prod.json",
      "releases/1.0.0/prod/manifest.json",
      "releases/1.0.0/prod/manifest.sigstore.json",
      "sigstore/trusted_root.json"
    ]);
    expect(persisted.targetBytes["sigstore/trusted_root.json"]).toBeString();
    expect(persisted.targetBytes["releases/1.0.0/prod/manifest.sigstore.json"]).toBeString();
  });

  test("authorizes only one complete PCR tuple and never mixes active releases", async () => {
    const fixture = await buildTufFixture({
      extraReleasePcrs: { "0": PCR0, "1": "04".repeat(48), "2": "05".repeat(48) }
    });
    const policy = await client(fixture).refresh("prod");

    expect(validatePcrsAgainstSnapshot(hexPcrMap(), "prod", policy).isMatch).toBe(true);
    expect(
      validatePcrsAgainstSnapshot(
        hexPcrMap({ "0": PCR0, "1": PCR1, "2": "05".repeat(48) }),
        "prod",
        policy
      ).isMatch
    ).toBe(false);
    expect(validatePcrsAgainstSnapshot(hexPcrMap(), "dev", policy).isMatch).toBe(false);

    const missing = hexPcrMap();
    missing.delete(2);
    expect(validatePcrsAgainstSnapshot(missing, "prod", policy).isMatch).toBe(false);
  });

  test("treats an authenticated empty channel as unreleased and fails closed", async () => {
    const fixture = await buildTufFixture();
    const tuf = client(fixture);
    await tuf.refresh("prod");
    const cacheKey = fixture.storage.key(0)!;
    const before = fixture.storage.getItem(cacheKey);

    const revoked = await buildTufFixture({
      emptyActive: true,
      sequence: 2,
      versions: { timestamp: 2, snapshot: 2, targets: 2 }
    });
    replaceRoutes(fixture.routes as Map<string, unknown>, revoked.routes as Map<string, unknown>);
    await expect(tuf.refresh("prod")).rejects.toMatchObject({
      code: "POLICY_RELEASE_NOT_ACTIVE"
    });
    expect(fixture.storage.getItem(cacheKey)).not.toBe(before);

    fixture.routes.set(revoked.urls.timestamp, { status: 503 });
    await expect(tuf.refresh("prod")).rejects.toMatchObject({
      code: "POLICY_RELEASE_NOT_ACTIVE"
    });
  });

  test("rejects more than two active release manifests", async () => {
    const fixture = await buildTufFixture({
      extraReleasePcrs: { "0": "04".repeat(48), "1": "05".repeat(48), "2": "06".repeat(48) },
      thirdReleasePcrs: { "0": "07".repeat(48), "1": "08".repeat(48), "2": "09".repeat(48) }
    });
    await expect(client(fixture).refresh("prod")).rejects.toMatchObject({
      code: "TUF_METADATA_INVALID"
    });
  });

  test.each([
    ["root", { root: "2029-12-31T23:59:59.000Z" }],
    ["timestamp", { timestamp: "2030-01-01T00:00:00.000Z" }],
    ["snapshot", { snapshot: "2029-12-31T23:59:59.000Z" }],
    ["targets", { targets: "2029-12-31T23:59:59.000Z" }]
  ])("rejects expired %s metadata", async (_role, expires) => {
    const fixture = await buildTufFixture({ expires });
    await expect(client(fixture).refresh("prod")).rejects.toMatchObject({ code: "TUF_EXPIRED" });
  });

  test("rejects timestamps whose validity exceeds the 48-hour client window", async () => {
    const fixture = await buildTufFixture({
      expires: { timestamp: "2030-01-03T00:00:00.001Z" }
    });
    await expect(client(fixture).refresh("prod")).rejects.toMatchObject({ code: "TUF_EXPIRED" });
  });

  test("rechecks metadata expiry after all downloads complete", async () => {
    const fixture = await buildTufFixture();
    let clockReads = 0;
    const tuf = createAttestationTufClientForTesting({
      fetch: fixture.fetch,
      storage: fixture.storage,
      now: () => (clockReads++ === 0 ? FIXTURE_NOW : new Date("2030-01-03T00:00:00.000Z")),
      bootstrap: fixture.bootstrap
    });
    await expect(tuf.refresh("prod")).rejects.toMatchObject({ code: "TUF_EXPIRED" });
    expect(
      Array.from({ length: fixture.storage.length }, (_, index) => fixture.storage.key(index)).some(
        (key) => key?.startsWith("opensecret:attestation-tuf:v4:prod:")
      )
    ).toBe(false);
  });

  test.each([
    ["unsafe source path", { sourcePath: "../backend" }],
    ["unsafe artifact name", { artifactName: "../backend.eif" }],
    ["query-bearing build URI", { runUri: "https://ci.example/runs/1?token=secret" }]
  ])("rejects a manifest with %s", async (_description, fixtureOptions) => {
    const fixture = await buildTufFixture(fixtureOptions);
    await expect(client(fixture).refresh("prod")).rejects.toBeInstanceOf(AttestationTrustError);
  });

  test("does not turn source repository provenance into client authorization", async () => {
    const fixture = await buildTufFixture({ sourceUri: "https://code.example/opensecret" });
    await expect(client(fixture).refresh("prod")).resolves.toMatchObject({
      releases: [{ manifest: { source: { uri: "https://code.example/opensecret" } } }]
    });
  });

  test("rejects invalid signatures and non-sequential root rotation", async () => {
    const badTimestamp = await buildTufFixture({ tamperTimestampSignature: true });
    await expect(client(badTimestamp).refresh("prod")).rejects.toMatchObject({
      code: "TUF_SIGNATURE_INVALID"
    });

    const skippedRoot = await buildTufFixture({
      versions: { root: 2 },
      rootEnvelopeVersionOverride: 3
    });
    await expect(client(skippedRoot).refresh("prod")).rejects.toMatchObject({
      code: "TUF_ROOT_CHAIN_INVALID"
    });
  });

  test("rejects an invalid intermediate root before requesting a clean final root", async () => {
    const fixture = await buildTufFixture({
      versions: { root: 3 },
      tamperRootSignatureVersion: 2
    });
    await expect(client(fixture).refresh("prod")).rejects.toMatchObject({
      code: "TUF_SIGNATURE_INVALID"
    });
    expect(fixture.requests.some(({ url }) => url.endsWith("/metadata/3.root.json"))).toBe(false);
  });

  test("remembers the bootstrap authority across a first-refresh multi-root chain", async () => {
    const A = 8;
    const B = 9;
    const C = 10;
    const fixture = await buildTufFixture({
      versions: { root: 4 },
      metadataRoleKeySeedsByRootVersion: {
        1: roleSeeds([A]),
        2: roleSeeds([B]),
        3: roleSeeds([C]),
        4: roleSeeds([A])
      },
      metadataSigningSeeds: signingSeeds(A)
    });

    await expect(client(fixture).refresh("prod")).rejects.toMatchObject({ code: "TUF_ROLLBACK" });
    expect(fixture.requests.some(({ url }) => url.endsWith("/metadata/timestamp.json"))).toBe(
      false
    );
  });

  test("rejects first-refresh reuse of retired authority material by another role", async () => {
    const A = 8;
    const B = 9;
    const C = 10;
    const D = 11;
    const fixture = await buildTufFixture({
      versions: { root: 3 },
      metadataRoleKeySeedsByRootVersion: {
        1: roleSeeds([A], [B], [C]),
        2: roleSeeds([D], [B], [C]),
        3: roleSeeds([B], [B], [C])
      },
      metadataSigningSeeds: signingSeeds(B, B, C)
    });

    await expect(client(fixture).refresh("prod")).rejects.toMatchObject({ code: "TUF_ROLLBACK" });
    expect(fixture.requests.some(({ url }) => url.endsWith("/metadata/timestamp.json"))).toBe(
      false
    );
  });

  test("rejects independently authenticated roots that fork at the same version", async () => {
    const forkA = await buildTufFixture({
      versions: { root: 2 },
      metadataRoleKeySeedsByRootVersion: { 2: roleSeeds([8]) },
      metadataSigningSeeds: signingSeeds(8)
    });
    const forkB = await buildTufFixture({
      versions: { root: 2 },
      metadataRoleKeySeedsByRootVersion: { 2: roleSeeds([9]) },
      metadataSigningSeeds: signingSeeds(9)
    });
    forkA.routes.set(forkA.urls.timestamp, { status: 503 });
    forkB.routes.set(forkB.urls.timestamp, { status: 503 });
    await expect(client(forkA).refresh("prod")).rejects.toBeInstanceOf(Error);
    await expect(client(forkB).refresh("prod")).rejects.toBeInstanceOf(Error);
    copyStorage(forkB.storage, forkA.storage);
    forkA.requests.length = 0;

    await expect(client(forkA).refresh("prod")).rejects.toMatchObject({ code: "TUF_ROLLBACK" });
    expect(forkA.requests).toHaveLength(0);
  });

  test("rejects a higher root chain that does not contain the accepted lower-root anchor", async () => {
    const lowerFork = await buildTufFixture({
      versions: { root: 2 },
      metadataRoleKeySeedsByRootVersion: { 2: roleSeeds([8]) },
      metadataSigningSeeds: signingSeeds(8)
    });
    const higherFork = await buildTufFixture({
      versions: { root: 3 },
      metadataRoleKeySeedsByRootVersion: {
        2: roleSeeds([9]),
        3: roleSeeds([10])
      },
      metadataSigningSeeds: signingSeeds(10)
    });
    await client(lowerFork).refresh("prod");
    await client(higherFork).refresh("prod");
    copyStorage(higherFork.storage, lowerFork.storage);
    lowerFork.requests.length = 0;

    await expect(client(lowerFork).refresh("prod")).rejects.toMatchObject({
      code: "TUF_ROLLBACK"
    });
    expect(lowerFork.requests).toHaveLength(0);
  });

  test("rejects duplicate aliases for authorized key material before network access", async () => {
    const fixture = await buildTufFixture({ duplicateAuthorizedKeyMaterialAlias: true });
    await expect(client(fixture).refresh("prod")).rejects.toMatchObject({
      code: "TUF_ROOT_CHAIN_INVALID"
    });
    expect(fixture.requests).toHaveLength(0);
  });

  test("rejects offline root material reused by an online role before network access", async () => {
    const fixture = await buildTufFixture({ rootSigningSeed: 7 });

    await expect(client(fixture).refresh("prod")).rejects.toMatchObject({
      code: "TUF_ROOT_CHAIN_INVALID"
    });
    expect(fixture.requests).toHaveLength(0);
  });

  test("rejects moving retired offline root material into an online role", async () => {
    const fixture = await buildTufFixture({
      versions: { root: 3 },
      rootRoleKeySeedsByRootVersion: { 1: [6], 2: [8], 3: [10] },
      metadataRoleKeySeedsByRootVersion: {
        1: roleSeeds([7]),
        2: roleSeeds([9]),
        3: roleSeeds([6], [9], [9])
      },
      metadataSigningSeeds: { timestamp: 6, snapshot: 9, targets: 9 }
    });

    await expect(client(fixture).refresh("prod")).rejects.toMatchObject({
      code: "TUF_ROLLBACK"
    });
    expect(fixture.requests.some(({ url }) => url.endsWith("/metadata/timestamp.json"))).toBe(
      false
    );
    expect(
      observationValues(fixture.storage).some(
        (value) => value.repositoryHighWater?.root?.version === 2
      )
    ).toBe(true);

    fixture.requests.length = 0;
    await expect(client(fixture).refresh("prod")).rejects.toMatchObject({
      code: "TUF_ROLLBACK"
    });
    expect(fixture.requests.some(({ url }) => url.endsWith("/metadata/3.root.json"))).toBe(true);
    expect(fixture.requests.some(({ url }) => url.endsWith("/metadata/timestamp.json"))).toBe(
      false
    );
  });

  test("rejects promoting previously-online material into the offline root role", async () => {
    const fixture = await buildTufFixture({
      versions: { root: 3 },
      rootRoleKeySeedsByRootVersion: { 1: [6], 2: [8], 3: [7] },
      metadataRoleKeySeedsByRootVersion: {
        1: roleSeeds([7]),
        2: roleSeeds([9]),
        3: roleSeeds([10])
      },
      metadataSigningSeeds: signingSeeds(10)
    });

    await expect(client(fixture).refresh("prod")).rejects.toMatchObject({
      code: "TUF_ROLLBACK"
    });
    expect(fixture.requests.some(({ url }) => url.endsWith("/metadata/timestamp.json"))).toBe(
      false
    );
  });

  test("accepts fresh offline and online authority rotation without crossing custody classes", async () => {
    const fixture = await buildTufFixture({
      versions: { root: 3 },
      rootRoleKeySeedsByRootVersion: { 1: [6], 2: [8], 3: [10] },
      metadataRoleKeySeedsByRootVersion: {
        1: roleSeeds([7]),
        2: roleSeeds([9]),
        3: roleSeeds([11])
      },
      metadataSigningSeeds: signingSeeds(11)
    });

    await expect(client(fixture).refresh("prod")).resolves.toMatchObject({
      metadataVersions: { root: 3 }
    });
  });

  test.each([
    ["snapshot length", { timestampSnapshotLengthDelta: 1 }],
    ["snapshot hash", { timestampSnapshotHash: "ff".repeat(32) }],
    ["targets length", { snapshotTargetsLengthDelta: 1 }],
    ["targets hash", { snapshotTargetsHash: "ff".repeat(32) }],
    ["manifest channel hash", { channelManifestHash: "ff".repeat(32) }]
  ])("rejects an authenticated %s mismatch", async (_name, fixtureOptions) => {
    const fixture = await buildTufFixture(fixtureOptions);
    await expect(client(fixture).refresh("prod")).rejects.toBeInstanceOf(AttestationTrustError);
  });

  test("rejects a target body that no longer matches its authenticated digest", async () => {
    const fixture = await buildTufFixture();
    fixture.routes.set(fixture.urls.manifest, { bytes: new TextEncoder().encode("{}") });
    await expect(client(fixture).refresh("prod")).rejects.toMatchObject({
      code: "TUF_TARGET_INTEGRITY"
    });
  });

  test("rejects redirects and both declared and streamed response overflows", async () => {
    const redirected = await buildTufFixture();
    redirected.routes.set(redirected.urls.nextRoot, {
      status: 404,
      redirected: true,
      finalUrl: "https://evil.example/2.root.json"
    });
    await expect(client(redirected).refresh("prod")).rejects.toMatchObject({
      code: "TRUST_REDIRECT"
    });

    const missingFinalUrl = await buildTufFixture();
    missingFinalUrl.routes.set(missingFinalUrl.urls.nextRoot, { status: 404, finalUrl: "" });
    await expect(client(missingFinalUrl).refresh("prod")).rejects.toMatchObject({
      code: "TRUST_REDIRECT"
    });

    const declared = await buildTufFixture();
    declared.routes.set(declared.urls.timestamp, {
      bytes: new Uint8Array(),
      headers: { "content-length": "999999" }
    });
    await expect(client(declared).refresh("prod")).rejects.toMatchObject({
      code: "TRUST_SIZE_LIMIT"
    });

    const streamed = await buildTufFixture();
    streamed.routes.set(streamed.urls.manifest, {
      stream: new ReadableStream({
        start(controller) {
          controller.enqueue(new Uint8Array(128 * 1024));
          controller.enqueue(new Uint8Array([1]));
          controller.close();
        }
      })
    });
    await expect(client(streamed).refresh("prod")).rejects.toMatchObject({
      code: "TRUST_SIZE_LIMIT"
    });
  });

  test("persists rollback high-water marks and refuses lower metadata", async () => {
    const current = await buildTufFixture({
      versions: { timestamp: 2, snapshot: 2, targets: 2 },
      sequence: 2
    });
    const tuf = client(current);
    await tuf.refresh("prod");

    const rolledBack = await buildTufFixture();
    replaceRoutes(
      current.routes as Map<string, unknown>,
      rolledBack.routes as Map<string, unknown>
    );
    await expect(tuf.refresh("prod")).rejects.toMatchObject({ code: "TUF_ROLLBACK" });
  });

  test("preserves rollback state across reload without requiring Web Locks", async () => {
    globalThis.localStorage.clear();
    try {
      const current = await buildTufFixture({
        versions: { timestamp: 2, snapshot: 2, targets: 2 },
        sequence: 2
      });
      const firstPage = createAttestationTufClientForTesting({
        fetch: current.fetch,
        now: () => FIXTURE_NOW,
        bootstrap: current.bootstrap
      });
      await firstPage.refresh("prod");
      expect(generationCacheKey(globalThis.localStorage)).toContain(
        "opensecret:attestation-tuf:v4:prod:"
      );

      const rolledBack = await buildTufFixture();
      const reloadedPage = createAttestationTufClientForTesting({
        fetch: rolledBack.fetch,
        now: () => FIXTURE_NOW,
        bootstrap: current.bootstrap
      });
      await expect(reloadedPage.refresh("prod")).rejects.toMatchObject({
        code: "TUF_ROLLBACK"
      });
    } finally {
      globalThis.localStorage.clear();
    }
  });

  test("recovers max-version floors after a disjoint replacement of every online role", async () => {
    const A = 8;
    const B = 9;
    const high = Number.MAX_SAFE_INTEGER;
    const current = await buildTufFixture({
      versions: { timestamp: high, snapshot: high, targets: high },
      sequence: high,
      metadataRoleKeySeedsByRootVersion: { 1: roleSeeds([A]) },
      metadataSigningSeeds: signingSeeds(A)
    });
    await client(current).refresh("prod");

    const recovered = await buildTufFixture({
      versions: { root: 2, timestamp: 1, snapshot: 1, targets: 1 },
      sequence: 1,
      metadataRoleKeySeedsByRootVersion: {
        1: roleSeeds([A]),
        2: roleSeeds([B])
      },
      metadataSigningSeeds: signingSeeds(B)
    });
    replaceRoutes(current.routes as Map<string, unknown>, recovered.routes as Map<string, unknown>);

    await expect(client(current).refresh("prod")).resolves.toMatchObject({ sequence: 1 });
  });

  test("persists a disjoint root-only recovery before a failed timestamp fetch", async () => {
    const A = 8;
    const B = 9;
    const high = Number.MAX_SAFE_INTEGER;
    const current = await buildTufFixture({
      versions: { timestamp: high, snapshot: high, targets: high },
      sequence: high,
      metadataRoleKeySeedsByRootVersion: { 1: roleSeeds([A]) },
      metadataSigningSeeds: signingSeeds(A)
    });
    await client(current).refresh("prod");

    const recovered = await buildTufFixture({
      versions: { root: 2 },
      metadataRoleKeySeedsByRootVersion: {
        1: roleSeeds([A]),
        2: roleSeeds([B])
      },
      metadataSigningSeeds: signingSeeds(B)
    });
    const timestampRoute = recovered.routes.get(recovered.urls.timestamp)!;
    recovered.routes.set(recovered.urls.timestamp, { status: 503 });
    replaceRoutes(current.routes as Map<string, unknown>, recovered.routes as Map<string, unknown>);
    await expect(client(current).refresh("prod")).rejects.toMatchObject({ code: "TUF_ROLLBACK" });

    current.routes.set(recovered.urls.timestamp, timestampRoute);
    current.requests.length = 0;
    await expect(client(current).refresh("prod")).resolves.toMatchObject({ sequence: 1 });
    expect(current.requests.some(({ url }) => url.endsWith("/metadata/2.root.json"))).toBe(false);
    expect(current.requests.some(({ url }) => url.endsWith("/metadata/3.root.json"))).toBe(true);
  });

  test("widens floors across overlap roots and does not recover through a disjoint endpoint", async () => {
    const A = 8;
    const B = 9;
    const C = 10;
    const high = Number.MAX_SAFE_INTEGER;
    const current = await buildTufFixture({
      versions: { timestamp: high, snapshot: high, targets: high },
      sequence: high,
      metadataRoleKeySeedsByRootVersion: { 1: roleSeeds([A]) },
      metadataSigningSeeds: signingSeeds(A)
    });
    await client(current).refresh("prod");

    const replay = await buildTufFixture({
      versions: { root: 3 },
      metadataRoleKeySeedsByRootVersion: {
        1: roleSeeds([A]),
        2: roleSeeds([A, B]),
        3: roleSeeds([B, C])
      },
      metadataSigningSeeds: signingSeeds(C)
    });
    replaceRoutes(current.routes as Map<string, unknown>, replay.routes as Map<string, unknown>);

    await expect(client(current).refresh("prod")).rejects.toMatchObject({ code: "TUF_ROLLBACK" });
    const rootThree = observationValues(current.storage).find(
      (value) => value.repositoryHighWater?.root?.version === 3
    );
    expect(rootThree?.repositoryHighWater.timestamp.authority.keyFingerprints).toHaveLength(3);
  });

  test("retains overlap provenance across reload before a single-key cutover", async () => {
    const A = 8;
    const B = 9;
    const initial = await buildTufFixture({
      versions: { timestamp: 10, snapshot: 10, targets: 10 },
      sequence: 10,
      metadataRoleKeySeedsByRootVersion: { 1: roleSeeds([A]) },
      metadataSigningSeeds: signingSeeds(A)
    });
    await client(initial).refresh("prod");

    const overlap = await buildTufFixture({
      versions: { root: 2, timestamp: 11, snapshot: 11, targets: 11 },
      sequence: 11,
      metadataRoleKeySeedsByRootVersion: {
        1: roleSeeds([A]),
        2: roleSeeds([A, B])
      },
      metadataSigningSeeds: signingSeeds(B)
    });
    replaceRoutes(initial.routes as Map<string, unknown>, overlap.routes as Map<string, unknown>);
    await client(initial).refresh("prod");

    const replay = await buildTufFixture({
      versions: { root: 3, timestamp: 1, snapshot: 1, targets: 1 },
      sequence: 1,
      metadataRoleKeySeedsByRootVersion: {
        1: roleSeeds([A]),
        2: roleSeeds([A, B]),
        3: roleSeeds([B])
      },
      metadataSigningSeeds: signingSeeds(B)
    });
    replaceRoutes(initial.routes as Map<string, unknown>, replay.routes as Map<string, unknown>);

    await expect(client(initial).refresh("prod")).rejects.toMatchObject({ code: "TUF_ROLLBACK" });
  });

  test("never reauthorizes retired online-role key material", async () => {
    const A = 8;
    const B = 9;
    const initial = await buildTufFixture({
      metadataRoleKeySeedsByRootVersion: { 1: roleSeeds([A]) },
      metadataSigningSeeds: signingSeeds(A)
    });
    await client(initial).refresh("prod");

    const replacement = await buildTufFixture({
      versions: { root: 2 },
      metadataRoleKeySeedsByRootVersion: {
        1: roleSeeds([A]),
        2: roleSeeds([B])
      },
      metadataSigningSeeds: signingSeeds(B)
    });
    replaceRoutes(
      initial.routes as Map<string, unknown>,
      replacement.routes as Map<string, unknown>
    );
    await client(initial).refresh("prod");

    const reintroduced = await buildTufFixture({
      versions: { root: 3 },
      metadataRoleKeySeedsByRootVersion: {
        1: roleSeeds([A]),
        2: roleSeeds([B]),
        3: roleSeeds([A])
      },
      metadataSigningSeeds: signingSeeds(A)
    });
    replaceRoutes(
      initial.routes as Map<string, unknown>,
      reintroduced.routes as Map<string, unknown>
    );
    initial.requests.length = 0;

    await expect(client(initial).refresh("prod")).rejects.toMatchObject({ code: "TUF_ROLLBACK" });
    expect(initial.requests.some(({ url }) => url.endsWith("/metadata/timestamp.json"))).toBe(
      false
    );
  });

  test("rejects cross-role reuse of previously authorized key material", async () => {
    const A = 8;
    const B = 9;
    const C = 10;
    const D = 11;
    const initial = await buildTufFixture({
      metadataRoleKeySeedsByRootVersion: { 1: roleSeeds([A], [B], [C]) },
      metadataSigningSeeds: signingSeeds(A, B, C)
    });
    await client(initial).refresh("prod");

    const replacement = await buildTufFixture({
      versions: { root: 2 },
      metadataRoleKeySeedsByRootVersion: {
        1: roleSeeds([A], [B], [C]),
        2: roleSeeds([D], [B], [C])
      },
      metadataSigningSeeds: signingSeeds(D, B, C)
    });
    replaceRoutes(
      initial.routes as Map<string, unknown>,
      replacement.routes as Map<string, unknown>
    );
    await client(initial).refresh("prod");

    const reused = await buildTufFixture({
      versions: { root: 3 },
      metadataRoleKeySeedsByRootVersion: {
        1: roleSeeds([A], [B], [C]),
        2: roleSeeds([D], [B], [C]),
        3: roleSeeds([B], [B], [C])
      },
      metadataSigningSeeds: signingSeeds(B, B, C)
    });
    replaceRoutes(initial.routes as Map<string, unknown>, reused.routes as Map<string, unknown>);

    await expect(client(initial).refresh("prod")).rejects.toMatchObject({ code: "TUF_ROLLBACK" });
  });

  test("uses the candidate threshold when deciding whether authority replacement is disjoint", async () => {
    const A = 8;
    const B = 9;
    const C = 10;
    const high = Number.MAX_SAFE_INTEGER;
    const initial = await buildTufFixture({
      versions: { timestamp: high, snapshot: high, targets: high },
      sequence: high,
      metadataRoleKeySeedsByRootVersion: { 1: roleSeeds([A, B]) },
      metadataRoleThresholdsByRootVersion: {
        1: { timestamp: 2, snapshot: 2, targets: 2 }
      },
      metadataSigningSeeds: {
        timestamp: [A, B],
        snapshot: [A, B],
        targets: [A, B]
      }
    });
    await client(initial).refresh("prod");

    const replacement = await buildTufFixture({
      versions: { root: 2 },
      metadataRoleKeySeedsByRootVersion: {
        1: roleSeeds([A, B]),
        2: roleSeeds([B, C])
      },
      metadataRoleThresholdsByRootVersion: {
        1: { timestamp: 2, snapshot: 2, targets: 2 },
        2: { timestamp: 2, snapshot: 2, targets: 2 }
      },
      metadataSigningSeeds: {
        timestamp: [B, C],
        snapshot: [B, C],
        targets: [B, C]
      }
    });
    replaceRoutes(
      initial.routes as Map<string, unknown>,
      replacement.routes as Map<string, unknown>
    );

    await expect(client(initial).refresh("prod")).resolves.toMatchObject({ sequence: 1 });

    const retained = await buildTufFixture({
      versions: { timestamp: high, snapshot: high, targets: high },
      sequence: high,
      metadataRoleKeySeedsByRootVersion: { 1: roleSeeds([A, B, C]) },
      metadataSigningSeeds: signingSeeds(A)
    });
    await client(retained).refresh("prod");
    const D = 11;
    const overlappingThreshold = await buildTufFixture({
      versions: { root: 2 },
      metadataRoleKeySeedsByRootVersion: {
        1: roleSeeds([A, B, C]),
        2: roleSeeds([B, C, D])
      },
      metadataRoleThresholdsByRootVersion: {
        2: { timestamp: 2, snapshot: 2, targets: 2 }
      },
      metadataSigningSeeds: {
        timestamp: [B, C],
        snapshot: [B, C],
        targets: [B, C]
      }
    });
    replaceRoutes(
      retained.routes as Map<string, unknown>,
      overlappingThreshold.routes as Map<string, unknown>
    );
    await expect(client(retained).refresh("prod")).rejects.toMatchObject({
      code: "TUF_ROLLBACK"
    });
  });

  test("rejects persisted provenance that omits a currently authorized key", async () => {
    const A = 8;
    const B = 9;
    const fixture = await buildTufFixture({
      metadataRoleKeySeedsByRootVersion: { 1: roleSeeds([A, B]) },
      metadataSigningSeeds: signingSeeds(A)
    });
    await client(fixture).refresh("prod");
    const key = generationCacheKey(fixture.storage);
    const raw = JSON.parse(fixture.storage.getItem(key)!) as Record<string, any>;
    raw.repositoryHighWater.timestamp.authority.keyFingerprints.pop();
    fixture.storage.setItem(key, JSON.stringify(raw));
    fixture.requests.length = 0;

    await expect(client(fixture).refresh("prod")).rejects.toMatchObject({
      code: "TRUST_CACHE_INVALID"
    });
    expect(fixture.requests).toHaveLength(0);
  });

  test("a targets-only authority replacement resets the channel sequence", async () => {
    const A = 8;
    const B = 9;
    const initial = await buildTufFixture({
      versions: { timestamp: 10, snapshot: 10, targets: 10 },
      sequence: 10,
      metadataRoleKeySeedsByRootVersion: { 1: roleSeeds([A]) },
      metadataSigningSeeds: signingSeeds(A)
    });
    await client(initial).refresh("prod");

    const replacement = await buildTufFixture({
      versions: { root: 2, timestamp: 11, snapshot: 11, targets: 1 },
      sequence: 1,
      metadataRoleKeySeedsByRootVersion: {
        1: roleSeeds([A]),
        2: roleSeeds([A], [A], [B])
      },
      metadataSigningSeeds: signingSeeds(A, A, B)
    });
    replaceRoutes(
      initial.routes as Map<string, unknown>,
      replacement.routes as Map<string, unknown>
    );

    await expect(client(initial).refresh("prod")).resolves.toMatchObject({ sequence: 1 });
  });

  test("resets composite pointer floors when either side of their authority is replaced", async () => {
    const A = 8;
    const B = 9;
    const initialOptions = {
      versions: { timestamp: 10, snapshot: 10, targets: 10 },
      sequence: 10,
      metadataRoleKeySeedsByRootVersion: { 1: roleSeeds([A]) },
      metadataSigningSeeds: signingSeeds(A)
    };

    const parentChanged = await buildTufFixture(initialOptions);
    await client(parentChanged).refresh("prod");
    const timestampRotation = await buildTufFixture({
      versions: { root: 2 },
      metadataRoleKeySeedsByRootVersion: {
        1: roleSeeds([A]),
        2: roleSeeds([B], [A], [A])
      },
      metadataSigningSeeds: { timestamp: B, snapshot: A, targets: A }
    });
    timestampRotation.routes.set(timestampRotation.urls.timestamp, { status: 503 });
    replaceRoutes(
      parentChanged.routes as Map<string, unknown>,
      timestampRotation.routes as Map<string, unknown>
    );
    await expect(client(parentChanged).refresh("prod")).rejects.toMatchObject({
      code: "TUF_ROLLBACK"
    });
    const parentState = observationValues(parentChanged.storage).find(
      (value) => value.repositoryHighWater?.root?.version === 2
    )?.repositoryHighWater;
    expect(parentState?.timestamp).toBeUndefined();
    expect(parentState?.snapshotDescriptor).toBeUndefined();
    expect(parentState?.snapshot).toBeDefined();

    const childChanged = await buildTufFixture(initialOptions);
    await client(childChanged).refresh("prod");
    const snapshotRotation = await buildTufFixture({
      versions: { root: 2 },
      metadataRoleKeySeedsByRootVersion: {
        1: roleSeeds([A]),
        2: roleSeeds([A], [B], [A])
      },
      metadataSigningSeeds: { timestamp: A, snapshot: B, targets: A }
    });
    snapshotRotation.routes.set(snapshotRotation.urls.timestamp, { status: 503 });
    replaceRoutes(
      childChanged.routes as Map<string, unknown>,
      snapshotRotation.routes as Map<string, unknown>
    );
    await expect(client(childChanged).refresh("prod")).rejects.toMatchObject({
      code: "TUF_ROLLBACK"
    });
    const childState = observationValues(childChanged.storage).find(
      (value) => value.repositoryHighWater?.root?.version === 2
    )?.repositoryHighWater;
    expect(childState?.timestamp).toBeDefined();
    expect(childState?.snapshotDescriptor).toBeUndefined();
    expect(childState?.snapshot).toBeUndefined();
    expect(childState?.targetsDescriptor).toBeUndefined();
    expect(childState?.targets).toBeDefined();
  });

  test("rejects a pre-provenance legacy generation cache without network trust fallback", async () => {
    const A = 8;
    const B = 9;
    const current = await buildTufFixture({
      versions: { root: 2, timestamp: 10, snapshot: 10, targets: 10 },
      sequence: 10,
      metadataRoleKeySeedsByRootVersion: {
        1: roleSeeds([A]),
        2: roleSeeds([A, B])
      },
      metadataSigningSeeds: signingSeeds(A)
    });
    await client(current).refresh("prod");
    downgradePersistedGenerationToV2(current.storage);
    current.requests.length = 0;

    await expect(client(current).refresh("prod")).rejects.toMatchObject({
      code: "TRUST_CACHE_INVALID"
    });
    expect(current.requests).toHaveLength(0);
  });

  test("rejects the pre-root-history v3 cache namespace before network access", async () => {
    const fixture = await buildTufFixture();
    fixture.storage.setItem("opensecret:attestation-tuf:v3:prod:legacy", "{}");

    await expect(client(fixture).refresh("prod")).rejects.toMatchObject({
      code: "TRUST_CACHE_INVALID"
    });
    expect(fixture.requests).toHaveLength(0);
  });

  test("treats signature-array variation as the same signed metadata", async () => {
    const fixture = await buildTufFixture();
    await client(fixture).refresh("prod");
    const repeated = await buildTufFixture();
    const route = repeated.routes.get(repeated.urls.timestamp)! as { bytes: Uint8Array };
    const envelope = JSON.parse(new TextDecoder().decode(route.bytes)) as {
      signatures: Array<{ keyid: string; sig: string }>;
    };
    envelope.signatures.push({ ...envelope.signatures[0] });
    repeated.routes.set(repeated.urls.timestamp, {
      bytes: new TextEncoder().encode(JSON.stringify(envelope))
    });
    replaceRoutes(fixture.routes as Map<string, unknown>, repeated.routes as Map<string, unknown>);
    await expect(client(fixture).refresh("prod")).resolves.toMatchObject({ sequence: 1 });
  });

  test("rejects embedded-root replacement after sequential remote root rotation", async () => {
    const fixture = await buildTufFixture({ versions: { root: 2 } });
    await client(fixture).refresh("prod");
    fixture.requests.length = 0;

    const upgradedSdk = createAttestationTufClientForTesting({
      fetch: fixture.fetch,
      storage: fixture.storage,
      now: () => FIXTURE_NOW,
      bootstrap: fixture.rootEnvelopes[2]
    });
    await expect(upgradedSdk.refresh("prod")).rejects.toMatchObject({
      code: "TUF_ROOT_CHAIN_INVALID"
    });
    expect(fixture.requests).toHaveLength(0);
  });

  test("rejects even an immediate embedded-root successor for persisted state", async () => {
    const cached = await buildTufFixture();
    await client(cached).refresh("prod");
    const upgraded = await buildTufFixture({ versions: { root: 2 } });
    const upgradedSdk = createAttestationTufClientForTesting({
      fetch: upgraded.fetch,
      storage: cached.storage,
      now: () => FIXTURE_NOW,
      bootstrap: upgraded.rootEnvelopes[2]
    });

    await expect(upgradedSdk.refresh("prod")).rejects.toMatchObject({
      code: "TUF_ROOT_CHAIN_INVALID"
    });
    expect(upgraded.requests).toHaveLength(0);
  });

  test("rejects a root-one cache that skips the embedded root-two trust epoch", async () => {
    const cached = await buildTufFixture();
    await client(cached).refresh("prod");
    const upgraded = await buildTufFixture({ versions: { root: 3 } });
    const upgradedSdk = createAttestationTufClientForTesting({
      fetch: upgraded.fetch,
      storage: cached.storage,
      now: () => FIXTURE_NOW,
      bootstrap: upgraded.rootEnvelopes[3]
    });

    await expect(upgradedSdk.refresh("prod")).rejects.toMatchObject({
      code: "TUF_ROOT_CHAIN_INVALID"
    });
    expect(upgraded.requests).toHaveLength(0);
  });

  test("cannot forget an unseen retired online key across a skipped embedded root", async () => {
    const A = 8;
    const X = 9;
    const cached = await buildTufFixture({
      metadataRoleKeySeedsByRootVersion: { 1: roleSeeds([A]) },
      metadataSigningSeeds: signingSeeds(A)
    });
    await client(cached).refresh("prod");

    const skipped = await buildTufFixture({
      versions: { root: 4 },
      metadataRoleKeySeedsByRootVersion: {
        1: roleSeeds([A]),
        2: roleSeeds([X]),
        3: roleSeeds([A]),
        4: roleSeeds([X])
      },
      metadataSigningSeeds: signingSeeds(X)
    });
    const upgradedClient = createAttestationTufClientForTesting({
      fetch: skipped.fetch,
      storage: cached.storage,
      now: () => FIXTURE_NOW,
      bootstrap: skipped.rootEnvelopes[3]
    });

    await expect(upgradedClient.refresh("prod")).rejects.toMatchObject({
      code: "TUF_ROOT_CHAIN_INVALID"
    });
    expect(skipped.requests).toHaveLength(0);
  });

  test("rejects embedded-root replacement before evaluating cached metadata floors", async () => {
    const current = await buildTufFixture({
      versions: { timestamp: 3, snapshot: 3, targets: 3 },
      sequence: 3
    });
    await client(current).refresh("prod");

    const replay = await buildTufFixture({ versions: { root: 2 }, sequence: 1 });
    const upgradedSdk = createAttestationTufClientForTesting({
      fetch: replay.fetch,
      storage: current.storage,
      now: () => FIXTURE_NOW,
      bootstrap: replay.rootEnvelopes[2]
    });

    await expect(upgradedSdk.refresh("prod")).rejects.toMatchObject({
      code: "TUF_ROOT_CHAIN_INVALID"
    });
    expect(replay.requests).toHaveLength(0);
  });

  test("rejects changed-role embedded-root replacement before cached signature checks", async () => {
    const current = await buildTufFixture({
      versions: { timestamp: 3, snapshot: 3, targets: 3 },
      sequence: 3
    });
    await client(current).refresh("prod");

    const incompatible = await buildTufFixture({
      versions: { root: 2 },
      signingSeed: 8
    });
    const upgradedSdk = createAttestationTufClientForTesting({
      fetch: incompatible.fetch,
      storage: current.storage,
      now: () => FIXTURE_NOW,
      bootstrap: incompatible.rootEnvelopes[2]
    });

    await expect(upgradedSdk.refresh("prod")).rejects.toMatchObject({
      code: "TUF_ROOT_CHAIN_INVALID"
    });
    expect(incompatible.requests).toHaveLength(0);
  });

  test("persists each authenticated root before probing the following root", async () => {
    const initial = await buildTufFixture();
    await client(initial).refresh("prod");

    const rotating = await buildTufFixture({ versions: { root: 2 } });
    rotating.routes.set(rotating.urls.nextRoot, { status: 503 });
    replaceRoutes(initial.routes as Map<string, unknown>, rotating.routes as Map<string, unknown>);
    initial.requests.length = 0;

    await expect(client(initial).refresh("prod")).rejects.toMatchObject({ code: "TUF_ROLLBACK" });
    expect(initial.requests.some(({ url }) => url.endsWith("/metadata/2.root.json"))).toBe(true);

    initial.requests.length = 0;
    const reloaded = createAttestationTufClientForTesting({
      fetch: initial.fetch,
      storage: initial.storage,
      now: () => FIXTURE_NOW,
      bootstrap: initial.bootstrap
    });
    await expect(reloaded.refresh("prod")).rejects.toMatchObject({ code: "TUF_ROLLBACK" });
    expect(initial.requests.some(({ url }) => url.endsWith("/metadata/3.root.json"))).toBe(true);
    expect(initial.requests.some(({ url }) => url.endsWith("/metadata/2.root.json"))).toBe(false);
  });

  test("enforces the root-rotation ceiling across refresh restarts", async () => {
    const fixture = await buildTufFixture({ versions: { root: 34 } });
    const backingStorage = fixture.storage;
    fixture.storage = storageKeepingLatestObservation(backingStorage);
    const root34Url = "https://attestations.trymaple.ai/tuf/metadata/34.root.json";

    await expect(client(fixture).refresh("prod")).rejects.toMatchObject({
      code: "TUF_ROOT_CHAIN_INVALID"
    });
    expect(fixture.requests.some(({ url }) => url.endsWith("/metadata/timestamp.json"))).toBe(
      false
    );
    const maximumObservation = observationValues(fixture.storage).find(
      (value) => value.repositoryHighWater?.root?.version === 33
    );
    expect(maximumObservation).toBeDefined();
    expect(maximumObservation?.rootChain).toHaveLength(32);
    expect(
      observationValues(fixture.storage).some(
        (value) => value.repositoryHighWater?.root?.version === 34
      )
    ).toBe(false);

    const root34 = fixture.routes.get(root34Url);
    if (!maximumObservation || !root34?.bytes) throw new Error("missing root ceiling fixture");
    const overCapObservation = structuredClone(maximumObservation);
    overCapObservation.rootChain.push(new TextDecoder().decode(root34.bytes));
    const overCapKey = "opensecret:attestation-tuf:v4:repository-observation:over-cap-replay";
    backingStorage.setItem(overCapKey, JSON.stringify(overCapObservation));
    fixture.requests.length = 0;
    await expect(client(fixture).refresh("prod")).rejects.toMatchObject({
      code: "TUF_ROOT_CHAIN_INVALID"
    });
    expect(fixture.requests).toHaveLength(0);
    backingStorage.removeItem(overCapKey);

    fixture.requests.length = 0;
    await expect(client(fixture).refresh("prod")).rejects.toMatchObject({
      code: "TUF_ROOT_CHAIN_INVALID"
    });
    expect(fixture.requests.map(({ url }) => url)).toEqual([root34Url]);
    expect(
      observationValues(fixture.storage).some(
        (value) => value.repositoryHighWater?.root?.version === 34
      )
    ).toBe(false);

    fixture.routes.set(root34Url, { status: 404 });
    fixture.requests.length = 0;
    await expect(client(fixture).refresh("prod")).resolves.toMatchObject({
      metadataVersions: { root: 33 }
    });
    expect(fixture.requests.some(({ url }) => url.endsWith("/metadata/timestamp.json"))).toBe(true);

    for (const status of [403, 408, 429, 500, 503]) {
      fixture.routes.set(root34Url, { status });
      fixture.requests.length = 0;
      await expect(client(fixture).refresh("prod")).rejects.toMatchObject({
        code: "TUF_ROOT_CHAIN_INVALID"
      });
      expect(fixture.requests.map(({ url }) => url)).toEqual([root34Url]);
    }

    const routedFetch = fixture.fetch;
    fixture.fetch = (async (input: URL | RequestInfo, init?: RequestInit) => {
      const url = typeof input === "string" ? input : input instanceof URL ? input.href : input.url;
      if (url !== root34Url) return await routedFetch(input, init);
      fixture.requests.push({ url, init });
      return await new Promise<Response>((_resolve, reject) => {
        init?.signal?.addEventListener(
          "abort",
          () => reject(new DOMException("sentinel timeout", "AbortError")),
          { once: true }
        );
      });
    }) as typeof fetch;
    fixture.requests.length = 0;
    await expect(client(fixture).refresh("prod")).rejects.toMatchObject({
      code: "TUF_ROOT_CHAIN_INVALID"
    });
    expect(fixture.requests.map(({ url }) => url)).toEqual([root34Url]);
    fixture.fetch = routedFetch;

    fixture.routes.set(root34Url, {
      stream: new ReadableStream<Uint8Array>({
        start(controller) {
          controller.enqueue(new Uint8Array([0x7b]));
          controller.error(new Error("sentinel response interrupted"));
        }
      })
    });
    fixture.requests.length = 0;
    await expect(client(fixture).refresh("prod")).rejects.toMatchObject({
      code: "TUF_ROOT_CHAIN_INVALID"
    });
    expect(fixture.requests.map(({ url }) => url)).toEqual([root34Url]);

    fixture.routes.set(root34Url, { status: 404 });
    fixture.routes.set(fixture.urls.timestamp, { status: 503 });
    fixture.requests.length = 0;
    await expect(client(fixture).refresh("prod")).resolves.toMatchObject({
      metadataVersions: { root: 33 }
    });
    expect(fixture.requests.map(({ url }) => url)).toEqual([root34Url, fixture.urls.timestamp]);
  }, 120_000);

  test("never falls back after authenticating a newer partial repository generation", async () => {
    const initial = await buildTufFixture();
    await client(initial).refresh("prod");

    const advanced = await buildTufFixture({
      versions: { timestamp: 2, snapshot: 2, targets: 2 },
      sequence: 2
    });
    advanced.routes.set(advanced.urls.channel, { status: 404 });
    replaceRoutes(initial.routes as Map<string, unknown>, advanced.routes as Map<string, unknown>);

    await expect(client(initial).refresh("prod")).rejects.toMatchObject({
      code: "TUF_ROLLBACK"
    });

    const rolledBack = await buildTufFixture();
    const reloaded = createAttestationTufClientForTesting({
      fetch: rolledBack.fetch,
      storage: initial.storage,
      now: () => FIXTURE_NOW,
      bootstrap: initial.bootstrap
    });
    await expect(reloaded.refresh("prod")).rejects.toMatchObject({ code: "TUF_ROLLBACK" });
  });

  test("never commits a partial update and retries the same timestamp after repair", async () => {
    const initial = await buildTufFixture();
    const tuf = client(initial);
    await tuf.refresh("prod");
    const cacheKey = generationCacheKey(initial.storage);
    const before = initial.storage.getItem(cacheKey);

    const partial = await buildTufFixture({
      versions: { timestamp: 2, snapshot: 2, targets: 2 },
      sequence: 2,
      timestampSnapshotHash: "ff".repeat(32)
    });
    replaceRoutes(initial.routes as Map<string, unknown>, partial.routes as Map<string, unknown>);
    await expect(tuf.refresh("prod")).rejects.toBeInstanceOf(AttestationTrustError);
    expect(initial.storage.getItem(cacheKey)).toBe(before);

    const conflictingSameVersion = await buildTufFixture({
      versions: { timestamp: 2, snapshot: 2, targets: 2 },
      sequence: 2
    });
    replaceRoutes(
      initial.routes as Map<string, unknown>,
      conflictingSameVersion.routes as Map<string, unknown>
    );
    await expect(tuf.refresh("prod")).rejects.toMatchObject({ code: "TUF_ROLLBACK" });

    const repaired = await buildTufFixture({
      versions: { timestamp: 3, snapshot: 3, targets: 3 },
      sequence: 3
    });
    replaceRoutes(initial.routes as Map<string, unknown>, repaired.routes as Map<string, unknown>);
    await expect(tuf.refresh("prod")).resolves.toMatchObject({ sequence: 3 });
    expect(initial.storage.getItem(cacheKey)).not.toBe(before);
  });

  test("does not authorize a generation when persistent cache commit fails", async () => {
    const fixture = await buildTufFixture();
    const failingStorage: Storage = {
      ...fixture.storage,
      get length() {
        return fixture.storage.length;
      },
      getItem: fixture.storage.getItem.bind(fixture.storage),
      key: fixture.storage.key.bind(fixture.storage),
      removeItem: fixture.storage.removeItem.bind(fixture.storage),
      clear: fixture.storage.clear.bind(fixture.storage),
      setItem() {
        throw new Error("quota exceeded");
      }
    };
    const tuf = createAttestationTufClientForTesting({
      fetch: fixture.fetch,
      storage: failingStorage,
      now: () => FIXTURE_NOW,
      bootstrap: fixture.bootstrap
    });
    await expect(tuf.refresh("prod")).rejects.toMatchObject({ code: "TRUST_CACHE_INVALID" });
    expect(tuf.getMemoryPolicy("prod")).toBeUndefined();
  });

  test("does not hide a signature failure behind last-known-good policy", async () => {
    const initial = await buildTufFixture();
    const tuf = client(initial);
    await tuf.refresh("prod");
    const invalid = await buildTufFixture({ tamperTimestampSignature: true });
    replaceRoutes(initial.routes as Map<string, unknown>, invalid.routes as Map<string, unknown>);
    await expect(tuf.refresh("prod")).rejects.toMatchObject({ code: "TUF_SIGNATURE_INVALID" });
  });

  test("fails closed on an ambiguous fetch rejection that could be a blocked redirect", async () => {
    const fixture = await buildTufFixture();
    const tuf = client(fixture);
    await tuf.refresh("prod");
    fixture.routes.clear();
    await expect(tuf.refresh("prod")).rejects.toMatchObject({ code: "TRUST_FETCH_FAILED" });

    const expiredClient = createAttestationTufClientForTesting({
      fetch: mockFetch(
        new Map([
          [fixture.urls.nextRoot, { status: 404 }],
          [fixture.urls.timestamp, { status: 503 }]
        ])
      ),
      storage: fixture.storage,
      now: () => new Date("2030-01-03T00:00:00.000Z"),
      bootstrap: fixture.bootstrap
    });
    await expect(expiredClient.refresh("prod")).rejects.toMatchObject({
      code: "TRUST_NETWORK_UNAVAILABLE"
    });
  });

  test("shares repository metadata high-water marks across prod and dev", async () => {
    const prod = await buildTufFixture({
      sequence: 2,
      versions: { root: 2, timestamp: 2, snapshot: 2, targets: 2 }
    });
    const tuf = client(prod);
    await tuf.refresh("prod");
    expect(prod.requests.filter(({ url }) => url.endsWith("/2.root.json"))).toHaveLength(1);

    const dev = await buildTufFixture({
      environment: "dev",
      versions: { root: 2, timestamp: 3, snapshot: 3, targets: 3 }
    });
    replaceRoutes(prod.routes as Map<string, unknown>, dev.routes as Map<string, unknown>);
    await expect(tuf.refresh("dev")).resolves.toMatchObject({ environment: "dev" });
    expect(prod.requests.filter(({ url }) => url.endsWith("/2.root.json"))).toHaveLength(1);
    expect(prod.requests.filter(({ url }) => url.endsWith("/3.root.json"))).toHaveLength(2);

    prod.routes.set(dev.urls.timestamp, { status: 503 });
    await expect(tuf.refresh("prod")).rejects.toMatchObject({
      code: "TRUST_NETWORK_UNAVAILABLE"
    });
  });

  test("returns a deeply immutable verified policy", async () => {
    const fixture = await buildTufFixture();
    const policy = await client(fixture).refresh("prod");
    const pcrs = policy.releases[0].manifest.measurements.pcrs;
    expect(Object.isFrozen(policy)).toBe(true);
    expect(Object.isFrozen(policy.releases)).toBe(true);
    expect(Object.isFrozen(pcrs)).toBe(true);
    expect(() => {
      (pcrs as { "0": string })["0"] = "ff".repeat(48);
    }).toThrow();
    expect(validatePcrsAgainstSnapshot(hexPcrMap(), "prod", policy).isMatch).toBe(true);
  });

  test.each([404, 408, 429, 500, 503])(
    "uses an unexpired last-known-good generation when required metadata returns HTTP %i",
    async (status) => {
      const fixture = await buildTufFixture();
      const tuf = client(fixture);
      const first = await tuf.refresh("prod");
      fixture.routes.set(fixture.urls.timestamp, { status });
      await expect(tuf.refresh("prod")).resolves.toEqual(first);
    }
  );

  test("uses LKG after a response body is interrupted post-headers", async () => {
    const fixture = await buildTufFixture();
    const tuf = client(fixture);
    const first = await tuf.refresh("prod");
    fixture.routes.set(fixture.urls.timestamp, {
      stream: new ReadableStream({
        start(controller) {
          controller.error(new Error("connection interrupted"));
        }
      })
    });
    await expect(tuf.refresh("prod")).resolves.toEqual(first);
  });

  test("a stale browser context cannot overwrite newer policy in shared storage", async () => {
    const initial = await buildTufFixture();
    await client(initial).refresh("prod");

    const stale = await buildTufFixture({
      versions: { timestamp: 2, snapshot: 2, targets: 2 },
      sequence: 2
    });
    let releaseStale!: () => void;
    let markStaleReached!: () => void;
    const staleGate = new Promise<void>((resolve) => {
      releaseStale = resolve;
    });
    const staleReached = new Promise<void>((resolve) => {
      markStaleReached = resolve;
    });
    const pausedFetch = (async (input: URL | RequestInfo, init?: RequestInit) => {
      const url = typeof input === "string" ? input : input instanceof URL ? input.href : input.url;
      if (url === stale.urls.manifest) {
        markStaleReached();
        await staleGate;
      }
      return stale.fetch(input, init);
    }) as typeof fetch;
    const staleClient = createAttestationTufClientForTesting({
      fetch: pausedFetch,
      storage: initial.storage,
      now: () => FIXTURE_NOW,
      bootstrap: initial.bootstrap
    });
    const staleRefresh = staleClient.refresh("prod");
    await staleReached;

    const current = await buildTufFixture({
      versions: { timestamp: 3, snapshot: 3, targets: 3 },
      sequence: 3
    });
    const currentClient = createAttestationTufClientForTesting({
      fetch: current.fetch,
      storage: initial.storage,
      now: () => FIXTURE_NOW,
      bootstrap: initial.bootstrap
    });
    await expect(currentClient.refresh("prod")).resolves.toMatchObject({ sequence: 3 });

    releaseStale();
    await expect(staleRefresh).rejects.toMatchObject({ code: "TUF_ROLLBACK" });
    await expect(currentClient.refresh("prod")).resolves.toMatchObject({ sequence: 3 });
  });

  test("rejects a held policy after another browser context commits a revocation", async () => {
    const initial = await buildTufFixture();
    const staleClient = client(initial);
    const stalePolicy = await staleClient.refresh("prod");

    const revoked = await buildTufFixture({
      versions: { timestamp: 2, snapshot: 2, targets: 2 },
      sequence: 2,
      pcrs: { "0": "08".repeat(48), "1": "09".repeat(48), "2": "0a".repeat(48) }
    });
    replaceRoutes(initial.routes as Map<string, unknown>, revoked.routes as Map<string, unknown>);
    const currentClient = client(initial);
    const currentPolicy = await currentClient.refresh("prod");

    await expect(staleClient.assertPolicyCurrent(stalePolicy)).rejects.toMatchObject({
      code: "TUF_ROLLBACK"
    });
    await expect(currentClient.assertPolicyCurrent(currentPolicy)).resolves.toBeUndefined();
  });

  test("currentness ignores a retained generation from before remote root rotation", async () => {
    const initial = await buildTufFixture();
    const retainingStorage = storageWithoutCleanup(initial.storage);
    const initialClient = createAttestationTufClientForTesting({
      fetch: initial.fetch,
      storage: retainingStorage,
      now: () => FIXTURE_NOW,
      bootstrap: initial.bootstrap
    });
    await initialClient.refresh("prod");

    const rotated = await buildTufFixture({ versions: { root: 2 } });
    replaceRoutes(initial.routes as Map<string, unknown>, rotated.routes as Map<string, unknown>);
    const rotatingClient = createAttestationTufClientForTesting({
      fetch: initial.fetch,
      storage: retainingStorage,
      now: () => FIXTURE_NOW,
      bootstrap: initial.bootstrap
    });
    const policy = await rotatingClient.refresh("prod");
    const retainedGenerations = Array.from({ length: initial.storage.length }, (_, index) =>
      initial.storage.key(index)
    ).filter((key) => key?.startsWith("opensecret:attestation-tuf:v4:prod:"));
    expect(retainedGenerations).toHaveLength(2);

    expect(policy.metadataVersions.root).toBe(2);
    await expect(rotatingClient.assertPolicyCurrent(policy)).resolves.toBeUndefined();
  });

  test("a newer root-only observation after commit cannot publish a stale policy", async () => {
    const initial = await buildTufFixture();
    await client(initial).refresh("prod");
    const stale = await buildTufFixture({
      versions: { timestamp: 2, snapshot: 2, targets: 2 },
      sequence: 2
    });
    const current = await buildTufFixture({
      versions: { root: 2, timestamp: 3, snapshot: 3, targets: 3 },
      sequence: 3
    });
    const currentTimestamp = current.routes.get(current.urls.timestamp)!;
    current.routes.set(current.urls.timestamp, { status: 503 });

    let pauseNextDigest = false;
    let markStalePaused!: () => void;
    let releaseStale!: () => void;
    const stalePaused = new Promise<void>((resolve) => {
      markStalePaused = resolve;
    });
    const staleGate = new Promise<void>((resolve) => {
      releaseStale = resolve;
    });
    const backing = initial.storage;
    const staleStorage: Storage = {
      get length() {
        return backing.length;
      },
      clear: backing.clear.bind(backing),
      getItem: backing.getItem.bind(backing),
      key: backing.key.bind(backing),
      removeItem(key) {
        backing.removeItem(key);
        if (key.startsWith("opensecret:attestation-tuf:v4:prod:")) {
          // Generation cleanup is the final synchronous step after commit's
          // post-write generation/journal checks. Pause the next verification
          // digest before the complete observation can be persisted.
          pauseNextDigest = true;
        }
      },
      setItem: backing.setItem.bind(backing)
    };
    const subtle = crypto.subtle as SubtleCrypto & {
      digest: SubtleCrypto["digest"];
    };
    const originalDigest = subtle.digest.bind(subtle);
    subtle.digest = (async (...args: Parameters<SubtleCrypto["digest"]>) => {
      if (pauseNextDigest) {
        pauseNextDigest = false;
        markStalePaused();
        await staleGate;
      }
      return originalDigest(...args);
    }) as SubtleCrypto["digest"];

    try {
      const staleClient = createAttestationTufClientForTesting({
        fetch: stale.fetch,
        storage: staleStorage,
        now: () => FIXTURE_NOW,
        bootstrap: initial.bootstrap
      });
      const staleRefresh = staleClient.refresh("prod");
      await stalePaused;

      const currentClient = createAttestationTufClientForTesting({
        fetch: current.fetch,
        storage: storageView(initial.storage),
        now: () => FIXTURE_NOW,
        bootstrap: initial.bootstrap
      });
      await expect(currentClient.refresh("prod")).rejects.toMatchObject({ code: "TUF_ROLLBACK" });

      releaseStale();
      await expect(staleRefresh).rejects.toMatchObject({ code: "TUF_ROLLBACK" });
      expect(staleClient.getMemoryPolicy("prod")).toMatchObject({ sequence: 1 });

      current.routes.set(current.urls.timestamp, currentTimestamp);
      current.requests.length = 0;
      const reloaded = createAttestationTufClientForTesting({
        fetch: current.fetch,
        storage: storageView(initial.storage),
        now: () => FIXTURE_NOW,
        bootstrap: initial.bootstrap
      });
      await expect(reloaded.refresh("prod")).resolves.toMatchObject({ sequence: 3 });
      expect(current.requests.some(({ url }) => url.endsWith("/metadata/2.root.json"))).toBe(false);
      expect(current.requests.some(({ url }) => url.endsWith("/metadata/3.root.json"))).toBe(true);
    } finally {
      subtle.digest = originalDigest as SubtleCrypto["digest"];
      releaseStale();
    }
  });

  test("a no-Web-Locks tab rechecks high-water after its immutable cache write", async () => {
    const initial = await buildTufFixture();
    await client(initial).refresh("prod");
    const stale = await buildTufFixture({
      versions: { timestamp: 2, snapshot: 2, targets: 2 },
      sequence: 2
    });
    const current = await buildTufFixture({
      versions: { timestamp: 3, snapshot: 3, targets: 3 },
      sequence: 3
    });

    let staleStorageReads = 0;
    let pauseNextDigest = false;
    let markStalePaused!: () => void;
    let releaseStale!: () => void;
    const stalePaused = new Promise<void>((resolve) => {
      markStalePaused = resolve;
    });
    const staleGate = new Promise<void>((resolve) => {
      releaseStale = resolve;
    });
    const staleStorage = storageView(initial.storage, () => {
      staleStorageReads += 1;
      if (staleStorageReads === 2) pauseNextDigest = true;
    });
    const currentStorage = storageView(initial.storage);
    const subtle = crypto.subtle as SubtleCrypto & {
      digest: SubtleCrypto["digest"];
    };
    const originalDigest = subtle.digest.bind(subtle);
    subtle.digest = (async (...args: Parameters<SubtleCrypto["digest"]>) => {
      if (pauseNextDigest) {
        pauseNextDigest = false;
        markStalePaused();
        await staleGate;
      }
      return originalDigest(...args);
    }) as SubtleCrypto["digest"];

    try {
      const staleClient = createAttestationTufClientForTesting({
        fetch: stale.fetch,
        storage: staleStorage,
        now: () => FIXTURE_NOW,
        bootstrap: initial.bootstrap
      });
      const staleRefresh = staleClient.refresh("prod");
      await stalePaused;

      const currentClient = createAttestationTufClientForTesting({
        fetch: current.fetch,
        storage: currentStorage,
        now: () => FIXTURE_NOW,
        bootstrap: initial.bootstrap
      });
      await expect(currentClient.refresh("prod")).resolves.toMatchObject({ sequence: 3 });
      releaseStale();
      await expect(staleRefresh).rejects.toMatchObject({ code: "TUF_ROLLBACK" });

      const reloaded = createAttestationTufClientForTesting({
        fetch: current.fetch,
        storage: storageView(initial.storage),
        now: () => FIXTURE_NOW,
        bootstrap: initial.bootstrap
      });
      await expect(reloaded.refresh("prod")).resolves.toMatchObject({ sequence: 3 });
    } finally {
      subtle.digest = originalDigest as SubtleCrypto["digest"];
      releaseStale();
    }
  });
});

test("a compaction race cannot hide a newer persisted generation from fallback", async () => {
  const initial = await buildTufFixture();
  const backing = initial.storage;
  const retainingStorage: Storage = {
    get length() {
      return backing.length;
    },
    clear: backing.clear.bind(backing),
    getItem: backing.getItem.bind(backing),
    key: backing.key.bind(backing),
    removeItem: () => undefined,
    setItem: backing.setItem.bind(backing)
  };
  const first = createAttestationTufClientForTesting({
    fetch: initial.fetch,
    storage: retainingStorage,
    now: () => FIXTURE_NOW,
    bootstrap: initial.bootstrap
  });
  await first.refresh("prod");

  const newer = await buildTufFixture({
    versions: { timestamp: 3, snapshot: 3, targets: 3 },
    sequence: 3
  });
  replaceRoutes(initial.routes as Map<string, unknown>, newer.routes as Map<string, unknown>);
  await first.refresh("prod");
  initial.routes.set(newer.urls.timestamp, { status: 503 });

  let raced = false;
  const racingStorage: Storage = {
    get length() {
      return backing.length;
    },
    clear: backing.clear.bind(backing),
    getItem: backing.getItem.bind(backing),
    key(index) {
      const key = backing.key(index);
      if (!raced && key?.startsWith("opensecret:attestation-tuf:v4:prod:")) {
        raced = true;
        backing.removeItem(key);
      }
      return key;
    },
    removeItem: backing.removeItem.bind(backing),
    setItem: backing.setItem.bind(backing)
  };
  const reloaded = createAttestationTufClientForTesting({
    fetch: initial.fetch,
    storage: racingStorage,
    now: () => FIXTURE_NOW,
    bootstrap: initial.bootstrap
  });

  await expect(reloaded.refresh("prod")).resolves.toMatchObject({ sequence: 3 });
  expect(raced).toBe(true);
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
