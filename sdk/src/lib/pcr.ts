import { z } from "zod";
import {
  assertAttestationPolicyCurrent,
  getCachedAttestationPolicy,
  refreshAttestationPolicy,
  type AttestationChannel,
  type TrustedTufRelease,
  type VerifiedAttestationPolicy
} from "./attestationTuf";

const LOCAL_DEVELOPMENT_API_HOSTS = new Set(["127.0.0.1", "localhost", "[::1]"]);

export const AttestationEnvironmentSchema = z.enum(["prod", "dev"]);
export type AttestationEnvironment = AttestationChannel;
export type TrustedEnclaveRelease = TrustedTufRelease;
export type TrustedEnclaveReleaseSnapshot = VerifiedAttestationPolicy;

/**
 * Attestation policy configuration. Official clients retrieve current policy
 * from https://attestations.trymaple.ai/tuf. Raw allowlists and GitHub history
 * URLs are not trust inputs.
 */
export type PcrConfig = {
  environment?: AttestationEnvironment;
  /** @deprecated Raw PCR overrides are not an authorization mechanism. */
  pcr0Values?: never;
  /** @deprecated Raw PCR overrides are not an authorization mechanism. */
  pcr0DevValues?: never;
  /** @deprecated Attestation policy has one fixed, authenticated TUF origin. */
  remoteAttestation?: never;
  /** @deprecated Attestation policy has one fixed, authenticated TUF origin. */
  remoteAttestationUrls?: never;
};

export function snapshotPcrConfig(config?: PcrConfig): PcrConfig {
  return Object.freeze({ environment: config?.environment });
}

export function serializePcrConfig(config?: PcrConfig): string {
  const snapshot = snapshotPcrConfig(config);
  return JSON.stringify({
    version: "attestation-tuf-v1",
    environment: snapshot.environment ?? null
  });
}

export type Pcr0ValidationResult = {
  isMatch: boolean;
  text: string;
  environment?: AttestationEnvironment;
  releaseTag?: string;
  releaseVersion?: string;
  sourceCommit?: string;
  sourceRef?: string;
  artifactSha256?: string;
  manifestSha256?: string;
  bundleSha256?: string;
  snapshotId: string;
  channelSequence?: number;
  builderId?: string;
  /** Authenticated policy that the promotion pipeline applies to the Sigstore certificate. */
  signerIdentityPolicy?: string;
  /** @deprecated Browser runtime does not observe or verify a Sigstore signer identity. */
  signerIdentity?: string;
  oidcIssuer?: string;
  sigstoreTrustedRootSha256?: string;
  /** Browser runtime does not interpret transparency evidence from the bundle. */
  transparencyLog?: { logIndex: string; logId: string };
  /** @deprecated Sigstore timestamps are verified by the promotion pipeline. */
  verifiedAt?: string;
};

export type PcrValidationResult = Pcr0ValidationResult;

const OFFICIAL_ENVIRONMENTS_BY_ORIGIN = new Map<string, AttestationEnvironment>([
  ["https://api.opensecret.cloud", "prod"],
  ["https://developer.opensecret.cloud", "prod"],
  ["https://enclave.trymaple.ai", "prod"],
  ["https://enclave.secretgpt.ai", "dev"]
]);

export function normalizeApiOrigin(apiUrl: string): string {
  const url = new URL(apiUrl);
  if (url.username || url.password || url.search || url.hash) {
    throw new Error("Attestation API URL must not include credentials, a query, or a fragment.");
  }
  const isExactLoopback = LOCAL_DEVELOPMENT_API_HOSTS.has(url.hostname.toLowerCase());
  if (url.protocol !== "https:" && !(url.protocol === "http:" && isExactLoopback)) {
    throw new Error("Attestation API URL must use HTTPS unless it is an exact loopback host.");
  }
  return url.origin;
}

export function normalizeApiBaseUrl(apiUrl: string): string {
  const origin = normalizeApiOrigin(apiUrl);
  const url = new URL(apiUrl);
  const pathname = url.pathname === "/" ? "" : url.pathname.replace(/\/+$/, "");
  return `${origin}${pathname}`;
}

export function resolveAttestationEnvironment(
  apiUrl: string,
  explicitEnvironment?: AttestationEnvironment
): AttestationEnvironment {
  if (
    explicitEnvironment !== undefined &&
    !AttestationEnvironmentSchema.safeParse(explicitEnvironment).success
  ) {
    throw new Error("Attestation environment must be exactly prod or dev.");
  }
  const origin = normalizeApiOrigin(apiUrl);
  const officialEnvironment = OFFICIAL_ENVIRONMENTS_BY_ORIGIN.get(origin);
  if (officialEnvironment && explicitEnvironment && explicitEnvironment !== officialEnvironment) {
    throw new Error(
      `Attestation environment ${explicitEnvironment} is not allowed for official origin ${origin}.`
    );
  }
  const environment = officialEnvironment ?? explicitEnvironment;
  if (!environment) {
    throw new Error(
      `Attestation environment must be configured explicitly for non-official origin ${origin}.`
    );
  }
  return environment;
}

/** @deprecated Use the snapshotId returned by requireTrustedPcrs. */
export function getTrustedReleaseSnapshotId(environment?: AttestationEnvironment): string {
  if (environment) return getCachedAttestationPolicy(environment)?.policyId ?? "unavailable";
  return (
    getCachedAttestationPolicy("prod")?.policyId ??
    getCachedAttestationPolicy("dev")?.policyId ??
    "unavailable"
  );
}

