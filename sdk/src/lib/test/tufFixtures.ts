import nacl from "tweetnacl";
import { canonicalJsonBytes } from "../attestationTuf";

export const FIXTURE_NOW = new Date("2030-01-01T00:00:00.000Z");
export const PCR0 = "01".repeat(48);
export const PCR1 = "02".repeat(48);
export const PCR2 = "03".repeat(48);
const BASE = "https://attestations.trymaple.ai/tuf";
const encoder = new TextEncoder();
type OnlineRole = "timestamp" | "snapshot" | "targets";

export type FixtureOptions = {
  environment?: "prod" | "dev";
  sequence?: number;
  versions?: Partial<{ root: number; timestamp: number; snapshot: number; targets: number }>;
  expires?: Partial<{ root: string; timestamp: string; snapshot: string; targets: string }>;
  pcrs?: { "0": string; "1": string; "2": string };
  extraReleasePcrs?: { "0": string; "1": string; "2": string };
  thirdReleasePcrs?: { "0": string; "1": string; "2": string };
  emptyActive?: boolean;
  timestampSnapshotVersion?: number;
  snapshotTargetsVersion?: number;
  timestampSnapshotLengthDelta?: number;
  timestampSnapshotHash?: string;
  snapshotTargetsLengthDelta?: number;
  snapshotTargetsHash?: string;
  channelManifestHash?: string;
  tamperTimestampSignature?: boolean;
  tamperManifestBody?: boolean;
  rootEnvelopeVersionOverride?: number;
  tamperRootSignatureVersion?: number;
  duplicateAuthorizedKeyMaterialAlias?: boolean;
  sourceUri?: string;
  sourcePath?: string;
  artifactName?: string;
  runUri?: string;
  rootSigningSeed?: number;
  rootRoleKeySeedsByRootVersion?: Partial<Record<number, readonly number[]>>;
  rootRoleThresholdsByRootVersion?: Partial<Record<number, number>>;
  signingSeed?: number;
  metadataRoleKeySeedsByRootVersion?: Partial<
    Record<number, Partial<Record<OnlineRole, readonly number[]>>>
  >;
  metadataRoleThresholdsByRootVersion?: Partial<
    Record<number, Partial<Record<OnlineRole, number>>>
  >;
  metadataSigningSeeds?: Partial<Record<OnlineRole, number | readonly number[]>>;
};

type Route = {
  bytes?: Uint8Array;
  status?: number;
  redirected?: boolean;
  finalUrl?: string;
  headers?: Record<string, string>;
  stream?: ReadableStream<Uint8Array>;
};

export type TufFixture = {
  bootstrap: unknown;
  rootEnvelopes: Record<number, unknown>;
  routes: Map<string, Route>;
  fetch: typeof fetch;
  storage: Storage;
  requests: Array<{ url: string; init?: RequestInit }>;
  pcrs: { "0": string; "1": string; "2": string };
  urls: {
    nextRoot: string;
    timestamp: string;
    snapshot: string;
    targets: string;
    channel: string;
    bundle: string;
    manifest: string;
  };
};

function toHex(bytes: Uint8Array): string {
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("");
}

async function sha256(bytes: Uint8Array): Promise<string> {
  return toHex(new Uint8Array(await crypto.subtle.digest("SHA-256", bytes)));
}

function jsonBytes(value: unknown): Uint8Array {
  return encoder.encode(JSON.stringify(value));
}

async function signedBy(
  signedValue: Record<string, unknown>,
  signers: ReadonlyArray<{ keyid: string; signer: nacl.SignKeyPair }>
) {
  return {
    signatures: signers.map(({ keyid, signer }) => ({
      keyid,
      sig: toHex(nacl.sign.detached(canonicalJsonBytes(signedValue), signer.secretKey))
    })),
    signed: signedValue
  };
}

async function descriptor(bytes: Uint8Array) {
  return { length: bytes.byteLength, hashes: { sha256: await sha256(bytes) } };
}

function consistentTargetUrl(path: string, digest: string): string {
  const separator = path.lastIndexOf("/");
  const directory = separator === -1 ? "" : path.slice(0, separator + 1);
  const basename = path.slice(separator + 1);
  return `${BASE}/targets/${directory}${digest}.${basename}`;
}

