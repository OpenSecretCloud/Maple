import { z } from "zod";
import trustedReleaseSnapshotJson from "./trusted-enclave-releases.generated.json";

const SNAPSHOT_SCHEMA = "https://opensecret.cloud/sdk/trusted-enclave-releases/v1";
const MANIFEST_SCHEMA = "https://opensecret.cloud/attestations/nitro-eif-release/v1";
const SOURCE_REPOSITORY = "OpenSecretCloud/opensecret";
const EIF_MEDIA_TYPE = "application/vnd.aws.nitro.eif";
const PCR_HEX_PATTERN = /^[0-9a-f]{96}$/;
const SHA256_HEX_PATTERN = /^[0-9a-f]{64}$/;
const COMMIT_HEX_PATTERN = /^[0-9a-f]{40}$/;
const RELEASE_TAG_PATTERN = /^v(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/;
const WORKFLOW_PATH = ".github/workflows/release-nitro-eif.yml";
const OIDC_ISSUER = "https://token.actions.githubusercontent.com";
const LOCAL_DEVELOPMENT_API_HOSTS = new Set(["127.0.0.1", "localhost", "[::1]"]);

export const AttestationEnvironmentSchema = z.enum(["prod", "dev"]);
export type AttestationEnvironment = z.infer<typeof AttestationEnvironmentSchema>;

const PcrMeasurementsSchema = z
  .object({
    algorithm: z.literal("sha384"),
    requiredPcrs: z.tuple([z.literal(0), z.literal(1), z.literal(2)]),
    pcrs: z
      .object({
        "0": z
          .string()
          .regex(PCR_HEX_PATTERN)
          .refine((value) => !/^0+$/.test(value)),
        "1": z
          .string()
          .regex(PCR_HEX_PATTERN)
          .refine((value) => !/^0+$/.test(value)),
        "2": z
          .string()
          .regex(PCR_HEX_PATTERN)
          .refine((value) => !/^0+$/.test(value))
      })
      .strict()
  })
  .strict();

const ReleaseManifestSchema = z
  .object({
    schema: z.literal(MANIFEST_SCHEMA),
    environment: AttestationEnvironmentSchema,
    source: z
      .object({
        repository: z.literal(SOURCE_REPOSITORY),
        repositoryId: z.literal(921901924),
        ownerId: z.literal(185423582),
        ref: z.string().startsWith("refs/tags/"),
        commit: z.string().regex(COMMIT_HEX_PATTERN)
      })
      .strict(),
    release: z
      .object({
        tag: z.string().regex(RELEASE_TAG_PATTERN)
      })
      .strict(),
    artifact: z
      .object({
        name: z.string().min(1),
        mediaType: z.literal(EIF_MEDIA_TYPE),
        sha256: z.string().regex(SHA256_HEX_PATTERN),
        size: z.number().safe().int().positive()
      })
      .strict(),
    measurements: PcrMeasurementsSchema,
    build: z
      .object({
        system: z.literal("nix"),
        flakeLockSha256: z.string().regex(SHA256_HEX_PATTERN),
        derivation: z.enum(["eif-prod", "eif-dev"]),
        workflowRun: z
          .string()
          .regex(
            /^https:\/\/github\.com\/OpenSecretCloud\/opensecret\/actions\/runs\/[1-9]\d*\/attempts\/[1-9]\d*$/
          )
      })
      .strict()
  })
  .strict()
  .superRefine((manifest, context) => {
    if (manifest.source.ref !== `refs/tags/${manifest.release.tag}`) {
      context.addIssue({
        code: z.ZodIssueCode.custom,
        path: ["source", "ref"],
        message: "source ref must be the exact release tag"
      });
    }

    if (
      manifest.artifact.name !== `opensecret-${manifest.release.tag}-${manifest.environment}.eif`
    ) {
      context.addIssue({
        code: z.ZodIssueCode.custom,
        path: ["artifact", "name"],
        message: "artifact name must match the release tag and environment"
      });
    }

    if (manifest.build.derivation !== `eif-${manifest.environment}`) {
      context.addIssue({
        code: z.ZodIssueCode.custom,
        path: ["build", "derivation"],
        message: "build derivation must match the release environment"
      });
    }
  });

const TrustedReleaseSchema = z
  .object({
    manifestSha256: z.string().regex(SHA256_HEX_PATTERN),
    bundleSha256: z.string().regex(SHA256_HEX_PATTERN),
    signer: z
      .object({
        oidcIssuer: z.literal(OIDC_ISSUER),
        identity: z.string().url()
      })
      .strict(),
    transparencyLog: z
      .object({
        logIndex: z.string().regex(/^(0|[1-9]\d*)$/),
        logId: z.string().regex(SHA256_HEX_PATTERN)
      })
      .strict(),
    manifest: ReleaseManifestSchema
  })
  .strict()
  .superRefine((release, context) => {
    const expectedIdentity = `https://github.com/${SOURCE_REPOSITORY}/${WORKFLOW_PATH}@${release.manifest.source.ref}`;
    if (release.signer.identity !== expectedIdentity) {
      context.addIssue({
        code: z.ZodIssueCode.custom,
        path: ["signer", "identity"],
        message: "signer identity must match the exact release workflow and manifest tag"
      });
    }
  });

const TrustedReleaseSnapshotSchema = z
  .object({
    schema: z.literal(SNAPSHOT_SCHEMA),
    snapshotId: z.string().regex(SHA256_HEX_PATTERN),
    policy: z
      .object({
        oidcIssuer: z.literal(OIDC_ISSUER),
        sourceRepository: z.literal(SOURCE_REPOSITORY),
        sourceRepositoryId: z.literal(921901924),
        sourceRepositoryOwnerId: z.literal(185423582),
        workflow: z
          .object({
            path: z.literal(WORKFLOW_PATH),
            name: z.literal("Nitro EIF Release"),
            trigger: z.literal("workflow_dispatch"),
            environment: z.literal("production-release")
          })
          .strict()
      })
      .strict(),
    releases: z.array(TrustedReleaseSchema)
  })
  .strict()
  .superRefine((snapshot, context) => {
    const releaseKeys = new Set<string>();
    const manifestDigests = new Set<string>();
    snapshot.releases.forEach((release, index) => {
      const releaseKey = `${release.manifest.environment}:${release.manifest.release.tag}`;
      if (releaseKeys.has(releaseKey)) {
        context.addIssue({
          code: z.ZodIssueCode.custom,
          path: ["releases", index],
          message: "duplicate release environment and tag"
        });
      }
      releaseKeys.add(releaseKey);

      if (manifestDigests.has(release.manifestSha256)) {
        context.addIssue({
          code: z.ZodIssueCode.custom,
          path: ["releases", index, "manifestSha256"],
          message: "duplicate release manifest digest"
        });
      }
      manifestDigests.add(release.manifestSha256);
    });
  });

export type TrustedEnclaveRelease = z.infer<typeof TrustedReleaseSchema>;
export type TrustedEnclaveReleaseSnapshot = z.infer<typeof TrustedReleaseSnapshotSchema>;

function deepFreeze<T>(value: T): T {
  if (value !== null && typeof value === "object" && !Object.isFrozen(value)) {
    for (const nested of Object.values(value)) {
      deepFreeze(nested);
    }
    Object.freeze(value);
  }
  return value;
}

const TRUSTED_RELEASE_SNAPSHOT = deepFreeze(
  TrustedReleaseSnapshotSchema.parse(trustedReleaseSnapshotJson)
);
let snapshotIntegrityPromise: Promise<void> | undefined;

function sortJson(value: unknown): unknown {
  if (Array.isArray(value)) {
    return value.map(sortJson);
  }
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

async function sha256CanonicalJson(value: unknown): Promise<string> {
  const canonicalBytes = new TextEncoder().encode(`${JSON.stringify(sortJson(value), null, 2)}\n`);
  const digest = await crypto.subtle.digest("SHA-256", canonicalBytes);
  return Array.from(new Uint8Array(digest), (byte) => byte.toString(16).padStart(2, "0")).join("");
}

export function assertTrustedReleaseSnapshotIntegrity(): Promise<void> {
  snapshotIntegrityPromise ??= (async () => {
    const snapshotPayload = {
      schema: TRUSTED_RELEASE_SNAPSHOT.schema,
      policy: TRUSTED_RELEASE_SNAPSHOT.policy,
      releases: TRUSTED_RELEASE_SNAPSHOT.releases
    };
    const actualSnapshotId = await sha256CanonicalJson(snapshotPayload);
    if (actualSnapshotId !== TRUSTED_RELEASE_SNAPSHOT.snapshotId) {
      throw new Error("Embedded trusted-release snapshot ID is invalid.");
    }

    for (const release of TRUSTED_RELEASE_SNAPSHOT.releases) {
      const actualManifestSha256 = await sha256CanonicalJson(release.manifest);
      if (actualManifestSha256 !== release.manifestSha256) {
        throw new Error(
          `Embedded trusted-release manifest digest is invalid for ${release.manifest.release.tag}.`
        );
      }
    }
  })();
  return snapshotIntegrityPromise;
}

/**
 * Attestation policy configuration.
 *
 * Non-loopback deployments whose origin is not one of the SDK's exact official
 * origins must select an environment explicitly. Raw PCR allowlists and remote
 * PCR-history URLs are intentionally no longer supported.
 */
export type PcrConfig = {
  environment?: AttestationEnvironment;
  /** @deprecated Raw PCR overrides are no longer an authorization mechanism. */
  pcr0Values?: never;
  /** @deprecated Raw PCR overrides are no longer an authorization mechanism. */
  pcr0DevValues?: never;
  /** @deprecated Runtime PCR-history fetching has been removed. */
  remoteAttestation?: never;
  /** @deprecated Runtime PCR-history fetching has been removed. */
  remoteAttestationUrls?: never;
};

/** Return a detached, immutable copy suitable for an attestation session policy. */
export function snapshotPcrConfig(config?: PcrConfig): PcrConfig {
  return Object.freeze({ environment: config?.environment });
}

/** Canonical policy fingerprint input used to scope cached attestation sessions. */
export function serializePcrConfig(config?: PcrConfig): string {
  const snapshot = snapshotPcrConfig(config);
  return JSON.stringify({
    version: "sigstore-trusted-release-v1",
    environment: snapshot.environment ?? null,
    snapshotId: TRUSTED_RELEASE_SNAPSHOT.snapshotId
  });
}

export type Pcr0ValidationResult = {
  /** Whether PCR0, PCR1, and PCR2 match one authenticated release as a tuple. */
  isMatch: boolean;
  /** Human-readable description of the validation result. */
  text: string;
  /** Environment selected by caller policy. */
  environment?: AttestationEnvironment;
  releaseTag?: string;
  sourceCommit?: string;
  sourceRef?: string;
  artifactSha256?: string;
  manifestSha256?: string;
  bundleSha256?: string;
  snapshotId: string;
  signerIdentity?: string;
  transparencyLog?: {
    logIndex: string;
    logId: string;
  };
  /**
   * Retained for source compatibility. Sigstore v0.3 verification does not
   * expose Rekor integratedTime as a trusted timestamp.
   */
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

export function getTrustedReleaseSnapshotId(): string {
  return TRUSTED_RELEASE_SNAPSHOT.snapshotId;
}

export function getTrustedReleaseSnapshot(): TrustedEnclaveReleaseSnapshot {
  return TRUSTED_RELEASE_SNAPSHOT;
}

function pcrBytesToHex(value: Uint8Array | undefined): string | null {
  if (!value || value.length !== 48) {
    return null;
  }

  return Array.from(value, (byte) => byte.toString(16).padStart(2, "0")).join("");
}

function matchedReleaseResult(release: TrustedEnclaveRelease): PcrValidationResult {
  const { manifest } = release;
  return {
    isMatch: true,
    text: `PCR0/PCR1/PCR2 match Sigstore-verified ${manifest.environment} release ${manifest.release.tag}`,
    environment: manifest.environment,
    releaseTag: manifest.release.tag,
    sourceCommit: manifest.source.commit,
    sourceRef: manifest.source.ref,
    artifactSha256: manifest.artifact.sha256,
    manifestSha256: release.manifestSha256,
    bundleSha256: release.bundleSha256,
    snapshotId: TRUSTED_RELEASE_SNAPSHOT.snapshotId,
    signerIdentity: release.signer.identity,
    transparencyLog: release.transparencyLog
  };
}

export function validatePcrsAgainstSnapshot(
  pcrs: ReadonlyMap<number, Uint8Array>,
  environment: AttestationEnvironment,
  snapshot: TrustedEnclaveReleaseSnapshot = TRUSTED_RELEASE_SNAPSHOT
): PcrValidationResult {
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
      snapshotId: snapshot.snapshotId
    };
  }

  const matches = snapshot.releases.filter((release) => {
    const expected = release.manifest.measurements.pcrs;
    return (
      release.manifest.environment === environment &&
      expected["0"] === actualPcrs["0"] &&
      expected["1"] === actualPcrs["1"] &&
      expected["2"] === actualPcrs["2"]
    );
  });

  if (matches.length === 0) {
    return {
      isMatch: false,
      text: `PCR0/PCR1/PCR2 do not match a trusted ${environment} release`,
      environment,
      snapshotId: snapshot.snapshotId
    };
  }

  matches.sort((left, right) =>
    compareReleaseTags(right.manifest.release.tag, left.manifest.release.tag)
  );
  return {
    ...matchedReleaseResult(matches[0]),
    snapshotId: snapshot.snapshotId
  };
}

function compareReleaseTags(left: string, right: string): number {
  const leftParts = left.slice(1).split(".").map(BigInt);
  const rightParts = right.slice(1).split(".").map(BigInt);
  for (let index = 0; index < 3; index += 1) {
    if (leftParts[index] > rightParts[index]) return 1;
    if (leftParts[index] < rightParts[index]) return -1;
  }
  return 0;
}

export function requireTrustedPcrs(
  pcrs: ReadonlyMap<number, Uint8Array>,
  environment: AttestationEnvironment
): PcrValidationResult {
  const result = validatePcrsAgainstSnapshot(pcrs, environment);
  if (!result.isMatch) {
    throw new Error(result.text);
  }
  return result;
}

/**
 * Display-only compatibility helper. PCR0 by itself is never used to authorize
 * key exchange; runtime authorization calls requireTrustedPcrs with PCR0/1/2.
 */
export async function validatePcr0Hash(
  hash: string,
  config?: PcrConfig
): Promise<Pcr0ValidationResult> {
  const environment = config?.environment;
  const matches = TRUSTED_RELEASE_SNAPSHOT.releases.filter(
    (release) =>
      (!environment || release.manifest.environment === environment) &&
      release.manifest.measurements.pcrs["0"] === hash
  );

  if (matches.length > 0) {
    matches.sort((left, right) =>
      compareReleaseTags(right.manifest.release.tag, left.manifest.release.tag)
    );
    return matchedReleaseResult(matches[0]);
  }

  return {
    isMatch: false,
    text: "PCR0 does not match a trusted release; full PCR0/PCR1/PCR2 verification is required",
    environment,
    snapshotId: TRUSTED_RELEASE_SNAPSHOT.snapshotId
  };
}