/** @deprecated Current policy is loaded asynchronously by requireTrustedPcrs. */
export function getTrustedReleaseSnapshot(
  environment?: AttestationEnvironment
): TrustedEnclaveReleaseSnapshot {
  const snapshot = environment
    ? getCachedAttestationPolicy(environment)
    : (getCachedAttestationPolicy("prod") ?? getCachedAttestationPolicy("dev"));
  if (!snapshot) throw new Error("No verified attestation TUF policy is cached in memory.");
  return snapshot;
}

function pcrBytesToHex(value: Uint8Array | undefined): string | null {
  if (!value || value.length !== 48) return null;
  return Array.from(value, (byte) => byte.toString(16).padStart(2, "0")).join("");
}

function matchedReleaseResult(
  release: TrustedEnclaveRelease,
  snapshot: TrustedEnclaveReleaseSnapshot
): PcrValidationResult {
  const { manifest, sigstore } = release;
  return {
    isMatch: true,
    text: `PCR0/PCR1/PCR2 match TUF-authorized ${manifest.environment} release ${manifest.release.version}`,
    environment: manifest.environment,
    releaseTag: `v${manifest.release.version}`,
    releaseVersion: manifest.release.version,
    sourceCommit: manifest.source.revision.digest,
    sourceRef: manifest.source.ref,
    artifactSha256: manifest.artifact.digests.sha256,
    manifestSha256: release.manifestSha256,
    bundleSha256: sigstore.bundleSha256,
    snapshotId: snapshot.policyId,
    channelSequence: snapshot.sequence,
    builderId: manifest.build.builderId,
    signerIdentityPolicy: sigstore.builder.certificateIdentityRegexp,
    oidcIssuer: sigstore.builder.certificateOidcIssuer,
    sigstoreTrustedRootSha256: sigstore.trustedRootSha256
  };
}

export function validatePcrsAgainstSnapshot(
  pcrs: ReadonlyMap<number, Uint8Array>,
  environment: AttestationEnvironment,
  snapshot: TrustedEnclaveReleaseSnapshot | undefined = getCachedAttestationPolicy(environment)
): PcrValidationResult {
  const snapshotId = snapshot?.policyId ?? "unavailable";
  if (!snapshot || snapshot.environment !== environment) {
    return {
      isMatch: false,
      text: `No verified ${environment} attestation policy is available`,
      environment,
      snapshotId
    };
  }
  const actualPcrs = {
    "0": pcrBytesToHex(pcrs.get(0)),
    "1": pcrBytesToHex(pcrs.get(1)),
    "2": pcrBytesToHex(pcrs.get(2))
  };
  if (!actualPcrs["0"] || !actualPcrs["1"] || !actualPcrs["2"]) {
    return {
      isMatch: false,
      text: "Attestation document must contain 48-byte PCR0, PCR1, and PCR2 values",
      environment,
      snapshotId
    };
  }
  const match = snapshot.releases.find((release) => {
    const expected = release.manifest.measurements.pcrs;
    return (
      release.manifest.environment === environment &&
      expected["0"] === actualPcrs["0"] &&
      expected["1"] === actualPcrs["1"] &&
      expected["2"] === actualPcrs["2"]
    );
  });
  if (!match) {
    return {
      isMatch: false,
      text: `PCR0/PCR1/PCR2 do not match one active ${environment} release`,
      environment,
      snapshotId,
      channelSequence: snapshot.sequence
    };
  }
  return matchedReleaseResult(match, snapshot);
}

export async function resolveTrustedPcrPolicy(
  environment: AttestationEnvironment
): Promise<TrustedEnclaveReleaseSnapshot> {
  return await refreshAttestationPolicy(environment);
}

export async function requireTrustedPcrsAgainstSnapshot(
  pcrs: ReadonlyMap<number, Uint8Array>,
  environment: AttestationEnvironment,
  snapshot: TrustedEnclaveReleaseSnapshot,
  now = new Date()
): Promise<PcrValidationResult> {
  if (!Number.isFinite(now.getTime())) throw new Error("The local clock is invalid.");
  for (const role of ["root", "timestamp", "snapshot", "targets"] as const) {
    const expiry = Date.parse(snapshot.expires[role]);
    if (!Number.isFinite(expiry) || expiry <= now.getTime()) {
      throw new Error(`${role} metadata is expired.`);
    }
  }
  // A different browser context may have committed a newer/revoking policy
  // while this attestation document was being verified. Recheck persistent
  // authenticated state without another network refresh before authorizing it.
  await assertAttestationPolicyCurrent(snapshot);
  const result = validatePcrsAgainstSnapshot(pcrs, environment, snapshot);
  if (!result.isMatch) throw new Error(result.text);
  return result;
}

export async function requireTrustedPcrs(
  pcrs: ReadonlyMap<number, Uint8Array>,
  environment: AttestationEnvironment
): Promise<PcrValidationResult> {
  const snapshot = await resolveTrustedPcrPolicy(environment);
  return await requireTrustedPcrsAgainstSnapshot(pcrs, environment, snapshot);
}

/** Display-only helper. PCR0 alone never authorizes key exchange. */
export async function validatePcr0Hash(
  hash: string,
  config?: PcrConfig
): Promise<Pcr0ValidationResult> {
  const environment = config?.environment;
  if (!environment) {
    return {
      isMatch: false,
      text: "An attestation environment is required; full PCR0/PCR1/PCR2 verification is required",
      snapshotId: "unavailable"
    };
  }
  const snapshot = await refreshAttestationPolicy(environment);
  const match = snapshot.releases.find(
    (release) => release.manifest.measurements.pcrs["0"] === hash
  );
  if (match) return matchedReleaseResult(match, snapshot);
  return {
    isMatch: false,
    text: "PCR0 does not match an active release; full PCR0/PCR1/PCR2 verification is required",
    environment,
    snapshotId: snapshot.policyId,
    channelSequence: snapshot.sequence
  };
}