function makeStorage(): Storage {
  const values = new Map<string, string>();
  return {
    get length() {
      return values.size;
    },
    clear() {
      values.clear();
    },
    getItem(key) {
      return values.get(key) ?? null;
    },
    key(index) {
      return [...values.keys()][index] ?? null;
    },
    removeItem(key) {
      values.delete(key);
    },
    setItem(key, value) {
      values.set(key, String(value));
    },
    [Symbol.iterator]() {
      return values.keys();
    }
  };
}

function responseFor(url: string, route: Route): Response {
  const response = route.stream
    ? new Response(route.stream, { status: route.status ?? 200, headers: route.headers })
    : new Response(route.bytes ?? new Uint8Array(), {
        status: route.status ?? 200,
        headers: route.headers
      });
  Object.defineProperties(response, {
    url: { value: route.finalUrl ?? url },
    redirected: { value: route.redirected ?? false }
  });
  return response;
}

export function mockFetch(
  routes: Map<string, Route>,
  requests: Array<{ url: string; init?: RequestInit }> = []
): typeof fetch {
  return (async (input: URL | RequestInfo, init?: RequestInit) => {
    const url = typeof input === "string" ? input : input instanceof URL ? input.href : input.url;
    requests.push({ url, init });
    const route = routes.get(url);
    if (!route) throw new Error(`Unexpected TUF URL: ${url}`);
    return responseFor(url, route);
  }) as typeof fetch;
}

export async function buildTufFixture(options: FixtureOptions = {}): Promise<TufFixture> {
  const defaultSeed = options.signingSeed ?? 7;
  const environment = options.environment ?? "prod";
  const versions = {
    root: options.versions?.root ?? 1,
    timestamp: options.versions?.timestamp ?? 1,
    snapshot: options.versions?.snapshot ?? 1,
    targets: options.versions?.targets ?? 1
  };
  const expires = {
    root: options.expires?.root ?? "2030-12-31T00:00:00.000Z",
    timestamp: options.expires?.timestamp ?? "2030-01-02T23:00:00.000Z",
    snapshot: options.expires?.snapshot ?? "2030-02-01T00:00:00.000Z",
    targets: options.expires?.targets ?? "2030-03-01T00:00:00.000Z"
  };
  const pcrs = options.pcrs ?? { "0": PCR0, "1": PCR1, "2": PCR2 };
  const materialForSeed = async (seed: number) => {
    const signer = nacl.sign.keyPair.fromSeed(new Uint8Array(32).fill(seed));
    const key = {
      keytype: "ed25519" as const,
      scheme: "ed25519" as const,
      keyval: { public: toHex(signer.publicKey) }
    };
    return { signer, key, keyid: await sha256(canonicalJsonBytes(key)) };
  };
  const rootSeedsAt = (version: number): readonly number[] => {
    let seeds: readonly number[] | undefined;
    for (const [declaredVersion, declaredSeeds] of Object.entries(
      options.rootRoleKeySeedsByRootVersion ?? {}
    )) {
      if (Number(declaredVersion) <= version) seeds = declaredSeeds;
    }
    return seeds ?? [options.rootSigningSeed ?? 6];
  };
  const rootThresholdAt = (version: number): number => {
    let threshold: number | undefined;
    for (const [declaredVersion, declaredThreshold] of Object.entries(
      options.rootRoleThresholdsByRootVersion ?? {}
    )) {
      if (Number(declaredVersion) <= version) threshold = declaredThreshold;
    }
    return threshold ?? 1;
  };
  const roleSeedsAt = (version: number, role: OnlineRole): readonly number[] => {
    let seeds: readonly number[] | undefined;
    for (const [declaredVersion, roles] of Object.entries(
      options.metadataRoleKeySeedsByRootVersion ?? {}
    )) {
      if (Number(declaredVersion) <= version && roles?.[role]) seeds = roles[role];
    }
    return seeds ?? [defaultSeed];
  };
  const roleThresholdAt = (version: number, role: OnlineRole): number => {
    let threshold: number | undefined;
    for (const [declaredVersion, roles] of Object.entries(
      options.metadataRoleThresholdsByRootVersion ?? {}
    )) {
      if (Number(declaredVersion) <= version && roles?.[role] !== undefined) {
        threshold = roles[role];
      }
    }
    return threshold ?? 1;
  };
  const rootForVersion = async (version: number, signedVersion = version) => {
    const rootMaterials = await Promise.all(rootSeedsAt(version).map(materialForSeed));
    const previousRootMaterials =
      version === 1
        ? rootMaterials
        : await Promise.all(rootSeedsAt(version - 1).map(materialForSeed));
    const keys: Record<string, Awaited<ReturnType<typeof materialForSeed>>["key"]> = {};
    for (const material of rootMaterials) keys[material.keyid] = material.key;
    const roles: Record<string, { keyids: string[]; threshold: number }> = {
      root: {
        keyids: rootMaterials.map((material) => material.keyid),
        threshold: rootThresholdAt(version)
      }
    };
    for (const role of ["timestamp", "snapshot", "targets"] as const) {
      const materials = await Promise.all(roleSeedsAt(version, role).map(materialForSeed));
      for (const material of materials) keys[material.keyid] = material.key;
      roles[role] = {
        keyids: materials.map((material) => material.keyid),
        threshold: roleThresholdAt(version, role)
      };
    }
    if (options.duplicateAuthorizedKeyMaterialAlias && version === 1) {
      const alias = "ff".repeat(32);
      keys[alias] = keys[roles.timestamp.keyids[0]];
      roles.timestamp.keyids.push(alias);
    }
    const rootSigners = [...previousRootMaterials, ...rootMaterials].filter(
      (material, index, all) => all.findIndex(({ keyid }) => keyid === material.keyid) === index
    );
    return await signedBy(
      {
        _type: "root",
        spec_version: "1.0.36",
        version: signedVersion,
        expires: expires.root,
        consistent_snapshot: true,
        keys,
        roles
      },
      rootSigners.map(({ keyid, signer }) => ({ keyid, signer }))
    );
  };
  const bootstrapRoot = await rootForVersion(1);
  const bootstrap = bootstrapRoot;
  const rootEnvelopes: Record<number, unknown> = { 1: bootstrapRoot };
  for (let version = 2; version <= versions.root; version += 1) {
    rootEnvelopes[version] = await rootForVersion(version);
  }
  const metadataMaterials = async (role: OnlineRole) => {
    const configured = options.metadataSigningSeeds?.[role];
    const seeds =
      configured === undefined
        ? roleSeedsAt(versions.root, role).slice(0, roleThresholdAt(versions.root, role))
        : typeof configured === "number"
          ? [configured]
          : configured;
    return await Promise.all(seeds.map(materialForSeed));
  };
  const [timestampMaterials, snapshotMaterials, targetsMaterials] = await Promise.all([
    metadataMaterials("timestamp"),
    metadataMaterials("snapshot"),
    metadataMaterials("targets")
  ]);

  const trustedRootPath = "sigstore/trusted_root.json";
  const channelPath = `channels/${environment}.json`;
  const manifestPath = `releases/1.0.0/${environment}/manifest.json`;
  const bundlePath = `releases/1.0.0/${environment}/manifest.sigstore.json`;
  const trustedRootBytes = jsonBytes({
    mediaType: "application/vnd.dev.sigstore.trustedroot+json;version=0.1"
  });
  const bundleBytes = jsonBytes({ mediaType: "application/vnd.dev.sigstore.bundle.v0.3+json" });

  const manifestValue = (releasePcrs: typeof pcrs, version = "1.0.0") => ({
    schema: "https://attestations.trymaple.ai/schemas/nitro-eif-release/v1",
    component: "opensecret-backend",
    environment,
    release: { version },
    source: {
      uri: options.sourceUri ?? "https://github.com/OpenSecretCloud/opensecret",
      path: options.sourcePath ?? ".",
      ref: `refs/tags/v${version}`,
      revision: { algorithm: "git-sha1", digest: "12".repeat(20) }
    },
    artifact: {
      name: options.artifactName ?? `opensecret-${version}-${environment}.eif`,
      mediaType: "application/vnd.aws.nitro.eif",
      size: 123,
      digests: { sha256: "13".repeat(32) }
    },
    measurements: {
      algorithm: "sha384",
      requiredPcrs: [0, 1, 2],
      pcrs: releasePcrs
    },
    build: {
      system: "nix",
      builderId: "github-opensecret-v1",
      derivation: `eif-${environment}`,
      flakeLockSha256: "14".repeat(32),
      runUri: options.runUri ?? "https://ci.example.test/runs/1"
    }
  });
  let manifestBytes = jsonBytes(manifestValue(pcrs));
  if (options.tamperManifestBody) {
    manifestBytes = new Uint8Array(manifestBytes);
    manifestBytes[manifestBytes.length - 2] ^= 1;
  }

  const targetPayloads = new Map<string, Uint8Array>([
    [trustedRootPath, trustedRootBytes],
    [manifestPath, manifestBytes],
    [bundlePath, bundleBytes]
  ]);
  const targetDescriptors: Record<string, Awaited<ReturnType<typeof descriptor>>> = {};
  for (const [path, bytes] of targetPayloads) targetDescriptors[path] = await descriptor(bytes);

  const active: Array<{
    manifestTarget: string;
    manifestSha256: string;
    bundleTarget: string;
    bundleSha256: string;
  }> = options.emptyActive
    ? []
    : [
        {
          manifestTarget: manifestPath,
          manifestSha256:
            options.channelManifestHash ?? targetDescriptors[manifestPath].hashes.sha256,
          bundleTarget: bundlePath,
          bundleSha256: targetDescriptors[bundlePath].hashes.sha256
        }
      ];
  if (options.extraReleasePcrs) {
    const extraManifestPath = `releases/1.0.1/${environment}/manifest.json`;
    const extraBundlePath = `releases/1.0.1/${environment}/manifest.sigstore.json`;
    const extraManifestBytes = jsonBytes(manifestValue(options.extraReleasePcrs, "1.0.1"));
    targetPayloads.set(extraManifestPath, extraManifestBytes);
    targetPayloads.set(extraBundlePath, bundleBytes);
    targetDescriptors[extraManifestPath] = await descriptor(extraManifestBytes);
    targetDescriptors[extraBundlePath] = await descriptor(bundleBytes);
    active.push({
      manifestTarget: extraManifestPath,
      manifestSha256: targetDescriptors[extraManifestPath].hashes.sha256,
      bundleTarget: extraBundlePath,
      bundleSha256: targetDescriptors[extraBundlePath].hashes.sha256
    });
  }
  if (options.thirdReleasePcrs) {
    const thirdManifestPath = `releases/1.0.2/${environment}/manifest.json`;
    const thirdBundlePath = `releases/1.0.2/${environment}/manifest.sigstore.json`;
    const thirdManifestBytes = jsonBytes(manifestValue(options.thirdReleasePcrs, "1.0.2"));
    targetPayloads.set(thirdManifestPath, thirdManifestBytes);
    targetPayloads.set(thirdBundlePath, bundleBytes);
    targetDescriptors[thirdManifestPath] = await descriptor(thirdManifestBytes);
    targetDescriptors[thirdBundlePath] = await descriptor(bundleBytes);
    active.push({
      manifestTarget: thirdManifestPath,
      manifestSha256: targetDescriptors[thirdManifestPath].hashes.sha256,
      bundleTarget: thirdBundlePath,
      bundleSha256: targetDescriptors[thirdBundlePath].hashes.sha256
    });
  }

  const channelBytes = jsonBytes({
    schema: "https://attestations.trymaple.ai/schemas/channel/v1",
    environment,
    sequence: options.sequence ?? 1,
    sigstoreTrustedRootTarget: {
      path: trustedRootPath,
      sha256: targetDescriptors[trustedRootPath].hashes.sha256
    },
    active
  });
  targetPayloads.set(channelPath, channelBytes);
  targetDescriptors[channelPath] = await descriptor(channelBytes);

  const targetsEnvelope = await signedBy(
    {
      _type: "targets",
      spec_version: "1.0.36",
      version: versions.targets,
      expires: expires.targets,
      targets: targetDescriptors
    },
    targetsMaterials
  );
  const targetsBytes = jsonBytes(targetsEnvelope);
  const targetsMeta = await descriptor(targetsBytes);
  targetsMeta.length += options.snapshotTargetsLengthDelta ?? 0;
  if (options.snapshotTargetsHash) targetsMeta.hashes.sha256 = options.snapshotTargetsHash;

  const snapshotEnvelope = await signedBy(
    {
      _type: "snapshot",
      spec_version: "1.0.36",
      version: versions.snapshot,
      expires: expires.snapshot,
      meta: {
        "targets.json": {
          ...targetsMeta,
          version: options.snapshotTargetsVersion ?? versions.targets
        }
      }
    },
    snapshotMaterials
  );
  const snapshotBytes = jsonBytes(snapshotEnvelope);
  const snapshotMeta = await descriptor(snapshotBytes);
  snapshotMeta.length += options.timestampSnapshotLengthDelta ?? 0;
  if (options.timestampSnapshotHash) snapshotMeta.hashes.sha256 = options.timestampSnapshotHash;

  const timestampEnvelope = await signedBy(
    {
      _type: "timestamp",
      spec_version: "1.0.36",
      version: versions.timestamp,
      expires: expires.timestamp,
      meta: {
        "snapshot.json": {
          ...snapshotMeta,
          version: options.timestampSnapshotVersion ?? versions.snapshot
        }
      }
    },
    timestampMaterials
  );
  if (options.tamperTimestampSignature) timestampEnvelope.signatures[0].sig = "00".repeat(64);
  const timestampBytes = jsonBytes(timestampEnvelope);

  const routes = new Map<string, Route>();
  for (let version = 2; version <= versions.root; version += 1) {
    const signedVersion =
      version === versions.root && options.rootEnvelopeVersionOverride !== undefined
        ? options.rootEnvelopeVersionOverride
        : version;
    const envelope = JSON.parse(
      JSON.stringify(
        signedVersion === version
          ? rootEnvelopes[version]
          : await rootForVersion(version, signedVersion)
      )
    ) as { signatures: Array<{ sig: string }> };
    if (options.tamperRootSignatureVersion === version) {
      envelope.signatures[0].sig = "00".repeat(64);
    }
    routes.set(`${BASE}/metadata/${version}.root.json`, { bytes: jsonBytes(envelope) });
  }
  const nextRoot = `${BASE}/metadata/${versions.root + 1}.root.json`;
  routes.set(nextRoot, { status: 404 });
  const timestampUrl = `${BASE}/metadata/timestamp.json`;
  const snapshotUrl = `${BASE}/metadata/${options.timestampSnapshotVersion ?? versions.snapshot}.snapshot.json`;
  const targetsUrl = `${BASE}/metadata/${options.snapshotTargetsVersion ?? versions.targets}.targets.json`;
  routes.set(timestampUrl, { bytes: timestampBytes });
  routes.set(snapshotUrl, { bytes: snapshotBytes });
  routes.set(targetsUrl, { bytes: targetsBytes });
  for (const [path, bytes] of targetPayloads) {
    routes.set(consistentTargetUrl(path, targetDescriptors[path].hashes.sha256), { bytes });
  }
  const requests: Array<{ url: string; init?: RequestInit }> = [];
  return {
    bootstrap,
    rootEnvelopes,
    routes,
    fetch: mockFetch(routes, requests),
    storage: makeStorage(),
    requests,
    pcrs,
    urls: {
      nextRoot,
      timestamp: timestampUrl,
      snapshot: snapshotUrl,
      targets: targetsUrl,
      channel: consistentTargetUrl(channelPath, targetDescriptors[channelPath].hashes.sha256),
      bundle: consistentTargetUrl(bundlePath, targetDescriptors[bundlePath].hashes.sha256),
      manifest: consistentTargetUrl(manifestPath, targetDescriptors[manifestPath].hashes.sha256)
    }
  };
}

export function hexPcrMap(values = { "0": PCR0, "1": PCR1, "2": PCR2 }) {
  return new Map<number, Uint8Array>(
    ([0, 1, 2] as const).map((index) => [
      index,
      new Uint8Array(
        values[String(index) as "0" | "1" | "2"]
          .match(/../g)!
          .map((byte) => Number.parseInt(byte, 16))
      )
    ])
  );
}
