import { decode as decodeBase64, encode as encodeBase64 } from "@stablelib/base64";
import nacl from "tweetnacl";
import { z } from "zod";
import embeddedBootstrapJson from "./attestation-tuf-root.generated.json";

export const ATTESTATION_TUF_BASE_URL = "https://attestations.trymaple.ai/tuf";
const METADATA_BASE_URL = `${ATTESTATION_TUF_BASE_URL}/metadata/`;
const TARGETS_BASE_URL = `${ATTESTATION_TUF_BASE_URL}/targets/`;
const UNPUBLISHED_ROOT_SCHEMA = "https://attestations.trymaple.ai/schemas/unpublished-tuf-root/v1";
const CHANNEL_SCHEMA = "https://attestations.trymaple.ai/schemas/channel/v1";
const MANIFEST_SCHEMA = "https://attestations.trymaple.ai/schemas/nitro-eif-release/v1";
const BUILDER_POLICY_SCHEMA = "https://attestations.trymaple.ai/schemas/sigstore-builder-policy/v1";
const PCR_HEX_PATTERN = /^[0-9a-f]{96}$/;
const SHA256_HEX_PATTERN = /^[0-9a-f]{64}$/;
const ED25519_HEX_PATTERN = /^[0-9a-f]{64}$/;
const ED25519_SIGNATURE_PATTERN = /^[0-9a-f]{128}$/;
const TARGET_PATH_PATTERN = /^[A-Za-z0-9][A-Za-z0-9._/-]*$/;
const MAX_SAFE_VERSION = Number.MAX_SAFE_INTEGER;
const MAX_ROOT_ROTATIONS = 32;
const FETCH_TIMEOUT_MS = 15_000;
const MAX_TIMESTAMP_VALIDITY_MS = 48 * 60 * 60 * 1000;
const MAX_ROOT_BYTES = 64 * 1024;
const MAX_TIMESTAMP_BYTES = 32 * 1024;
const MAX_SNAPSHOT_BYTES = 128 * 1024;
const MAX_TARGETS_BYTES = 256 * 1024;
const MAX_POLICY_TARGET_BYTES = 128 * 1024;
const MAX_TRUST_ROOT_BYTES = 512 * 1024;
const MAX_MANIFEST_BYTES = 128 * 1024;
const MAX_BUNDLE_BYTES = 2 * 1024 * 1024;
const MAX_CACHE_JSON_CHARS = 8 * 1024 * 1024;
const MAX_CACHED_TARGET_BASE64_CHARS = Math.ceil((MAX_BUNDLE_BYTES * 4) / 3) + 4;
const CACHE_PREFIX = "opensecret:attestation-tuf:v4:";
const LEGACY_CACHE_PREFIX = "opensecret:attestation-tuf:v3:";
const OBSERVATION_PREFIX = `${CACHE_PREFIX}repository-observation:`;
const MAX_STORED_GENERATIONS_PER_CHANNEL = 32;
const MAX_STORED_OBSERVATIONS = 128;
const MAX_AUTHORITY_PROVENANCE_KEYS = 128;

function maximumRootVersionForBootstrap(bootstrapVersion: number): number {
  return bootstrapVersion > MAX_SAFE_VERSION - MAX_ROOT_ROTATIONS
    ? MAX_SAFE_VERSION
    : bootstrapVersion + MAX_ROOT_ROTATIONS;
}

const EnvironmentSchema = z.enum(["prod", "dev"]);
export type AttestationChannel = z.infer<typeof EnvironmentSchema>;

const PositiveVersionSchema = z.number().int().min(1).max(MAX_SAFE_VERSION);
const PositiveSequenceSchema = z.number().int().min(1).max(MAX_SAFE_VERSION);
const ExpirySchema = z.string().datetime({ offset: true });
const Sha256Schema = z.string().regex(SHA256_HEX_PATTERN);

const SignatureSchema = z
  .object({
    keyid: z.string().min(1).max(128),
    sig: z.string().regex(ED25519_SIGNATURE_PATTERN)
  })
  .strict();

const KeySchema = z
  .object({
    keytype: z.literal("ed25519"),
    scheme: z.literal("ed25519"),
    keyval: z
      .object({
        public: z.string().regex(ED25519_HEX_PATTERN)
      })
      .strict()
  })
  .strict();

const RoleSchema = z
  .object({
    keyids: z.array(z.string().min(1).max(128)).min(1).max(16),
    threshold: z.number().int().min(1).max(16)
  })
  .strict()
  .superRefine((role, context) => {
    if (new Set(role.keyids).size !== role.keyids.length) {
      context.addIssue({ code: z.ZodIssueCode.custom, message: "role key IDs must be unique" });
    }
    if (role.threshold > role.keyids.length) {
      context.addIssue({
        code: z.ZodIssueCode.custom,
        message: "role threshold exceeds its key count"
      });
    }
  });

const CommonSignedSchema = z.object({
  spec_version: z.string().regex(/^1\.0(?:\.\d+)?$/),
  version: PositiveVersionSchema,
  expires: ExpirySchema
});

const RootSignedSchema = CommonSignedSchema.extend({
  _type: z.literal("root"),
  consistent_snapshot: z.literal(true),
  keys: z.record(z.string().min(1).max(128), KeySchema),
  roles: z
    .object({
      root: RoleSchema,
      targets: RoleSchema,
      snapshot: RoleSchema,
      timestamp: RoleSchema
    })
    .strict()
})
  .strict()
  .superRefine((root, context) => {
    const keyEntries = Object.entries(root.keys);
    if (keyEntries.length === 0 || keyEntries.length > 32) {
      context.addIssue({
        code: z.ZodIssueCode.custom,
        path: ["keys"],
        message: "root must contain between 1 and 32 keys"
      });
    }
    for (const [roleName, role] of Object.entries(root.roles)) {
      for (const keyid of role.keyids) {
        if (!root.keys[keyid]) {
          context.addIssue({
            code: z.ZodIssueCode.custom,
            path: ["roles", roleName, "keyids"],
            message: `role references unknown key ${keyid}`
          });
        }
      }
    }
  });

const TargetFileSchema = z
  .object({
    length: z.number().int().min(0).max(MAX_BUNDLE_BYTES),
    hashes: z
      .object({
        sha256: Sha256Schema
      })
      .strict()
  })
  .strict();

const MetaFileSchema = z
  .object({
    version: PositiveVersionSchema,
    length: z.number().int().positive().max(MAX_TARGETS_BYTES),
    hashes: z
      .object({
        sha256: Sha256Schema
      })
      .strict()
  })
  .strict();

const TimestampSignedSchema = CommonSignedSchema.extend({
  _type: z.literal("timestamp"),
  meta: z
    .object({
      "snapshot.json": MetaFileSchema
    })
    .strict()
}).strict();

const SnapshotSignedSchema = CommonSignedSchema.extend({
  _type: z.literal("snapshot"),
  meta: z
    .object({
      "targets.json": MetaFileSchema
    })
    .strict()
}).strict();

const TargetsSignedSchema = CommonSignedSchema.extend({
  _type: z.literal("targets"),
  targets: z.record(z.string(), TargetFileSchema)
})
  .strict()
  .superRefine((targets, context) => {
    const paths = Object.keys(targets.targets);
    if (paths.length === 0 || paths.length > 256) {
      context.addIssue({
        code: z.ZodIssueCode.custom,
        path: ["targets"],
        message: "targets metadata must contain between 1 and 256 targets"
      });
    }
    for (const path of paths) {
      if (!isSafeTargetPath(path)) {
        context.addIssue({
          code: z.ZodIssueCode.custom,
          path: ["targets", path],
          message: "unsafe TUF target path"
        });
      }
    }
  });

function envelopeSchema<T extends z.ZodTypeAny>(signed: T) {
  return z
    .object({
      signatures: z.array(SignatureSchema).min(1).max(32),
      signed
    })
    .strict();
}

const RootEnvelopeSchema = envelopeSchema(RootSignedSchema);
const TimestampEnvelopeSchema = envelopeSchema(TimestampSignedSchema);
const SnapshotEnvelopeSchema = envelopeSchema(SnapshotSignedSchema);
const TargetsEnvelopeSchema = envelopeSchema(TargetsSignedSchema);

type RootEnvelope = z.infer<typeof RootEnvelopeSchema>;
type TimestampEnvelope = z.infer<typeof TimestampEnvelopeSchema>;
type SnapshotEnvelope = z.infer<typeof SnapshotEnvelopeSchema>;
type TargetsEnvelope = z.infer<typeof TargetsEnvelopeSchema>;
type RootSigned = RootEnvelope["signed"];
type TargetFile = z.infer<typeof TargetFileSchema>;

const UnpublishedRootSchema = z
  .object({
    schema: z.literal(UNPUBLISHED_ROOT_SCHEMA),
    status: z.literal("unpublished"),
    message: z.string().min(1)
  })
  .strict();

const TargetReferenceSchema = z
  .object({
    path: z.string().refine(isSafeTargetPath),
    sha256: Sha256Schema
  })
  .strict();

const ActiveReleaseSchema = z
  .object({
    manifestTarget: z.string().refine(isSafeTargetPath),
    manifestSha256: Sha256Schema,
    bundleTarget: z.string().refine(isSafeTargetPath),
    bundleSha256: Sha256Schema
  })
  .strict();

const ChannelSchema = z
  .object({
    schema: z.literal(CHANNEL_SCHEMA),
    environment: EnvironmentSchema,
    sequence: PositiveSequenceSchema,
    builderPolicyTarget: TargetReferenceSchema,
    sigstoreTrustedRootTarget: TargetReferenceSchema,
    active: z.array(ActiveReleaseSchema).max(2)
  })
  .strict()
  .superRefine((channel, context) => {
    const manifests = new Set<string>();
    const bundles = new Set<string>();
    channel.active.forEach((release, index) => {
      if (manifests.has(release.manifestTarget)) {
        context.addIssue({
          code: z.ZodIssueCode.custom,
          path: ["active", index, "manifestTarget"],
          message: "duplicate active manifest target"
        });
      }
      if (bundles.has(release.bundleTarget)) {
        context.addIssue({
          code: z.ZodIssueCode.custom,
          path: ["active", index, "bundleTarget"],
          message: "duplicate active Sigstore bundle target"
        });
      }
      manifests.add(release.manifestTarget);
      bundles.add(release.bundleTarget);
    });
  });

const ManifestSchema = z
  .object({
    schema: z.literal(MANIFEST_SCHEMA),
    component: z.literal("opensecret-backend"),
    environment: EnvironmentSchema,
    release: z
      .object({
        version: z.string().regex(/^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/)
      })
      .strict(),
    source: z
      .object({
        uri: z.string().url().refine(isExactHttpsUrl, "source URI must be an exact HTTPS URL"),
        path: z.string().min(1).max(512).refine(isSafeSourcePath, "unsafe source path"),
        ref: z.string().min(1).max(512),
        revision: z
          .object({
            algorithm: z.literal("git-sha1"),
            digest: z.string().regex(/^[0-9a-f]{40}$/)
          })
          .strict()
      })
      .strict(),
    artifact: z
      .object({
        name: z.string().min(1).max(512).refine(isSafeArtifactName, "unsafe artifact name"),
        mediaType: z.literal("application/vnd.aws.nitro.eif"),
        size: z.number().int().positive().max(Number.MAX_SAFE_INTEGER),
        digests: z
          .object({
            sha256: Sha256Schema
          })
          .strict()
      })
      .strict(),
    measurements: z
      .object({
        algorithm: z.literal("sha384"),
        requiredPcrs: z.tuple([z.literal(0), z.literal(1), z.literal(2)]),
        pcrs: z
          .object({
            "0": z.string().regex(PCR_HEX_PATTERN).refine(notAllZero),
            "1": z.string().regex(PCR_HEX_PATTERN).refine(notAllZero),
            "2": z.string().regex(PCR_HEX_PATTERN).refine(notAllZero)
          })
          .strict()
      })
      .strict(),
    build: z
      .object({
        system: z.literal("nix"),
        builderId: z.string().regex(/^[A-Za-z0-9][A-Za-z0-9._-]{0,255}$/),
        derivation: z.string().min(1).max(256),
        flakeLockSha256: Sha256Schema,
        runUri: z.string().url().refine(isExactHttpsUrl, "build run URI must be an exact HTTPS URL")
      })
      .strict()
  })
  .strict();

const BuilderIdentitySchema = z
  .object({
    certificateIdentityRegexp: z.string().min(2).max(2048),
    certificateOidcIssuer: z
      .string()
      .url()
      .refine(isExactHttpsUrl, "OIDC issuer must be an exact HTTPS URL"),
    workflowRepository: z.string().regex(/^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/),
    workflowName: z.string().min(1).max(512),
    workflowTrigger: z.string().min(1).max(128)
  })
  .strict()
  .refine(
    (builder) =>
      builder.certificateIdentityRegexp.startsWith("^") &&
      builder.certificateIdentityRegexp.endsWith("$") &&
      isValidRegexp(builder.certificateIdentityRegexp),
    "certificate identity policy must be anchored"
  );

const BuilderPolicySchema = z
  .object({
    schema: z.literal(BUILDER_POLICY_SCHEMA),
    builders: z.record(
      z.string().regex(/^[A-Za-z0-9][A-Za-z0-9._-]{0,255}$/),
      BuilderIdentitySchema
    )
  })
  .strict()
  .superRefine((policy, context) => {
    const ids = Object.keys(policy.builders);
    if (ids.length === 0 || ids.length > 32) {
      context.addIssue({
        code: z.ZodIssueCode.custom,
        path: ["builders"],
        message: "builder policy must contain between 1 and 32 builders"
      });
    }
  });

export type NitroReleaseManifest = z.infer<typeof ManifestSchema>;
export type AttestationBuilderPolicy = z.infer<typeof BuilderPolicySchema>;
export type AttestationBuilderIdentity = z.infer<typeof BuilderIdentitySchema> & { id: string };

export type SigstoreEvidence = {
  bundleTarget: string;
  bundleSha256: string;
  trustedRootTarget: string;
  trustedRootSha256: string;
  builderPolicyTarget: string;
  builderPolicySha256: string;
  builder: AttestationBuilderIdentity;
};

export type TrustedTufRelease = {
  manifestTarget: string;
  manifestSha256: string;
  manifest: NitroReleaseManifest;
  sigstore: SigstoreEvidence;
};

export type VerifiedAttestationPolicy = {
  environment: AttestationChannel;
  sequence: number;
  policyId: string;
  metadataVersions: {
    root: number;
    timestamp: number;
    snapshot: number;
    targets: number;
  };
  expires: {
    root: string;
    timestamp: string;
    snapshot: string;
    targets: string;
  };
  releases: readonly TrustedTufRelease[];
};

const RoleAuthoritySchema = z
  .object({
    threshold: z.number().int().min(1).max(16),
    keyFingerprints: z.array(Sha256Schema).min(1).max(16)
  })
  .strict()
  .superRefine((authority, context) => {
    if (authority.threshold > authority.keyFingerprints.length) {
      context.addIssue({
        code: z.ZodIssueCode.custom,
        message: "role authority threshold exceeds its key count"
      });
    }
    for (let index = 1; index < authority.keyFingerprints.length; index += 1) {
      if (authority.keyFingerprints[index - 1] >= authority.keyFingerprints[index]) {
        context.addIssue({
          code: z.ZodIssueCode.custom,
          path: ["keyFingerprints", index],
          message: "role authority key fingerprints must be unique and sorted"
        });
      }
    }
  });

const AuthorityFingerprintHistorySchema = z
  .array(Sha256Schema)
  .min(1)
  .max(MAX_AUTHORITY_PROVENANCE_KEYS)
  .superRefine((fingerprints, context) => {
    for (let index = 1; index < fingerprints.length; index += 1) {
      if (fingerprints[index - 1] >= fingerprints[index]) {
        context.addIssue({
          code: z.ZodIssueCode.custom,
          path: [index],
          message: "authority key fingerprints must be unique and sorted"
        });
      }
    }
  });

const AuthorityProvenanceSchema = z
  .object({
    keyFingerprints: AuthorityFingerprintHistorySchema
  })
  .strict();

// Authority history enforces two lifetime rules across every authenticated root:
// online-role key material is one-way retired, and offline root material never
// crosses into an online role (or vice versa).
const AuthorityHistorySchema = z
  .object({
    root: AuthorityFingerprintHistorySchema,
    timestamp: AuthorityFingerprintHistorySchema,
    snapshot: AuthorityFingerprintHistorySchema,
    targets: AuthorityFingerprintHistorySchema
  })
  .strict()
  .superRefine((history, context) => {
    const offline = new Set(history.root);
    for (const role of ["timestamp", "snapshot", "targets"] as const) {
      const index = history[role].findIndex((fingerprint) => offline.has(fingerprint));
      if (index !== -1) {
        context.addIssue({
          code: z.ZodIssueCode.custom,
          path: [role, index],
          message: "offline root and online authority history must be disjoint"
        });
      }
    }
  });

const RootHighWaterSchema = z
  .object({
    version: PositiveVersionSchema,
    sha256: Sha256Schema
  })
  .strict();

const RootHistorySchema = z
  .array(RootHighWaterSchema)
  .min(1)
  .max(256)
  .superRefine((history, context) => {
    for (let index = 1; index < history.length; index += 1) {
      if (history[index].version !== history[index - 1].version + 1) {
        context.addIssue({
          code: z.ZodIssueCode.custom,
          path: [index, "version"],
          message: "root history versions must be sequential"
        });
      }
    }
  });

const MetadataHighWaterSchema = RootHighWaterSchema.extend({
  authority: AuthorityProvenanceSchema
}).strict();

const DescriptorHighWaterSchema = RootHighWaterSchema.extend({
  parentAuthority: AuthorityProvenanceSchema,
  childAuthority: AuthorityProvenanceSchema
}).strict();

const RepositoryHighWaterSchema = z
  .object({
    root: RootHighWaterSchema,
    rootHistory: RootHistorySchema,
    authorities: z
      .object({
        root: RoleAuthoritySchema,
        timestamp: RoleAuthoritySchema,
        snapshot: RoleAuthoritySchema,
        targets: RoleAuthoritySchema
      })
      .strict(),
    authorityHistory: AuthorityHistorySchema,
    timestamp: MetadataHighWaterSchema.optional(),
    snapshotDescriptor: DescriptorHighWaterSchema.optional(),
    snapshot: MetadataHighWaterSchema.optional(),
    targetsDescriptor: DescriptorHighWaterSchema.optional(),
    targets: MetadataHighWaterSchema.optional()
  })
  .strict()
  .superRefine((repository, context) => {
    const current = repository.rootHistory[repository.rootHistory.length - 1];
    if (
      !current ||
      current.version !== repository.root.version ||
      current.sha256 !== repository.root.sha256
    ) {
      context.addIssue({
        code: z.ZodIssueCode.custom,
        path: ["rootHistory"],
        message: "root history must end at the current root high-water mark"
      });
    }
  });

const ChannelHighWaterSchema = z
  .object({
    sequence: PositiveSequenceSchema,
    policyId: Sha256Schema,
    authority: AuthorityProvenanceSchema
  })
  .strict();

const ChannelHighWatersSchema = z
  .object({
    prod: ChannelHighWaterSchema.optional(),
    dev: ChannelHighWaterSchema.optional()
  })
  .strict();

type RoleAuthority = z.infer<typeof RoleAuthoritySchema>;
type RootHighWater = z.infer<typeof RootHighWaterSchema>;
type AuthorityProvenance = z.infer<typeof AuthorityProvenanceSchema>;
type AuthorityHistory = z.infer<typeof AuthorityHistorySchema>;
type MetadataHighWater = z.infer<typeof MetadataHighWaterSchema>;
type DescriptorHighWater = z.infer<typeof DescriptorHighWaterSchema>;
type RepositoryHighWater = z.infer<typeof RepositoryHighWaterSchema>;
type ChannelHighWater = z.infer<typeof ChannelHighWaterSchema>;
type ChannelHighWaters = z.infer<typeof ChannelHighWatersSchema>;

type LegacyRawGeneration = {
  version: 2;
  trustedRootVersion: number;
  environment: AttestationChannel;
  rootChain: string[];
  timestamp: string;
  snapshot: string;
  targets: string;
  targetBytes: Record<string, string>;
};

type RawGeneration = Omit<LegacyRawGeneration, "version"> & {
  version: 4;
  repositoryHighWater: RepositoryHighWater;
  channelHighWater: ChannelHighWater;
};

const LegacyRawGenerationSchema = z
  .object({
    version: z.literal(2),
    trustedRootVersion: PositiveVersionSchema,
    environment: EnvironmentSchema,
    rootChain: z.array(z.string().min(1).max(MAX_ROOT_BYTES)).max(256),
    timestamp: z.string().min(1).max(MAX_TIMESTAMP_BYTES),
    snapshot: z.string().min(1).max(MAX_SNAPSHOT_BYTES),
    targets: z.string().min(1).max(MAX_TARGETS_BYTES),
    targetBytes: z.record(z.string(), z.string().max(MAX_CACHED_TARGET_BASE64_CHARS))
  })
  .strict()
  .superRefine((generation, context) => {
    const paths = Object.keys(generation.targetBytes);
    if (paths.length > 7 || paths.some((path) => !isSafeTargetPath(path))) {
      context.addIssue({
        code: z.ZodIssueCode.custom,
        path: ["targetBytes"],
        message: "cached target set is not bounded or contains an unsafe path"
      });
    }
  });

const RawGenerationSchema = z
  .object({
    version: z.literal(4),
    trustedRootVersion: PositiveVersionSchema,
    environment: EnvironmentSchema,
    rootChain: z.array(z.string().min(1).max(MAX_ROOT_BYTES)).max(256),
    timestamp: z.string().min(1).max(MAX_TIMESTAMP_BYTES),
    snapshot: z.string().min(1).max(MAX_SNAPSHOT_BYTES),
    targets: z.string().min(1).max(MAX_TARGETS_BYTES),
    targetBytes: z.record(z.string(), z.string().max(MAX_CACHED_TARGET_BASE64_CHARS)),
    repositoryHighWater: RepositoryHighWaterSchema,
    channelHighWater: ChannelHighWaterSchema
  })
  .strict()
  .superRefine((generation, context) => {
    const paths = Object.keys(generation.targetBytes);
    if (paths.length > 7 || paths.some((path) => !isSafeTargetPath(path))) {
      context.addIssue({
        code: z.ZodIssueCode.custom,
        path: ["targetBytes"],
        message: "cached target set is not bounded or contains an unsafe path"
      });
    }
  });

const AnyRawGenerationSchema = z.union([RawGenerationSchema, LegacyRawGenerationSchema]);

export type AttestationTufClientOptions = {
  fetch?: typeof fetch;
  storage?: Storage | null;
  now?: () => Date;
  bootstrap?: unknown;
};

type VerifiedGeneration = {
  raw: RawGeneration | LegacyRawGeneration;
  root: RootEnvelope;
  timestamp: TimestampEnvelope;
  snapshot: SnapshotEnvelope;
  targets: TargetsEnvelope;
  policy: VerifiedAttestationPolicy;
  repositoryHighWater: RepositoryHighWater;
  channelHighWater: ChannelHighWater;
};

type LegacyRawObservation = {
  version: 1;
  trustedRootVersion: number;
  rootChain: string[];
  timestamp?: string;
  snapshot?: string;
  targets?: string;
};

type RawObservation = Omit<LegacyRawObservation, "version"> & {
  version: 3;
  repositoryHighWater: RepositoryHighWater;
  channelHighWater: ChannelHighWaters;
};

type RawObservationDraft = Omit<LegacyRawObservation, "version">;

const LegacyRawObservationSchema = z
  .object({
    version: z.literal(1),
    trustedRootVersion: PositiveVersionSchema,
    rootChain: z.array(z.string().min(1).max(MAX_ROOT_BYTES)).max(256),
    timestamp: z.string().min(1).max(MAX_TIMESTAMP_BYTES).optional(),
    snapshot: z.string().min(1).max(MAX_SNAPSHOT_BYTES).optional(),
    targets: z.string().min(1).max(MAX_TARGETS_BYTES).optional()
  })
  .strict()
  .superRefine((observation, context) => {
    if (observation.snapshot && !observation.timestamp) {
      context.addIssue({ code: z.ZodIssueCode.custom, message: "snapshot requires timestamp" });
    }
    if (observation.targets && !observation.snapshot) {
      context.addIssue({ code: z.ZodIssueCode.custom, message: "targets require snapshot" });
    }
  });

const RawObservationSchema = z
  .object({
    version: z.literal(3),
    trustedRootVersion: PositiveVersionSchema,
    rootChain: z.array(z.string().min(1).max(MAX_ROOT_BYTES)).max(256),
    timestamp: z.string().min(1).max(MAX_TIMESTAMP_BYTES).optional(),
    snapshot: z.string().min(1).max(MAX_SNAPSHOT_BYTES).optional(),
    targets: z.string().min(1).max(MAX_TARGETS_BYTES).optional(),
    repositoryHighWater: RepositoryHighWaterSchema,
    channelHighWater: ChannelHighWatersSchema
  })
  .strict()
  .superRefine((observation, context) => {
    if (observation.snapshot && !observation.timestamp) {
      context.addIssue({ code: z.ZodIssueCode.custom, message: "snapshot requires timestamp" });
    }
    if (observation.targets && !observation.snapshot) {
      context.addIssue({ code: z.ZodIssueCode.custom, message: "targets require snapshot" });
    }
  });

const AnyRawObservationSchema = z.union([RawObservationSchema, LegacyRawObservationSchema]);

type VerifiedObservation = {
  raw: RawObservation | LegacyRawObservation;
  root: RootEnvelope;
  timestamp?: TimestampEnvelope;
  snapshot?: SnapshotEnvelope;
  targets?: TargetsEnvelope;
  repositoryHighWater: RepositoryHighWater;
  channelHighWater: ChannelHighWaters;
};

type StoredGeneration = {
  key: string;
  raw: unknown;
};

function sameKeys(left: readonly string[], right: readonly string[]): boolean {
  return left.length === right.length && left.every((key, index) => key === right[index]);
}

function stableStorageSnapshot(
  storage: Storage,
  prefix: string,
  maximumEntries: number,
  description: string
): Array<{ key: string; value: string }> {
  const enumerate = (): string[] => {
    const keys: string[] = [];
    for (let index = 0; index < storage.length; index += 1) {
      const key = storage.key(index);
      if (key?.startsWith(prefix)) keys.push(key);
    }
    keys.sort();
    if (keys.length > maximumEntries) {
      throw new AttestationTrustError(
        "TRUST_CACHE_INVALID",
        `${description} contains too many entries.`
      );
    }
    return keys;
  };

  for (let attempt = 0; attempt < 4; attempt += 1) {
    const before = enumerate();
    const entries: Array<{ key: string; value: string }> = [];
    let missing = false;
    for (const key of before) {
      const value = storage.getItem(key);
      if (value === null) {
        missing = true;
        break;
      }
      entries.push({ key, value });
    }
    const after = enumerate();
    if (!missing && sameKeys(before, after)) return entries;
  }
  throw new AttestationTrustError(
    "TRUST_CACHE_INVALID",
    `${description} changed repeatedly while it was being read.`
  );
}

export class AttestationTrustError extends Error {
  readonly code: string;

  constructor(code: string, message: string, options?: ErrorOptions) {
    super(message, options);
    this.name = "AttestationTrustError";
    this.code = code;
  }
}

class TrustNetworkError extends AttestationTrustError {
  constructor(message: string, options?: ErrorOptions) {
    super("TRUST_NETWORK_UNAVAILABLE", message, options);
  }
}

function notAllZero(value: string): boolean {
  return !/^0+$/.test(value);
}

function isSafeTargetPath(path: string): boolean {
  if (
    path.length === 0 ||
    path.length > 1024 ||
    !TARGET_PATH_PATTERN.test(path) ||
    path.startsWith("/") ||
    path.endsWith("/") ||
    path.includes("\\") ||
    path.includes("%") ||
    path.includes("//")
  ) {
    return false;
  }
  return path.split("/").every((part) => part !== "." && part !== "..");
}

function isExactHttpsUrl(value: string): boolean {
  try {
    const url = new URL(value);
    return (
      url.protocol === "https:" &&
      url.username === "" &&
      url.password === "" &&
      url.search === "" &&
      url.hash === ""
    );
  } catch {
    return false;
  }
}

function isSafeSourcePath(path: string): boolean {
  if (path === ".") return true;
  return (
    path.length <= 512 &&
    !path.startsWith("/") &&
    !path.endsWith("/") &&
    !path.includes("\\") &&
    !path.includes("%") &&
    !path.includes("//") &&
    path.split("/").every((part) => part !== "" && part !== "." && part !== "..")
  );
}

function isSafeArtifactName(name: string): boolean {
  return (
    name !== "." &&
    name !== ".." &&
    !name.includes("/") &&
    !name.includes("\\") &&
    !name.includes("%")
  );
}

function isValidRegexp(value: string): boolean {
  try {
    new RegExp(value);
    return true;
  } catch {
    return false;
  }
}

function fromHex(value: string): Uint8Array {
  const bytes = new Uint8Array(value.length / 2);
  for (let index = 0; index < bytes.length; index += 1) {
    bytes[index] = Number.parseInt(value.slice(index * 2, index * 2 + 2), 16);
  }
  return bytes;
}

function toHex(value: Uint8Array): string {
  return Array.from(value, (byte) => byte.toString(16).padStart(2, "0")).join("");
}

function canonicalize(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(canonicalize);
  if (value !== null && typeof value === "object") {
    const record = value as Record<string, unknown>;
    return Object.fromEntries(
      Object.keys(record)
        .sort()
        .map((key) => [key, canonicalize(record[key])])
    );
  }
  return value;
}

function deepFreeze<T>(value: T): T {
  if (value !== null && typeof value === "object" && !Object.isFrozen(value)) {
    for (const nested of Object.values(value)) deepFreeze(nested);
    Object.freeze(value);
  }
  return value;
}

export function canonicalJsonBytes(value: unknown): Uint8Array {
  return new TextEncoder().encode(JSON.stringify(canonicalize(value)));
}

async function sha256(bytes: Uint8Array): Promise<string> {
  return toHex(new Uint8Array(await crypto.subtle.digest("SHA-256", bytes)));
}

function parseJson<T>(raw: Uint8Array, schema: z.ZodType<T>, description: string): T {
  let value: unknown;
  try {
    value = JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(raw));
  } catch (error) {
    throw new AttestationTrustError("TUF_METADATA_INVALID", `${description} is not valid JSON.`, {
      cause: error
    });
  }
  const parsed = schema.safeParse(value);
  if (!parsed.success) {
    throw new AttestationTrustError(
      "TUF_METADATA_INVALID",
      `${description} does not match the supported TUF v1 profile.`,
      { cause: parsed.error }
    );
  }
  return parsed.data;
}

function parseJsonString<T>(raw: string, schema: z.ZodType<T>, description: string): T {
  return parseJson(new TextEncoder().encode(raw), schema, description);
}

function assertUnexpired(expires: string, now: Date, role: string): void {
  const expiry = Date.parse(expires);
  if (!Number.isFinite(expiry) || expiry <= now.getTime()) {
    throw new AttestationTrustError("TUF_EXPIRED", `${role} metadata is expired.`);
  }
}

async function assertRootKeyIds(root: RootSigned): Promise<void> {
  for (const [keyid, key] of Object.entries(root.keys)) {
    const actual = await sha256(canonicalJsonBytes(key));
    if (actual !== keyid) {
      throw new AttestationTrustError(
        "TUF_ROOT_CHAIN_INVALID",
        `TUF root key ID ${keyid} does not match its key material.`
      );
    }
  }
}

async function rootRoleAuthority(
  root: RootSigned,
  roleName: keyof RootSigned["roles"]
): Promise<RoleAuthority> {
  const role = root.roles[roleName];
  const keyFingerprints = await Promise.all(
    role.keyids.map(async (keyid) => {
      const key = root.keys[keyid];
      if (!key) {
        throw new AttestationTrustError(
          "TUF_ROOT_CHAIN_INVALID",
          `TUF ${roleName} role references unknown key ${keyid}.`
        );
      }
      // Authority provenance follows normalized verification-key material, not
      // TUF key IDs or the signatures a mirror happened to retain.
      return await sha256(fromHex(key.keyval.public));
    })
  );
  keyFingerprints.sort();
  if (keyFingerprints.some((fingerprint, index) => fingerprint === keyFingerprints[index - 1])) {
    throw new AttestationTrustError(
      "TUF_ROOT_CHAIN_INVALID",
      `TUF ${roleName} role authorizes duplicate aliases for the same key material.`
    );
  }
  return RoleAuthoritySchema.parse({ threshold: role.threshold, keyFingerprints });
}

async function rootRoleAuthorities(root: RootSigned): Promise<RepositoryHighWater["authorities"]> {
  const [offline, timestamp, snapshot, targets] = await Promise.all([
    rootRoleAuthority(root, "root"),
    rootRoleAuthority(root, "timestamp"),
    rootRoleAuthority(root, "snapshot"),
    rootRoleAuthority(root, "targets")
  ]);
  return { root: offline, timestamp, snapshot, targets };
}

async function assertRootAuthorities(root: RootSigned): Promise<void> {
  const authorities = await rootRoleAuthorities(root);
  const offlineKeys = new Set(authorities.root.keyFingerprints);
  for (const [role, authority] of [
    ["timestamp", authorities.timestamp],
    ["snapshot", authorities.snapshot],
    ["targets", authorities.targets]
  ] as const) {
    const overlap = authority.keyFingerprints.find((fingerprint) => offlineKeys.has(fingerprint));
    if (overlap) {
      throw new AttestationTrustError(
        "TUF_ROOT_CHAIN_INVALID",
        `TUF offline root and ${role} role reuse key material ${overlap}.`
      );
    }
  }
}

function assertThreshold(
  envelope: { signatures: Array<{ keyid: string; sig: string }>; signed: unknown },
  trustedRoot: RootSigned,
  roleName: keyof RootSigned["roles"]
): void {
  const role = trustedRoot.roles[roleName];
  const signedBytes = canonicalJsonBytes(envelope.signed);
  const verifiedKeyIds = new Set<string>();

  for (const signature of envelope.signatures) {
    if (verifiedKeyIds.has(signature.keyid) || !role.keyids.includes(signature.keyid)) continue;
    const key = trustedRoot.keys[signature.keyid];
    if (
      key &&
      nacl.sign.detached.verify(signedBytes, fromHex(signature.sig), fromHex(key.keyval.public))
    ) {
      verifiedKeyIds.add(signature.keyid);
    }
  }

  if (verifiedKeyIds.size < role.threshold) {
    throw new AttestationTrustError(
      "TUF_SIGNATURE_INVALID",
      `${roleName} metadata does not meet its trusted signature threshold.`
    );
  }
}

async function assertBytesMatch(
  bytes: Uint8Array,
  descriptor: { length: number; hashes: { sha256: string } },
  description: string
): Promise<void> {
  if (bytes.byteLength !== descriptor.length) {
    throw new AttestationTrustError(
      "TUF_TARGET_INTEGRITY",
      `${description} length does not match authenticated metadata.`
    );
  }
  if ((await sha256(bytes)) !== descriptor.hashes.sha256) {
    throw new AttestationTrustError(
      "TUF_TARGET_INTEGRITY",
      `${description} SHA-256 does not match authenticated metadata.`
    );
  }
}

function metadataUrl(name: string): URL {
  return new URL(name, METADATA_BASE_URL);
}

function targetUrl(path: string, sha256Digest: string): URL {
  if (!isSafeTargetPath(path)) {
    throw new AttestationTrustError("POLICY_INVALID", "Policy contains an unsafe target path.");
  }
  const separator = path.lastIndexOf("/");
  const directory = separator === -1 ? "" : path.slice(0, separator + 1);
  const basename = path.slice(separator + 1);
  return new URL(`${directory}${sha256Digest}.${basename}`, TARGETS_BASE_URL);
}

function readChunkWithAbort(
  reader: ReadableStreamDefaultReader<Uint8Array>,
  signal: AbortSignal,
  description: string
): Promise<ReadableStreamReadResult<Uint8Array>> {
  if (signal.aborted) return Promise.reject(new TrustNetworkError(`${description} timed out.`));
  return new Promise((resolve, reject) => {
    const onAbort = () => {
      void reader.cancel();
      reject(new TrustNetworkError(`${description} timed out.`));
    };
    signal.addEventListener("abort", onAbort, { once: true });
    reader
      .read()
      .then(resolve, (error) => {
        reject(new TrustNetworkError(`${description} response was interrupted.`, { cause: error }));
      })
      .finally(() => signal.removeEventListener("abort", onAbort));
  });
}

async function readBoundedResponse(
  response: Response,
  requestedUrl: URL,
  maxBytes: number,
  description: string,
  signal: AbortSignal
): Promise<Uint8Array> {
  if (response.redirected) {
    throw new AttestationTrustError("TRUST_REDIRECT", `${description} redirected unexpectedly.`);
  }

  if (!response.url) {
    throw new AttestationTrustError(
      "TRUST_REDIRECT",
      `${description} did not expose its final attestation repository URL.`
    );
  }

  {
    let finalUrl: URL;
    try {
      finalUrl = new URL(response.url);
    } catch (error) {
      throw new AttestationTrustError("TRUST_REDIRECT", `${description} returned an invalid URL.`, {
        cause: error
      });
    }
    if (
      finalUrl.origin !== requestedUrl.origin ||
      finalUrl.pathname !== requestedUrl.pathname ||
      finalUrl.search !== "" ||
      finalUrl.hash !== ""
    ) {
      throw new AttestationTrustError(
        "TRUST_REDIRECT",
        `${description} did not remain on its exact attestation repository URL.`
      );
    }
  }

  const contentLength = response.headers.get("content-length");
  if (contentLength !== null) {
    if (!/^(0|[1-9]\d*)$/.test(contentLength)) {
      throw new AttestationTrustError(
        "TRUST_SIZE_LIMIT",
        `${description} returned an invalid Content-Length.`
      );
    }
    const declaredLength = Number(contentLength);
    if (!Number.isSafeInteger(declaredLength) || declaredLength > maxBytes) {
      throw new AttestationTrustError(
        "TRUST_SIZE_LIMIT",
        `${description} exceeds the ${maxBytes}-byte limit.`
      );
    }
  }

  const reader = response.body?.getReader();
  if (!reader) {
    throw new AttestationTrustError(
      "TRUST_SIZE_LIMIT",
      `${description} did not provide a stream that can be bounded before allocation.`
    );
  }

  const chunks: Uint8Array[] = [];
  let total = 0;
  try {
    while (true) {
      const { done, value } = await readChunkWithAbort(reader, signal, description);
      if (done) break;
      total += value.byteLength;
      if (total > maxBytes) {
        await reader.cancel();
        throw new AttestationTrustError(
          "TRUST_SIZE_LIMIT",
          `${description} exceeds the ${maxBytes}-byte limit.`
        );
      }
      chunks.push(value);
    }
  } finally {
    reader.releaseLock();
  }

  const bytes = new Uint8Array(total);
  let offset = 0;
  for (const chunk of chunks) {
    bytes.set(chunk, offset);
    offset += chunk.byteLength;
  }
  return bytes;
}

async function fetchBytes(
  fetcher: typeof fetch,
  url: URL,
  maxBytes: number,
  description: string,
  allowNotFound = false
): Promise<Uint8Array | null> {
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), FETCH_TIMEOUT_MS);
  try {
    const response = await fetcher(url.href, {
      method: "GET",
      credentials: "omit",
      redirect: "error",
      cache: "no-store",
      referrerPolicy: "no-referrer",
      headers: { accept: "application/json" },
      signal: controller.signal
    });
    if (response.redirected) {
      throw new AttestationTrustError("TRUST_REDIRECT", `${description} redirected unexpectedly.`);
    }
    if (!response.url) {
      throw new AttestationTrustError(
        "TRUST_REDIRECT",
        `${description} did not expose its final attestation repository URL.`
      );
    }
    const finalUrl = new URL(response.url);
    if (
      finalUrl.origin !== url.origin ||
      finalUrl.pathname !== url.pathname ||
      finalUrl.search !== "" ||
      finalUrl.hash !== ""
    ) {
      throw new AttestationTrustError(
        "TRUST_REDIRECT",
        `${description} did not remain on its exact attestation repository URL.`
      );
    }
    if (allowNotFound && response.status === 404) return null;
    if (
      response.status === 404 ||
      response.status === 408 ||
      response.status === 429 ||
      response.status >= 500
    ) {
      throw new TrustNetworkError(
        `${description} is temporarily unavailable (HTTP ${response.status}).`
      );
    }
    if (!response.ok) {
      throw new AttestationTrustError(
        "TRUST_HTTP_ERROR",
        `${description} returned HTTP ${response.status}.`
      );
    }
    return await readBoundedResponse(response, url, maxBytes, description, controller.signal);
  } catch (error) {
    if (error instanceof AttestationTrustError) throw error;
    if (controller.signal.aborted) {
      throw new TrustNetworkError(`${description} timed out.`, { cause: error });
    }
    // Fetch exposes redirect:"error" and offline failures as the same TypeError.
    // Ambiguous rejections fail closed so a redirect can never select stale policy.
    throw new AttestationTrustError("TRUST_FETCH_FAILED", `${description} fetch failed.`, {
      cause: error
    });
  } finally {
    clearTimeout(timeout);
  }
}

function rootFromBootstrap(bootstrapValue: unknown): RootEnvelope {
  if (UnpublishedRootSchema.safeParse(bootstrapValue).success) {
    throw new AttestationTrustError(
      "TUF_BOOTSTRAP_INVALID",
      "The SDK attestation TUF root has not been bootstrapped for production yet."
    );
  }
  const root = RootEnvelopeSchema.safeParse(bootstrapValue);
  if (!root.success) {
    throw new AttestationTrustError(
      "TUF_BOOTSTRAP_INVALID",
      "The embedded attestation TUF root is invalid.",
      { cause: root.error }
    );
  }
  return root.data;
}

function assertOfficialEmbeddedBootstrap(bootstrapValue: unknown): void {
  if (UnpublishedRootSchema.safeParse(bootstrapValue).success) return;
  const root = rootFromBootstrap(bootstrapValue);
  if (root.signed.version !== 1) {
    throw new AttestationTrustError(
      "TUF_BOOTSTRAP_INVALID",
      "The official SDK attestation TUF bootstrap must remain root version 1."
    );
  }
}

async function verifyInitialRoot(root: RootEnvelope): Promise<void> {
  await assertRootKeyIds(root.signed);
  await assertRootAuthorities(root.signed);
  assertThreshold(root, root.signed, "root");
}

async function verifyNextRoot(previous: RootEnvelope, next: RootEnvelope): Promise<void> {
  if (next.signed.version !== previous.signed.version + 1) {
    throw new AttestationTrustError(
      "TUF_ROOT_CHAIN_INVALID",
      "TUF root versions must rotate one at a time."
    );
  }
  assertThreshold(next, previous.signed, "root");
  await assertRootKeyIds(next.signed);
  await assertRootAuthorities(next.signed);
  assertThreshold(next, next.signed, "root");
}

async function rootHighWater(root: RootEnvelope): Promise<RootHighWater> {
  return {
    version: root.signed.version,
    sha256: await sha256(canonicalJsonBytes(root.signed))
  };
}

async function restoreRootChain(
  bootstrap: RootEnvelope,
  rawRootChain: readonly string[],
  trustedRootVersion: number
): Promise<{
  root: RootEnvelope;
  rootHistory: RootHighWater[];
  authorityHistory: AuthorityHistory;
}> {
  await verifyInitialRoot(bootstrap);
  if (trustedRootVersion !== bootstrap.signed.version) {
    throw new AttestationTrustError(
      "TUF_ROOT_CHAIN_INVALID",
      "Cached trust state was created from a different embedded TUF root trust epoch."
    );
  }
  let root = bootstrap;
  const maximumRootVersion = maximumRootVersionForBootstrap(bootstrap.signed.version);
  const rootHistory = [await rootHighWater(bootstrap)];
  let authorities = await rootRoleAuthorities(bootstrap.signed);
  let authorityHistory = authorityHistoryFromAuthorities(authorities);
  for (const [index, raw] of rawRootChain.entries()) {
    const next = parseJsonString(raw, RootEnvelopeSchema, `cached root ${index + 1}`);
    if (next.signed.version < bootstrap.signed.version) continue;
    if (next.signed.version === bootstrap.signed.version) {
      if (!sameSignedPayload(next, bootstrap)) {
        throw new AttestationTrustError(
          "TUF_ROOT_CHAIN_INVALID",
          "Cached root conflicts with the embedded root at the same version."
        );
      }
      continue;
    }
    if (next.signed.version > maximumRootVersion) {
      throw new AttestationTrustError(
        "TUF_ROOT_CHAIN_INVALID",
        `TUF root rotation exceeds the ${MAX_ROOT_ROTATIONS}-version embedded-bootstrap limit.`
      );
    }
    await verifyNextRoot(root, next);
    const nextAuthorities = await rootRoleAuthorities(next.signed);
    authorityHistory = advanceAuthorityHistoryValues(
      authorities,
      authorityHistory,
      nextAuthorities,
      authorityHistoryFromAuthorities(nextAuthorities)
    );
    authorities = nextAuthorities;
    root = next;
    rootHistory.push(await rootHighWater(next));
  }
  return { root, rootHistory, authorityHistory };
}

async function refreshRootChain(
  fetcher: typeof fetch,
  initial: RootEnvelope,
  initialChain: readonly string[],
  maximumRootVersion: number,
  onRootAuthenticated?: (root: RootEnvelope, rootChain: readonly string[]) => Promise<void>
): Promise<{ root: RootEnvelope; rootChain: string[] }> {
  let root = initial;
  const rootChain = [...initialChain];
  if (root.signed.version > maximumRootVersion) {
    throw new AttestationTrustError(
      "TUF_ROOT_CHAIN_INVALID",
      `TUF root rotation exceeds the ${MAX_ROOT_ROTATIONS}-version embedded-bootstrap limit.`
    );
  }

  while (root.signed.version < maximumRootVersion) {
    const nextVersion = root.signed.version + 1;
    const bytes = await fetchBytes(
      fetcher,
      metadataUrl(`${nextVersion}.root.json`),
      MAX_ROOT_BYTES,
      `TUF root ${nextVersion}`,
      true
    );
    if (bytes === null) {
      return { root, rootChain };
    }
    const next = parseJson(bytes, RootEnvelopeSchema, `TUF root ${nextVersion}`);
    await verifyNextRoot(root, next);
    rootChain.push(new TextDecoder().decode(bytes));
    root = next;
    await onRootAuthenticated?.(root, rootChain);
  }

  // Probe one version beyond the absolute bootstrap-relative ceiling without
  // parsing, authenticating, or persisting it. A mirror cannot turn repeated
  // refreshes into an unbounded sequence of individually valid 32-root hops.
  if (maximumRootVersion === MAX_SAFE_VERSION) return { root, rootChain };
  const sentinelVersion = maximumRootVersion + 1;
  let sentinel: Uint8Array | null;
  try {
    sentinel = await fetchBytes(
      fetcher,
      metadataUrl(`${sentinelVersion}.root.json`),
      MAX_ROOT_BYTES,
      `TUF root ${sentinelVersion}`,
      true
    );
  } catch (error) {
    // At the trust-epoch ceiling, only an exact 404 proves that no forbidden
    // next root exists. Do not let transient/ambiguous probe failures select an
    // older cached policy through the ordinary network-fallback path.
    throw new AttestationTrustError(
      "TUF_ROOT_CHAIN_INVALID",
      `TUF root ${sentinelVersion} absence could not be proven at the embedded-bootstrap limit.`,
      { cause: error }
    );
  }
  if (sentinel !== null) {
    throw new AttestationTrustError(
      "TUF_ROOT_CHAIN_INVALID",
      `TUF root rotation exceeds the ${MAX_ROOT_ROTATIONS}-version embedded-bootstrap limit.`
    );
  }
  return { root, rootChain };
}

function assertMetadataVersion(
  actual: number,
  expected: number,
  role: string,
  minimum?: number
): void {
  if (actual !== expected) {
    throw new AttestationTrustError(
      "TUF_MIX_AND_MATCH",
      `${role} metadata version does not match its authenticated reference.`
    );
  }
  if (minimum !== undefined && actual < minimum) {
    throw new AttestationTrustError("TUF_ROLLBACK", `${role} metadata rolled back.`);
  }
}

function rawBytesFromCache(raw: RawGeneration | LegacyRawGeneration, path: string): Uint8Array {
  const encoded = raw.targetBytes[path];
  if (typeof encoded !== "string") {
    throw new AttestationTrustError(
      "TRUST_CACHE_INVALID",
      `The verified cache is missing target ${path}.`
    );
  }
  let bytes: Uint8Array;
  try {
    bytes = decodeBase64(encoded);
  } catch (error) {
    throw new AttestationTrustError(
      "TRUST_CACHE_INVALID",
      `Cached target ${path} is not valid base64.`,
      { cause: error }
    );
  }
  return bytes;
}

function targetDescriptor(targets: TargetsEnvelope, path: string): TargetFile {
  const descriptor = targets.signed.targets[path];
  if (!descriptor) {
    throw new AttestationTrustError(
      "POLICY_INVALID",
      `Policy references target ${path}, which targets metadata does not authorize.`
    );
  }
  return descriptor;
}

function releaseVersionFromTarget(
  path: string,
  environment: AttestationChannel,
  filename: "manifest.json" | "manifest.sigstore.json"
): string {
  const parts = path.split("/");
  if (
    parts.length !== 4 ||
    parts[0] !== "releases" ||
    parts[2] !== environment ||
    parts[3] !== filename ||
    !/^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.test(parts[1])
  ) {
    throw new AttestationTrustError(
      "POLICY_INVALID",
      `Release target ${path} does not belong to the ${environment} channel.`
    );
  }
  return parts[1];
}

function sourceUriMatchesRepository(sourceUri: string, workflowRepository: string): boolean {
  const pathname = new URL(sourceUri).pathname.replace(/^\/+|\/+$/g, "").replace(/\.git$/, "");
  return pathname === workflowRepository;
}

async function verifyCachedTarget(
  raw: RawGeneration | LegacyRawGeneration,
  targets: TargetsEnvelope,
  path: string,
  expectedSha256: string | undefined,
  maxBytes: number,
  description: string
): Promise<Uint8Array> {
  const descriptor = targetDescriptor(targets, path);
  if (descriptor.length > maxBytes) {
    throw new AttestationTrustError(
      "TRUST_SIZE_LIMIT",
      `${description} exceeds the ${maxBytes}-byte limit.`
    );
  }
  if (expectedSha256 !== undefined && descriptor.hashes.sha256 !== expectedSha256) {
    throw new AttestationTrustError(
      "TUF_TARGET_INTEGRITY",
      `${description} digest does not match its channel reference.`
    );
  }
  const bytes = rawBytesFromCache(raw, path);
  await assertBytesMatch(bytes, descriptor, description);
  return bytes;
}

function assertTargetReference(
  targets: TargetsEnvelope,
  path: string,
  expectedSha256: string,
  maxBytes: number,
  description: string
): TargetFile {
  const descriptor = targetDescriptor(targets, path);
  if (descriptor.length > maxBytes) {
    throw new AttestationTrustError(
      "TRUST_SIZE_LIMIT",
      `${description} exceeds the ${maxBytes}-byte limit.`
    );
  }
  if (descriptor.hashes.sha256 !== expectedSha256) {
    throw new AttestationTrustError(
      "TUF_TARGET_INTEGRITY",
      `${description} digest does not match its channel reference.`
    );
  }
  return descriptor;
}

async function policyFromTargets(
  raw: RawGeneration | LegacyRawGeneration,
  root: RootEnvelope,
  timestamp: TimestampEnvelope,
  snapshot: SnapshotEnvelope,
  targets: TargetsEnvelope
): Promise<VerifiedAttestationPolicy> {
  const channelPath = `channels/${raw.environment}.json`;
  const channelBytes = await verifyCachedTarget(
    raw,
    targets,
    channelPath,
    undefined,
    MAX_POLICY_TARGET_BYTES,
    `${raw.environment} channel`
  );
  const channel = parseJson(channelBytes, ChannelSchema, `${raw.environment} channel`);
  if (channel.environment !== raw.environment) {
    throw new AttestationTrustError(
      "POLICY_ENVIRONMENT_MISMATCH",
      `The ${raw.environment} channel contains ${channel.environment} policy.`
    );
  }
  if (channel.builderPolicyTarget.path !== "policy/builders.json") {
    throw new AttestationTrustError(
      "POLICY_INVALID",
      "builderPolicyTarget.path must be policy/builders.json."
    );
  }
  if (channel.sigstoreTrustedRootTarget.path !== "sigstore/trusted_root.json") {
    throw new AttestationTrustError(
      "POLICY_INVALID",
      "sigstoreTrustedRootTarget.path must be sigstore/trusted_root.json."
    );
  }

  const builderPolicyBytes = await verifyCachedTarget(
    raw,
    targets,
    channel.builderPolicyTarget.path,
    channel.builderPolicyTarget.sha256,
    MAX_POLICY_TARGET_BYTES,
    "builder policy"
  );
  const builderPolicy = parseJson(builderPolicyBytes, BuilderPolicySchema, "builder policy");
  const buildersById = new Map(
    Object.entries(builderPolicy.builders).map(([id, builder]) => [id, { id, ...builder }])
  );
  const expectedCachedTargets = new Set([channelPath, channel.builderPolicyTarget.path]);

  assertTargetReference(
    targets,
    channel.sigstoreTrustedRootTarget.path,
    channel.sigstoreTrustedRootTarget.sha256,
    MAX_TRUST_ROOT_BYTES,
    "Sigstore trusted root"
  );

  const releases: TrustedTufRelease[] = [];
  const releaseVersions = new Set<string>();
  const pcrTuples = new Set<string>();
  for (const active of channel.active) {
    expectedCachedTargets.add(active.manifestTarget);
    const releaseVersion = releaseVersionFromTarget(
      active.manifestTarget,
      raw.environment,
      "manifest.json"
    );
    const bundleVersion = releaseVersionFromTarget(
      active.bundleTarget,
      raw.environment,
      "manifest.sigstore.json"
    );
    if (releaseVersion !== bundleVersion || releaseVersions.has(releaseVersion)) {
      throw new AttestationTrustError(
        "POLICY_INVALID",
        "Active release targets identify different or duplicate releases."
      );
    }
    releaseVersions.add(releaseVersion);
    const manifestBytes = await verifyCachedTarget(
      raw,
      targets,
      active.manifestTarget,
      active.manifestSha256,
      MAX_MANIFEST_BYTES,
      "release manifest"
    );
    const manifest = parseJson(manifestBytes, ManifestSchema, "release manifest");
    if (manifest.environment !== raw.environment) {
      throw new AttestationTrustError(
        "POLICY_ENVIRONMENT_MISMATCH",
        `A ${raw.environment} channel manifest declares ${manifest.environment}.`
      );
    }
    if (
      manifest.release.version !== releaseVersion ||
      manifest.source.ref !== `refs/tags/v${releaseVersion}`
    ) {
      throw new AttestationTrustError(
        "POLICY_INVALID",
        "Release manifest version or source ref does not match its target path."
      );
    }
    const builder = buildersById.get(manifest.build.builderId);
    if (!builder) {
      throw new AttestationTrustError(
        "POLICY_INVALID",
        `Release manifest references unknown builder ${manifest.build.builderId}.`
      );
    }
    if (!sourceUriMatchesRepository(manifest.source.uri, builder.workflowRepository)) {
      throw new AttestationTrustError(
        "POLICY_INVALID",
        "Release manifest source URI does not match its authenticated builder repository."
      );
    }
    assertTargetReference(
      targets,
      active.bundleTarget,
      active.bundleSha256,
      MAX_BUNDLE_BYTES,
      "Sigstore bundle"
    );
    const tuple = [
      manifest.measurements.pcrs["0"],
      manifest.measurements.pcrs["1"],
      manifest.measurements.pcrs["2"]
    ].join(":");
    if (pcrTuples.has(tuple)) {
      throw new AttestationTrustError(
        "POLICY_INVALID",
        "Two active releases contain the same PCR0/PCR1/PCR2 tuple."
      );
    }
    pcrTuples.add(tuple);
    releases.push({
      manifestTarget: active.manifestTarget,
      manifestSha256: active.manifestSha256,
      manifest,
      sigstore: {
        bundleTarget: active.bundleTarget,
        bundleSha256: active.bundleSha256,
        trustedRootTarget: channel.sigstoreTrustedRootTarget.path,
        trustedRootSha256: channel.sigstoreTrustedRootTarget.sha256,
        builderPolicyTarget: channel.builderPolicyTarget.path,
        builderPolicySha256: channel.builderPolicyTarget.sha256,
        builder
      }
    });
  }

  const cachedPaths = Object.keys(raw.targetBytes);
  if (
    cachedPaths.length !== expectedCachedTargets.size ||
    cachedPaths.some((path) => !expectedCachedTargets.has(path))
  ) {
    throw new AttestationTrustError(
      "TRUST_CACHE_INVALID",
      "Attestation policy cache contains unexpected or missing target bodies."
    );
  }

  const policyId = await sha256(channelBytes);

  return deepFreeze({
    environment: raw.environment,
    sequence: channel.sequence,
    policyId,
    metadataVersions: {
      root: root.signed.version,
      timestamp: timestamp.signed.version,
      snapshot: snapshot.signed.version,
      targets: targets.signed.version
    },
    expires: {
      root: root.signed.expires,
      timestamp: timestamp.signed.expires,
      snapshot: snapshot.signed.expires,
      targets: targets.signed.expires
    },
    releases
  });
}

type SecurityHighWater = {
  repository: RepositoryHighWater;
  channels: ChannelHighWaters;
};

const REPOSITORY_DIRECT_FLOOR_AUTHORITIES = {
  timestamp: "timestamp",
  snapshot: "snapshot",
  targets: "targets"
} as const satisfies Record<
  "timestamp" | "snapshot" | "targets",
  keyof RepositoryHighWater["authorities"]
>;

const REPOSITORY_DESCRIPTOR_FLOOR_AUTHORITIES = {
  snapshotDescriptor: ["timestamp", "snapshot"],
  targetsDescriptor: ["snapshot", "targets"]
} as const satisfies Record<
  "snapshotDescriptor" | "targetsDescriptor",
  readonly [keyof RepositoryHighWater["authorities"], keyof RepositoryHighWater["authorities"]]
>;

function sameCanonicalValue(left: unknown, right: unknown): boolean {
  return JSON.stringify(canonicalize(left)) === JSON.stringify(canonicalize(right));
}

async function signedHighWater(
  envelope: { signed: { version: number } },
  authority: RoleAuthority
): Promise<MetadataHighWater> {
  return {
    version: envelope.signed.version,
    sha256: await sha256(canonicalJsonBytes(envelope.signed)),
    authority: provenanceFromAuthority(authority)
  };
}

async function descriptorHighWater(
  descriptor: z.infer<typeof MetaFileSchema>,
  parentAuthority: RoleAuthority,
  childAuthority: RoleAuthority
): Promise<DescriptorHighWater> {
  return {
    version: descriptor.version,
    sha256: await sha256(canonicalJsonBytes(descriptor)),
    parentAuthority: provenanceFromAuthority(parentAuthority),
    childAuthority: provenanceFromAuthority(childAuthority)
  };
}

function provenanceFromAuthority(authority: RoleAuthority): AuthorityProvenance {
  return { keyFingerprints: [...authority.keyFingerprints] };
}

function authorityHistoryFromAuthorities(
  authorities: RepositoryHighWater["authorities"]
): AuthorityHistory {
  return {
    root: [...authorities.root.keyFingerprints],
    timestamp: [...authorities.timestamp.keyFingerprints],
    snapshot: [...authorities.snapshot.keyFingerprints],
    targets: [...authorities.targets.keyFingerprints]
  };
}

function unionAuthorityFingerprints(...sets: readonly (readonly string[])[]): string[] {
  return AuthorityFingerprintHistorySchema.parse([...new Set(sets.flat())].sort());
}

function mergeAuthorityProvenance(
  left: AuthorityProvenance,
  right: AuthorityProvenance
): AuthorityProvenance {
  return {
    keyFingerprints: unionAuthorityFingerprints(left.keyFingerprints, right.keyFingerprints)
  };
}

function unionAuthorityProvenance(
  prior: AuthorityProvenance,
  candidate: RoleAuthority
): AuthorityProvenance {
  return mergeAuthorityProvenance(prior, provenanceFromAuthority(candidate));
}

function mergeAuthorityHistories(
  left: AuthorityHistory,
  right: AuthorityHistory
): AuthorityHistory {
  const merged = {
    root: unionAuthorityFingerprints(left.root, right.root),
    timestamp: unionAuthorityFingerprints(left.timestamp, right.timestamp),
    snapshot: unionAuthorityFingerprints(left.snapshot, right.snapshot),
    targets: unionAuthorityFingerprints(left.targets, right.targets)
  };
  const offline = new Set(merged.root);
  const crossed = [...merged.timestamp, ...merged.snapshot, ...merged.targets].find((fingerprint) =>
    offline.has(fingerprint)
  );
  if (crossed) {
    throw new AttestationTrustError(
      "TUF_ROLLBACK",
      `TUF key material ${crossed} crosses the offline root and online authority classes.`
    );
  }
  return merged;
}

function advanceAuthorityHistoryValues(
  priorAuthorities: RepositoryHighWater["authorities"],
  priorHistory: AuthorityHistory,
  candidateAuthorities: RepositoryHighWater["authorities"],
  candidateHistory: AuthorityHistory
): AuthorityHistory {
  const priorGlobal = new Set([
    ...priorHistory.timestamp,
    ...priorHistory.snapshot,
    ...priorHistory.targets
  ]);
  const advanced = {
    root: unionAuthorityFingerprints(priorHistory.root, candidateHistory.root)
  } as AuthorityHistory;
  for (const role of ["timestamp", "snapshot", "targets"] as const) {
    const priorCurrent = new Set(priorAuthorities[role].keyFingerprints);
    const reintroduced = candidateAuthorities[role].keyFingerprints.find(
      (fingerprint) => priorGlobal.has(fingerprint) && !priorCurrent.has(fingerprint)
    );
    if (reintroduced) {
      throw new AttestationTrustError(
        "TUF_ROLLBACK",
        `TUF ${role} authority reauthorizes retired key material ${reintroduced}.`
      );
    }
    advanced[role] = unionAuthorityFingerprints(priorHistory[role], candidateHistory[role]);
  }
  return mergeAuthorityHistories(priorHistory, advanced);
}

function advanceAuthorityHistory(
  prior: RepositoryHighWater,
  candidate: RepositoryHighWater
): AuthorityHistory {
  return advanceAuthorityHistoryValues(
    prior.authorities,
    prior.authorityHistory,
    candidate.authorities,
    candidate.authorityHistory
  );
}

async function repositoryHighWaterFromMetadata(
  root: RootEnvelope,
  rootHistory: RootHighWater[],
  authorityHistory: AuthorityHistory,
  timestamp?: TimestampEnvelope,
  snapshot?: SnapshotEnvelope,
  targets?: TargetsEnvelope
): Promise<RepositoryHighWater> {
  const authorities = await rootRoleAuthorities(root.signed);
  return {
    root: await rootHighWater(root),
    rootHistory,
    authorities,
    authorityHistory,
    ...(timestamp
      ? {
          timestamp: await signedHighWater(timestamp, authorities.timestamp),
          snapshotDescriptor: await descriptorHighWater(
            timestamp.signed.meta["snapshot.json"],
            authorities.timestamp,
            authorities.snapshot
          )
        }
      : {}),
    ...(snapshot
      ? {
          snapshot: await signedHighWater(snapshot, authorities.snapshot),
          targetsDescriptor: await descriptorHighWater(
            snapshot.signed.meta["targets.json"],
            authorities.snapshot,
            authorities.targets
          )
        }
      : {}),
    ...(targets ? { targets: await signedHighWater(targets, authorities.targets) } : {})
  };
}

function channelHighWaterFromPolicy(
  policy: VerifiedAttestationPolicy,
  authority: RoleAuthority
): ChannelHighWater {
  return {
    sequence: policy.sequence,
    policyId: policy.policyId,
    authority: provenanceFromAuthority(authority)
  };
}

function safelyReplacesAuthority(prior: AuthorityProvenance, candidate: RoleAuthority): boolean {
  const candidateKeys = new Set(candidate.keyFingerprints);
  const overlap = prior.keyFingerprints.filter((key) => candidateKeys.has(key)).length;
  return overlap < candidate.threshold;
}

function assertProvenanceCoversAuthority(
  role: string,
  provenance: AuthorityProvenance,
  authority: RoleAuthority
): void {
  const provenanceKeys = new Set(provenance.keyFingerprints);
  if (authority.keyFingerprints.some((key) => !provenanceKeys.has(key))) {
    throw new AttestationTrustError(
      "TRUST_CACHE_INVALID",
      `The persisted ${role} provenance does not cover the current authority.`
    );
  }
}

function assertHighWaterMarkMatches(
  role: string,
  stored: { version: number; sha256: string } | undefined,
  observed: { version: number; sha256: string } | undefined
): void {
  if (!observed) return;
  if (!stored || stored.version !== observed.version || stored.sha256 !== observed.sha256) {
    throw new AttestationTrustError(
      "TRUST_CACHE_INVALID",
      `The persisted ${role} high-water mark does not match its authenticated metadata.`
    );
  }
}

function assertRepositoryHighWaterBoundToMetadata(
  stored: RepositoryHighWater,
  observed: RepositoryHighWater
): void {
  if (
    !sameCanonicalValue(stored.root, observed.root) ||
    !sameCanonicalValue(stored.authorities, observed.authorities)
  ) {
    throw new AttestationTrustError(
      "TRUST_CACHE_INVALID",
      "The persisted repository high-water state does not match its authenticated TUF root."
    );
  }
  const normalizedHistory = mergeRootHistories(stored.rootHistory, observed.rootHistory);
  if (!sameCanonicalValue(normalizedHistory, stored.rootHistory)) {
    throw new AttestationTrustError(
      "TRUST_CACHE_INVALID",
      "The persisted root history omits an authenticated sequential root transition."
    );
  }
  const normalizedAuthorityHistory = mergeAuthorityHistories(
    stored.authorityHistory,
    observed.authorityHistory
  );
  if (!sameCanonicalValue(normalizedAuthorityHistory, stored.authorityHistory)) {
    throw new AttestationTrustError(
      "TRUST_CACHE_INVALID",
      "The persisted authority history omits an authenticated root transition."
    );
  }
  assertRepositoryProvenanceCoversAuthorities(stored);
  for (const floor of Object.keys(REPOSITORY_DIRECT_FLOOR_AUTHORITIES) as Array<
    keyof typeof REPOSITORY_DIRECT_FLOOR_AUTHORITIES
  >) {
    assertHighWaterMarkMatches(floor, stored[floor], observed[floor]);
  }
  for (const floor of Object.keys(REPOSITORY_DESCRIPTOR_FLOOR_AUTHORITIES) as Array<
    keyof typeof REPOSITORY_DESCRIPTOR_FLOOR_AUTHORITIES
  >) {
    assertHighWaterMarkMatches(floor, stored[floor], observed[floor]);
  }
}

function assertRepositoryProvenanceCoversAuthorities(stored: RepositoryHighWater): void {
  const offlineHistory = new Set(stored.authorityHistory.root);
  const crossed = [
    ...stored.authorityHistory.timestamp,
    ...stored.authorityHistory.snapshot,
    ...stored.authorityHistory.targets
  ].find((fingerprint) => offlineHistory.has(fingerprint));
  if (crossed) {
    throw new AttestationTrustError(
      "TRUST_CACHE_INVALID",
      `Persisted key material ${crossed} crosses the offline root and online authority classes.`
    );
  }
  for (const role of ["root", "timestamp", "snapshot", "targets"] as const) {
    const history = new Set(stored.authorityHistory[role]);
    if (stored.authorities[role].keyFingerprints.some((key) => !history.has(key))) {
      throw new AttestationTrustError(
        "TRUST_CACHE_INVALID",
        `The persisted ${role} authority history does not cover the current authority.`
      );
    }
  }
  for (const [floor, authorityName] of Object.entries(REPOSITORY_DIRECT_FLOOR_AUTHORITIES) as Array<
    [keyof typeof REPOSITORY_DIRECT_FLOOR_AUTHORITIES, keyof RepositoryHighWater["authorities"]]
  >) {
    const storedMark = stored[floor];
    if (storedMark) {
      assertProvenanceCoversAuthority(
        floor,
        storedMark.authority,
        stored.authorities[authorityName]
      );
    }
  }
  for (const [floor, [parentName, childName]] of Object.entries(
    REPOSITORY_DESCRIPTOR_FLOOR_AUTHORITIES
  ) as Array<
    [
      keyof typeof REPOSITORY_DESCRIPTOR_FLOOR_AUTHORITIES,
      readonly [keyof RepositoryHighWater["authorities"], keyof RepositoryHighWater["authorities"]]
    ]
  >) {
    const storedMark = stored[floor];
    if (storedMark) {
      assertProvenanceCoversAuthority(
        `${floor} parent`,
        storedMark.parentAuthority,
        stored.authorities[parentName]
      );
      assertProvenanceCoversAuthority(
        `${floor} child`,
        storedMark.childAuthority,
        stored.authorities[childName]
      );
    }
  }
}

function mergeMetadataHighWater(
  role: string,
  prior: MetadataHighWater | undefined,
  candidate: MetadataHighWater | undefined
): MetadataHighWater | undefined {
  if (!prior) return candidate;
  if (!candidate) return prior;
  if (candidate.version < prior.version) return prior;
  if (candidate.version === prior.version) {
    if (candidate.sha256 !== prior.sha256) {
      throw new AttestationTrustError(
        "TUF_ROLLBACK",
        `${role} metadata conflicts with an authenticated observation at the same version.`
      );
    }
    return {
      ...prior,
      authority: mergeAuthorityProvenance(prior.authority, candidate.authority)
    };
  }
  return candidate;
}

function mergeDescriptorHighWater(
  role: string,
  prior: DescriptorHighWater | undefined,
  candidate: DescriptorHighWater | undefined
): DescriptorHighWater | undefined {
  if (!prior) return candidate;
  if (!candidate) return prior;
  if (candidate.version < prior.version) return prior;
  if (candidate.version === prior.version) {
    if (candidate.sha256 !== prior.sha256) {
      throw new AttestationTrustError(
        "TUF_ROLLBACK",
        `${role} metadata pointer conflicts with an authenticated observation at the same version.`
      );
    }
    return {
      ...prior,
      parentAuthority: mergeAuthorityProvenance(prior.parentAuthority, candidate.parentAuthority),
      childAuthority: mergeAuthorityProvenance(prior.childAuthority, candidate.childAuthority)
    };
  }
  return candidate;
}

function mergeRootHistories(
  left: readonly RootHighWater[],
  right: readonly RootHighWater[]
): RootHighWater[] {
  const byVersion = new Map<number, RootHighWater>();
  for (const mark of left) byVersion.set(mark.version, mark);
  let sharesAnchor = false;
  for (const mark of right) {
    const existing = byVersion.get(mark.version);
    if (existing) {
      sharesAnchor = true;
      if (existing.sha256 !== mark.sha256) {
        throw new AttestationTrustError(
          "TUF_ROLLBACK",
          `Authenticated TUF root forks conflict at version ${mark.version}.`
        );
      }
    } else {
      byVersion.set(mark.version, mark);
    }
  }
  if (!sharesAnchor) {
    throw new AttestationTrustError(
      "TUF_ROLLBACK",
      "Authenticated TUF root histories do not share an exact trust anchor."
    );
  }
  const merged = [...byVersion.values()].sort((a, b) => a.version - b.version);
  for (let index = 1; index < merged.length; index += 1) {
    if (merged[index].version !== merged[index - 1].version + 1) {
      throw new AttestationTrustError(
        "TUF_ROLLBACK",
        "Authenticated TUF root history is not sequential."
      );
    }
  }
  return merged;
}

function mergeRepositoryHighWater(
  prior: RepositoryHighWater,
  candidate: RepositoryHighWater
): RepositoryHighWater {
  if (candidate.root.version < prior.root.version) {
    return mergeRepositoryHighWater(candidate, prior);
  }
  if (candidate.root.version === prior.root.version) {
    if (
      !sameCanonicalValue(candidate.root, prior.root) ||
      !sameCanonicalValue(candidate.authorities, prior.authorities)
    ) {
      throw new AttestationTrustError(
        "TUF_ROLLBACK",
        "Authenticated TUF root state conflicts at the same version."
      );
    }
  }

  const rootAdvanced = candidate.root.version > prior.root.version;
  const rootHistory = mergeRootHistories(prior.rootHistory, candidate.rootHistory);
  const merged: RepositoryHighWater = {
    root: rootAdvanced ? candidate.root : prior.root,
    rootHistory,
    authorities: rootAdvanced ? candidate.authorities : prior.authorities,
    authorityHistory: rootAdvanced
      ? advanceAuthorityHistory(prior, candidate)
      : mergeAuthorityHistories(prior.authorityHistory, candidate.authorityHistory)
  };
  for (const [floor, authorityName] of Object.entries(REPOSITORY_DIRECT_FLOOR_AUTHORITIES) as Array<
    [keyof typeof REPOSITORY_DIRECT_FLOOR_AUTHORITIES, keyof RepositoryHighWater["authorities"]]
  >) {
    const priorMark = prior[floor];
    const baseline = !rootAdvanced
      ? priorMark
      : priorMark &&
          !safelyReplacesAuthority(priorMark.authority, candidate.authorities[authorityName])
        ? {
            ...priorMark,
            authority: unionAuthorityProvenance(
              priorMark.authority,
              candidate.authorities[authorityName]
            )
          }
        : undefined;
    merged[floor] = mergeMetadataHighWater(floor, baseline, candidate[floor]);
  }
  for (const [floor, [parentName, childName]] of Object.entries(
    REPOSITORY_DESCRIPTOR_FLOOR_AUTHORITIES
  ) as Array<
    [
      keyof typeof REPOSITORY_DESCRIPTOR_FLOOR_AUTHORITIES,
      readonly [keyof RepositoryHighWater["authorities"], keyof RepositoryHighWater["authorities"]]
    ]
  >) {
    const priorMark = prior[floor];
    const baseline = !rootAdvanced
      ? priorMark
      : priorMark &&
          !safelyReplacesAuthority(priorMark.parentAuthority, candidate.authorities[parentName]) &&
          !safelyReplacesAuthority(priorMark.childAuthority, candidate.authorities[childName])
        ? {
            ...priorMark,
            parentAuthority: unionAuthorityProvenance(
              priorMark.parentAuthority,
              candidate.authorities[parentName]
            ),
            childAuthority: unionAuthorityProvenance(
              priorMark.childAuthority,
              candidate.authorities[childName]
            )
          }
        : undefined;
    merged[floor] = mergeDescriptorHighWater(floor, baseline, candidate[floor]);
  }
  return merged;
}

function mergeChannelHighWater(
  environment: AttestationChannel,
  prior: ChannelHighWater | undefined,
  candidate: ChannelHighWater | undefined
): ChannelHighWater | undefined {
  if (!prior) return candidate;
  if (!candidate) return prior;
  if (candidate.sequence < prior.sequence) return prior;
  if (candidate.sequence === prior.sequence) {
    if (candidate.policyId !== prior.policyId) {
      throw new AttestationTrustError(
        "TUF_ROLLBACK",
        `${environment} channel changed without incrementing sequence ${candidate.sequence}.`
      );
    }
    return {
      ...prior,
      authority: mergeAuthorityProvenance(prior.authority, candidate.authority)
    };
  }
  return candidate;
}

function mergeSecurityHighWaters(states: readonly SecurityHighWater[]): SecurityHighWater {
  if (states.length === 0) {
    throw new AttestationTrustError(
      "TRUST_CACHE_INVALID",
      "No authenticated state was available to establish a trust high-water mark."
    );
  }
  const ordered = [...states].sort(
    (left, right) => left.repository.root.version - right.repository.root.version
  );
  let merged: SecurityHighWater = {
    repository: ordered[0].repository,
    channels: { ...ordered[0].channels }
  };
  for (const candidate of ordered.slice(1)) {
    const rootAdvanced = candidate.repository.root.version > merged.repository.root.version;
    const channels: ChannelHighWaters = { ...merged.channels };
    if (rootAdvanced) {
      for (const environment of ["prod", "dev"] as const) {
        const floor = channels[environment];
        if (
          floor &&
          safelyReplacesAuthority(floor.authority, candidate.repository.authorities.targets)
        ) {
          delete channels[environment];
        } else if (floor) {
          channels[environment] = {
            ...floor,
            authority: unionAuthorityProvenance(
              floor.authority,
              candidate.repository.authorities.targets
            )
          };
        }
      }
    }
    merged = {
      repository: mergeRepositoryHighWater(merged.repository, candidate.repository),
      channels: {
        ...channels,
        ...Object.fromEntries(
          (["prod", "dev"] as const)
            .map((environment) => [
              environment,
              mergeChannelHighWater(
                environment,
                channels[environment],
                candidate.channels[environment]
              )
            ])
            .filter((entry): entry is [AttestationChannel, ChannelHighWater] => Boolean(entry[1]))
        )
      }
    };
  }
  return merged;
}

function securityHighWaterFromGeneration(generation: VerifiedGeneration): SecurityHighWater {
  return {
    repository: generation.repositoryHighWater,
    channels: { [generation.raw.environment]: generation.channelHighWater }
  };
}

function securityHighWaterFromObservation(observation: VerifiedObservation): SecurityHighWater {
  return {
    repository: observation.repositoryHighWater,
    channels: observation.channelHighWater
  };
}

async function verifyRawGeneration(
  rawValue: unknown,
  bootstrap: RootEnvelope,
  now: Date,
  enforceExpiry = true,
  allowDraft = false
): Promise<VerifiedGeneration> {
  const parsedRaw = (allowDraft ? AnyRawGenerationSchema : RawGenerationSchema).safeParse(rawValue);
  if (!parsedRaw.success) {
    throw new AttestationTrustError("TRUST_CACHE_INVALID", "Attestation policy cache is invalid.", {
      cause: parsedRaw.error
    });
  }
  const raw = parsedRaw.data;
  const { root, rootHistory, authorityHistory } = await restoreRootChain(
    bootstrap,
    raw.rootChain,
    raw.trustedRootVersion
  );
  if (enforceExpiry) assertUnexpired(root.signed.expires, now, "root");

  const timestamp = parseJsonString(raw.timestamp, TimestampEnvelopeSchema, "cached timestamp");
  assertThreshold(timestamp, root.signed, "timestamp");
  if (enforceExpiry) assertUnexpired(timestamp.signed.expires, now, "timestamp");
  const timestampExpiry = Date.parse(timestamp.signed.expires);
  if (timestampExpiry - now.getTime() > MAX_TIMESTAMP_VALIDITY_MS) {
    throw new AttestationTrustError(
      "TUF_EXPIRED",
      "Timestamp metadata validity exceeds the SDK's 48-hour freshness window."
    );
  }

  const snapshotBytes = new TextEncoder().encode(raw.snapshot);
  await assertBytesMatch(snapshotBytes, timestamp.signed.meta["snapshot.json"], "cached snapshot");
  const snapshot = parseJson(snapshotBytes, SnapshotEnvelopeSchema, "cached snapshot");
  assertMetadataVersion(
    snapshot.signed.version,
    timestamp.signed.meta["snapshot.json"].version,
    "snapshot"
  );
  assertThreshold(snapshot, root.signed, "snapshot");
  if (enforceExpiry) assertUnexpired(snapshot.signed.expires, now, "snapshot");

  const targetsBytes = new TextEncoder().encode(raw.targets);
  await assertBytesMatch(targetsBytes, snapshot.signed.meta["targets.json"], "cached targets");
  const targets = parseJson(targetsBytes, TargetsEnvelopeSchema, "cached targets");
  assertMetadataVersion(
    targets.signed.version,
    snapshot.signed.meta["targets.json"].version,
    "targets"
  );
  assertThreshold(targets, root.signed, "targets");
  if (enforceExpiry) assertUnexpired(targets.signed.expires, now, "targets");
  const policy = await policyFromTargets(raw, root, timestamp, snapshot, targets);
  const observedRepositoryHighWater = await repositoryHighWaterFromMetadata(
    root,
    rootHistory,
    authorityHistory,
    timestamp,
    snapshot,
    targets
  );
  const observedChannelHighWater = channelHighWaterFromPolicy(
    policy,
    observedRepositoryHighWater.authorities.targets
  );
  let repositoryHighWater = observedRepositoryHighWater;
  let channelHighWater = observedChannelHighWater;
  if (raw.version === 4) {
    assertRepositoryProvenanceCoversAuthorities(raw.repositoryHighWater);
    assertProvenanceCoversAuthority(
      `${raw.environment} channel`,
      raw.channelHighWater.authority,
      raw.repositoryHighWater.authorities.targets
    );
    const normalized = mergeSecurityHighWaters([
      {
        repository: raw.repositoryHighWater,
        channels: { [raw.environment]: raw.channelHighWater }
      },
      {
        repository: observedRepositoryHighWater,
        channels: { [raw.environment]: observedChannelHighWater }
      }
    ]);
    assertRepositoryHighWaterBoundToMetadata(normalized.repository, observedRepositoryHighWater);
    const normalizedChannel = normalized.channels[raw.environment];
    if (
      !normalizedChannel ||
      normalizedChannel.sequence !== observedChannelHighWater.sequence ||
      normalizedChannel.policyId !== observedChannelHighWater.policyId
    ) {
      throw new AttestationTrustError(
        "TRUST_CACHE_INVALID",
        "The persisted channel high-water mark does not match its authenticated policy."
      );
    }
    repositoryHighWater = normalized.repository;
    channelHighWater = normalizedChannel;
  }
  return {
    raw,
    root,
    timestamp,
    snapshot,
    targets,
    policy,
    repositoryHighWater,
    channelHighWater
  };
}

async function verifyRawObservation(
  rawValue: unknown,
  bootstrap: RootEnvelope,
  allowDraft = false
): Promise<VerifiedObservation> {
  const parsed = (allowDraft ? AnyRawObservationSchema : RawObservationSchema).safeParse(rawValue);
  if (!parsed.success) {
    throw new AttestationTrustError(
      "TRUST_CACHE_INVALID",
      "Authenticated repository observation cache is invalid.",
      { cause: parsed.error }
    );
  }
  const raw = parsed.data;
  const { root, rootHistory, authorityHistory } = await restoreRootChain(
    bootstrap,
    raw.rootChain,
    raw.trustedRootVersion
  );
  let timestamp: TimestampEnvelope | undefined;
  let snapshot: SnapshotEnvelope | undefined;
  let targets: TargetsEnvelope | undefined;
  if (raw.timestamp) {
    timestamp = parseJsonString(raw.timestamp, TimestampEnvelopeSchema, "observed timestamp");
    assertThreshold(timestamp, root.signed, "timestamp");
  }
  if (raw.snapshot && timestamp) {
    const bytes = new TextEncoder().encode(raw.snapshot);
    await assertBytesMatch(bytes, timestamp.signed.meta["snapshot.json"], "observed snapshot");
    snapshot = parseJson(bytes, SnapshotEnvelopeSchema, "observed snapshot");
    assertMetadataVersion(
      snapshot.signed.version,
      timestamp.signed.meta["snapshot.json"].version,
      "snapshot"
    );
    assertThreshold(snapshot, root.signed, "snapshot");
  }
  if (raw.targets && snapshot) {
    const bytes = new TextEncoder().encode(raw.targets);
    await assertBytesMatch(bytes, snapshot.signed.meta["targets.json"], "observed targets");
    targets = parseJson(bytes, TargetsEnvelopeSchema, "observed targets");
    assertMetadataVersion(
      targets.signed.version,
      snapshot.signed.meta["targets.json"].version,
      "targets"
    );
    assertThreshold(targets, root.signed, "targets");
  }
  const observedRepositoryHighWater = await repositoryHighWaterFromMetadata(
    root,
    rootHistory,
    authorityHistory,
    timestamp,
    snapshot,
    targets
  );
  let repositoryHighWater = observedRepositoryHighWater;
  let channelHighWater: ChannelHighWaters = {};
  if (raw.version === 3) {
    assertRepositoryProvenanceCoversAuthorities(raw.repositoryHighWater);
    for (const environment of ["prod", "dev"] as const) {
      const floor = raw.channelHighWater[environment];
      if (floor) {
        assertProvenanceCoversAuthority(
          `${environment} channel`,
          floor.authority,
          raw.repositoryHighWater.authorities.targets
        );
      }
    }
    const normalized = mergeSecurityHighWaters([
      {
        repository: raw.repositoryHighWater,
        channels: raw.channelHighWater
      },
      { repository: observedRepositoryHighWater, channels: {} }
    ]);
    assertRepositoryHighWaterBoundToMetadata(normalized.repository, observedRepositoryHighWater);
    repositoryHighWater = normalized.repository;
    channelHighWater = normalized.channels;
  }
  return {
    raw,
    root,
    timestamp,
    snapshot,
    targets,
    repositoryHighWater,
    channelHighWater
  };
}

async function downloadTarget(
  fetcher: typeof fetch,
  targets: TargetsEnvelope,
  path: string,
  expectedSha256: string | undefined,
  maxBytes: number,
  description: string
): Promise<Uint8Array> {
  const descriptor = targetDescriptor(targets, path);
  if (descriptor.length > maxBytes) {
    throw new AttestationTrustError(
      "TRUST_SIZE_LIMIT",
      `${description} exceeds the ${maxBytes}-byte limit.`
    );
  }
  if (expectedSha256 !== undefined && descriptor.hashes.sha256 !== expectedSha256) {
    throw new AttestationTrustError(
      "TUF_TARGET_INTEGRITY",
      `${description} digest does not match its channel reference.`
    );
  }
  const bytes = await fetchBytes(
    fetcher,
    targetUrl(path, descriptor.hashes.sha256),
    maxBytes,
    description
  );
  if (bytes === null) throw new Error("unreachable");
  await assertBytesMatch(bytes, descriptor, description);
  return bytes;
}

async function downloadPolicyTargets(
  fetcher: typeof fetch,
  environment: AttestationChannel,
  targets: TargetsEnvelope
): Promise<Record<string, string>> {
  const targetBytes: Record<string, string> = {};
  const channelPath = `channels/${environment}.json`;
  const channelBytes = await downloadTarget(
    fetcher,
    targets,
    channelPath,
    undefined,
    MAX_POLICY_TARGET_BYTES,
    `${environment} channel`
  );
  targetBytes[channelPath] = encodeBase64(channelBytes);
  const channel = parseJson(channelBytes, ChannelSchema, `${environment} channel`);
  if (channel.environment !== environment) {
    throw new AttestationTrustError(
      "POLICY_ENVIRONMENT_MISMATCH",
      `The ${environment} channel contains ${channel.environment} policy.`
    );
  }
  if (
    channel.builderPolicyTarget.path !== "policy/builders.json" ||
    channel.sigstoreTrustedRootTarget.path !== "sigstore/trusted_root.json"
  ) {
    throw new AttestationTrustError(
      "POLICY_INVALID",
      "Channel policy or Sigstore trusted-root target path is not the fixed v1 path."
    );
  }
  for (const active of channel.active) {
    const manifestVersion = releaseVersionFromTarget(
      active.manifestTarget,
      environment,
      "manifest.json"
    );
    const bundleVersion = releaseVersionFromTarget(
      active.bundleTarget,
      environment,
      "manifest.sigstore.json"
    );
    if (manifestVersion !== bundleVersion) {
      throw new AttestationTrustError(
        "POLICY_INVALID",
        "Manifest and bundle targets identify different releases."
      );
    }
  }

  const referenced: Array<[string, string, number, string, boolean]> = [
    [
      channel.builderPolicyTarget.path,
      channel.builderPolicyTarget.sha256,
      MAX_POLICY_TARGET_BYTES,
      "builder policy",
      true
    ],
    [
      channel.sigstoreTrustedRootTarget.path,
      channel.sigstoreTrustedRootTarget.sha256,
      MAX_TRUST_ROOT_BYTES,
      "Sigstore trusted root",
      false
    ]
  ];
  for (const release of channel.active) {
    referenced.push(
      [
        release.manifestTarget,
        release.manifestSha256,
        MAX_MANIFEST_BYTES,
        "release manifest",
        true
      ],
      [release.bundleTarget, release.bundleSha256, MAX_BUNDLE_BYTES, "Sigstore bundle", false]
    );
  }

  for (const [path, digest, limit, description, cacheBytes] of referenced) {
    if (targetBytes[path] !== undefined) {
      throw new AttestationTrustError("POLICY_INVALID", `Policy references target ${path} twice.`);
    }
    const bytes = await downloadTarget(fetcher, targets, path, digest, limit, description);
    if (cacheBytes) targetBytes[path] = encodeBase64(bytes);
  }
  return targetBytes;
}

function sameSignedPayload(left: { signed: unknown }, right: { signed: unknown }): boolean {
  return JSON.stringify(canonicalize(left.signed)) === JSON.stringify(canonicalize(right.signed));
}

function generationCacheTuple(generation: VerifiedGeneration): readonly number[] {
  return [
    generation.root.signed.version,
    generation.timestamp.signed.version,
    generation.snapshot.signed.version,
    generation.targets.signed.version,
    generation.policy.sequence
  ];
}

function assertEquivalentMetadata(left: VerifiedGeneration, right: VerifiedGeneration): void {
  if (
    !sameSignedPayload(left.root, right.root) ||
    !sameSignedPayload(left.timestamp, right.timestamp) ||
    !sameSignedPayload(left.snapshot, right.snapshot) ||
    !sameSignedPayload(left.targets, right.targets)
  ) {
    throw new AttestationTrustError(
      "TUF_ROLLBACK",
      "Two attestation repository generations conflict at the same metadata version."
    );
  }
}

function assertEquivalentGeneration(left: VerifiedGeneration, right: VerifiedGeneration): void {
  const channelPath = `channels/${left.raw.environment}.json`;
  assertEquivalentMetadata(left, right);
  if (left.raw.targetBytes[channelPath] !== right.raw.targetBytes[channelPath]) {
    throw new AttestationTrustError(
      "TUF_ROLLBACK",
      "Two attestation policy generations conflict at the same version."
    );
  }
}

function newestMetadataGeneration(
  left: VerifiedGeneration,
  right: VerifiedGeneration
): VerifiedGeneration {
  const repository = mergeSecurityHighWaters([
    { repository: left.repositoryHighWater, channels: {} },
    { repository: right.repositoryHighWater, channels: {} }
  ]).repository;
  const leftNewest = sameCanonicalValue(left.repositoryHighWater, repository);
  const rightNewest = sameCanonicalValue(right.repositoryHighWater, repository);
  if (leftNewest && rightNewest) {
    assertEquivalentMetadata(left, right);
    return left;
  }
  if (leftNewest) return left;
  if (rightNewest) return right;
  throw new AttestationTrustError(
    "TUF_ROLLBACK",
    "Attestation repository metadata high-water marks conflict."
  );
}

function newestGeneration(left: VerifiedGeneration, right: VerifiedGeneration): VerifiedGeneration {
  if (left.raw.environment !== right.raw.environment) {
    throw new AttestationTrustError(
      "TRUST_CACHE_INVALID",
      "Attestation policy generations from different channels cannot share a cache floor."
    );
  }
  const merged = mergeSecurityHighWaters([
    securityHighWaterFromGeneration(left),
    securityHighWaterFromGeneration(right)
  ]);
  const environment = left.raw.environment;
  const leftNewest =
    sameCanonicalValue(left.repositoryHighWater, merged.repository) &&
    sameCanonicalValue(left.channelHighWater, merged.channels[environment]);
  const rightNewest =
    sameCanonicalValue(right.repositoryHighWater, merged.repository) &&
    sameCanonicalValue(right.channelHighWater, merged.channels[environment]);
  if (leftNewest && rightNewest) {
    assertEquivalentGeneration(left, right);
    return left;
  }
  if (leftNewest) return left;
  if (rightNewest) return right;
  throw new AttestationTrustError(
    "TUF_ROLLBACK",
    "Cached attestation policy high-water marks conflict."
  );
}

function sameMetadataGeneration(left: VerifiedGeneration, right: VerifiedGeneration): boolean {
  if (!sameCanonicalValue(left.repositoryHighWater, right.repositoryHighWater)) return false;
  assertEquivalentMetadata(left, right);
  return true;
}

function assertObservationNotBehind(
  candidate: VerifiedObservation,
  observations: readonly VerifiedObservation[],
  requireComplete = false
): void {
  if (requireComplete && (!candidate.timestamp || !candidate.snapshot || !candidate.targets)) {
    throw new AttestationTrustError(
      "TUF_ROLLBACK",
      "Current trust state is not a complete authenticated repository observation."
    );
  }
  const merged = mergeSecurityHighWaters([
    ...observations.map(securityHighWaterFromObservation),
    securityHighWaterFromObservation(candidate)
  ]);
  if (!sameCanonicalValue(candidate.repositoryHighWater, merged.repository)) {
    throw new AttestationTrustError(
      "TUF_ROLLBACK",
      "Current trust state does not cover the repository high-water journal."
    );
  }
  for (const environment of ["prod", "dev"] as const) {
    if (
      (requireComplete || candidate.channelHighWater[environment]) &&
      !sameCanonicalValue(candidate.channelHighWater[environment], merged.channels[environment])
    ) {
      throw new AttestationTrustError(
        "TUF_ROLLBACK",
        `Current trust state does not cover the ${environment} channel high-water journal.`
      );
    }
  }
}

function observationDraftFromGeneration(generation: VerifiedGeneration): RawObservationDraft {
  return {
    trustedRootVersion: generation.raw.trustedRootVersion,
    rootChain: generation.raw.rootChain,
    timestamp: generation.raw.timestamp,
    snapshot: generation.raw.snapshot,
    targets: generation.raw.targets
  };
}

function decodeRawMetadata(bytes: Uint8Array): string {
  return new TextDecoder("utf-8", { fatal: true }).decode(bytes);
}

function defaultStorage(): Storage | null {
  try {
    return typeof globalThis.localStorage === "undefined" ? null : globalThis.localStorage;
  } catch {
    return null;
  }
}

type CrossContextLockManager = {
  request<T>(name: string, options: { mode: "exclusive" }, callback: () => Promise<T>): Promise<T>;
};

function browserLockManager(): CrossContextLockManager | null {
  if (typeof navigator === "undefined") return null;
  const locks = (navigator as Navigator & { locks?: CrossContextLockManager }).locks;
  return locks && typeof locks.request === "function" ? locks : null;
}

const inRealmStorageLocks = new WeakMap<Storage, Promise<void>>();

async function withInRealmStorageLock<T>(storage: Storage, action: () => Promise<T>): Promise<T> {
  const previous = inRealmStorageLocks.get(storage) ?? Promise.resolve();
  let release!: () => void;
  const gate = new Promise<void>((resolve) => {
    release = resolve;
  });
  const tail = previous.catch(() => undefined).then(() => gate);
  inRealmStorageLocks.set(storage, tail);
  await previous.catch(() => undefined);
  try {
    return await action();
  } finally {
    release();
    if (inRealmStorageLocks.get(storage) === tail) inRealmStorageLocks.delete(storage);
  }
}

function requireActivePolicy(policy: VerifiedAttestationPolicy): VerifiedAttestationPolicy {
  if (policy.releases.length === 0) {
    throw new AttestationTrustError(
      "POLICY_RELEASE_NOT_ACTIVE",
      `The authenticated ${policy.environment} channel has no active enclave release.`
    );
  }
  return policy;
}

export class AttestationTufClient {
  private readonly fetcher: typeof fetch;
  private readonly storage: Storage | null;
  private readonly now: () => Date;
  private readonly bootstrapValue: unknown;
  private readonly browserLocks: CrossContextLockManager | null;
  private readonly memory = new Map<AttestationChannel, VerifiedGeneration>();
  private readonly refreshes = new Map<AttestationChannel, Promise<VerifiedAttestationPolicy>>();
  private commitTail: Promise<void> = Promise.resolve();

  constructor(options: AttestationTufClientOptions = {}) {
    this.fetcher = options.fetch ?? globalThis.fetch.bind(globalThis);
    this.browserLocks = options.storage === undefined ? browserLockManager() : null;
    this.storage = options.storage === undefined ? defaultStorage() : options.storage;
    this.now = options.now ?? (() => new Date());
    this.bootstrapValue = options.bootstrap ?? embeddedBootstrapJson;
  }

  refresh(environment: AttestationChannel): Promise<VerifiedAttestationPolicy> {
    const parsedEnvironment = EnvironmentSchema.parse(environment);
    const pending = this.refreshes.get(parsedEnvironment);
    if (pending) return pending;

    const refresh = this.refreshOnce(parsedEnvironment).finally(() => {
      if (this.refreshes.get(parsedEnvironment) === refresh) {
        this.refreshes.delete(parsedEnvironment);
      }
    });
    this.refreshes.set(parsedEnvironment, refresh);
    return refresh;
  }

  getMemoryPolicy(environment: AttestationChannel): VerifiedAttestationPolicy | undefined {
    return this.memory.get(environment)?.policy;
  }

  /**
   * Revalidates a previously returned policy against the persistent journal
   * without performing another network refresh. This closes the interval in
   * which another browser context can commit a revocation while an attestation
   * document is being verified.
   */
  async assertPolicyCurrent(policy: VerifiedAttestationPolicy): Promise<void> {
    const environment = EnvironmentSchema.parse(policy.environment);
    const bootstrap = rootFromBootstrap(this.bootstrapValue);
    await verifyInitialRoot(bootstrap);
    if (!this.storage) {
      throw new AttestationTrustError(
        "TRUST_CACHE_INVALID",
        "Persistent browser storage is required for attestation authorization."
      );
    }
    this.assertNoLegacyCache();

    const generations = await this.readAllVerifiedGenerations(bootstrap);
    const matching = generations.filter(
      (generation) =>
        generation.raw.environment === environment && sameCanonicalValue(generation.policy, policy)
    );
    if (matching.length === 0) {
      throw new AttestationTrustError(
        "TUF_ROLLBACK",
        "The supplied attestation policy is no longer present in authenticated local state."
      );
    }
    // Cleanup is best-effort, so an older immutable generation can legitimately
    // remain beside a newer generation for the same public policy. Reconcile
    // every match and use its authenticated newest state rather than depending
    // on storage enumeration order.
    const candidate = matching.slice(1).reduce(newestGeneration, matching[0]);

    const assertCandidateCoversCurrentState = async (): Promise<void> => {
      const currentGenerations = await this.readAllVerifiedGenerations(bootstrap);
      const observations = await this.readVerifiedObservations(bootstrap);
      const merged = mergeSecurityHighWaters([
        ...currentGenerations.map(securityHighWaterFromGeneration),
        ...observations.map((entry) => securityHighWaterFromObservation(entry.verified))
      ]);
      if (
        !sameCanonicalValue(merged.repository, candidate.repositoryHighWater) ||
        !sameCanonicalValue(merged.channels[environment], candidate.channelHighWater)
      ) {
        throw new AttestationTrustError(
          "TUF_ROLLBACK",
          "The supplied attestation policy is behind authenticated persistent state."
        );
      }
    };

    await assertCandidateCoversCurrentState();
    await verifyRawGeneration(candidate.raw, bootstrap, this.currentTime(), true);
    // Verification contains asynchronous digest/signature work. Re-read after
    // it so an observation committed during that work cannot be missed.
    await assertCandidateCoversCurrentState();
  }

  private cachePrefix(environment: AttestationChannel): string {
    return `${CACHE_PREFIX}${environment}:`;
  }

  private cacheKey(generation: VerifiedGeneration): string {
    return `${this.cachePrefix(generation.raw.environment)}${generationCacheTuple(generation).join(".")}:${generation.policy.policyId}`;
  }

  private currentTime(): Date {
    const now = this.now();
    if (!Number.isFinite(now.getTime())) {
      throw new AttestationTrustError("TUF_EXPIRED", "The local clock is invalid.");
    }
    return now;
  }

  private assertNoLegacyCache(): void {
    if (!this.storage) return;
    const entries = stableStorageSnapshot(
      this.storage,
      LEGACY_CACHE_PREFIX,
      MAX_STORED_OBSERVATIONS + MAX_STORED_GENERATIONS_PER_CHANNEL * 2,
      "The legacy attestation trust cache"
    );
    if (entries.length > 0) {
      throw new AttestationTrustError(
        "TRUST_CACHE_INVALID",
        "A pre-authority-history attestation trust cache cannot be migrated safely."
      );
    }
  }

  private readStorage(
    environment: AttestationChannel,
    failOnUnavailable = false
  ): StoredGeneration[] {
    if (!this.storage) return [];
    const entries: StoredGeneration[] = [];
    try {
      for (const { key, value } of stableStorageSnapshot(
        this.storage,
        this.cachePrefix(environment),
        MAX_STORED_GENERATIONS_PER_CHANNEL,
        `The ${environment} attestation cache`
      )) {
        if (value.length > MAX_CACHE_JSON_CHARS) {
          throw new AttestationTrustError(
            "TRUST_CACHE_INVALID",
            `The ${environment} attestation policy cache exceeds its size limit.`
          );
        }
        try {
          entries.push({ key, raw: JSON.parse(value) as unknown });
        } catch (error) {
          throw new AttestationTrustError(
            "TRUST_CACHE_INVALID",
            `The ${environment} attestation policy cache is corrupt.`,
            { cause: error }
          );
        }
      }
    } catch (error) {
      if (error instanceof AttestationTrustError) throw error;
      if (failOnUnavailable) {
        throw new AttestationTrustError(
          "TRUST_CACHE_INVALID",
          "The persistent attestation trust cache could not be read safely.",
          { cause: error }
        );
      }
      return [];
    }
    return entries;
  }

  private readObservationStorage(failOnUnavailable = false): StoredGeneration[] {
    if (!this.storage) return [];
    const entries: StoredGeneration[] = [];
    try {
      for (const { key, value } of stableStorageSnapshot(
        this.storage,
        OBSERVATION_PREFIX,
        MAX_STORED_OBSERVATIONS,
        "The attestation repository observation journal"
      )) {
        if (value.length > MAX_CACHE_JSON_CHARS) {
          throw new AttestationTrustError(
            "TRUST_CACHE_INVALID",
            "An attestation repository observation exceeds its size limit."
          );
        }
        entries.push({ key, raw: JSON.parse(value) as unknown });
      }
    } catch (error) {
      if (error instanceof AttestationTrustError) throw error;
      if (failOnUnavailable) {
        throw new AttestationTrustError(
          "TRUST_CACHE_INVALID",
          "The persistent attestation observation journal could not be read safely.",
          { cause: error }
        );
      }
      return [];
    }
    return entries;
  }

  private async readVerifiedObservations(
    bootstrap: RootEnvelope
  ): Promise<Array<{ stored: StoredGeneration; verified: VerifiedObservation }>> {
    const observations: Array<{ stored: StoredGeneration; verified: VerifiedObservation }> = [];
    for (const stored of this.readObservationStorage(true)) {
      const parsed = RawObservationSchema.safeParse(stored.raw);
      if (!parsed.success) {
        throw new AttestationTrustError(
          "TRUST_CACHE_INVALID",
          "Authenticated repository observation cache is invalid.",
          { cause: parsed.error }
        );
      }
      observations.push({ stored, verified: await verifyRawObservation(stored.raw, bootstrap) });
    }
    return observations;
  }

  private async readAllVerifiedGenerations(bootstrap: RootEnvelope): Promise<VerifiedGeneration[]> {
    const verified: VerifiedGeneration[] = [];
    const now = this.currentTime();
    for (const environment of ["prod", "dev"] as const) {
      for (const stored of this.readStorage(environment, true)) {
        verified.push(await verifyRawGeneration(stored.raw, bootstrap, now, false));
      }
      const memory = this.memory.get(environment);
      if (memory) verified.push(await verifyRawGeneration(memory.raw, bootstrap, now, false));
    }
    return verified;
  }

  private async observationKey(raw: RawObservation | LegacyRawObservation): Promise<string> {
    return `${OBSERVATION_PREFIX}${await sha256(new TextEncoder().encode(JSON.stringify(raw)))}`;
  }

  private async persistObservation(
    draft: RawObservationDraft,
    bootstrap: RootEnvelope
  ): Promise<VerifiedObservation> {
    if (!this.storage) {
      throw new AttestationTrustError(
        "TRUST_CACHE_INVALID",
        "Persistent browser storage is required for attestation authorization."
      );
    }
    const observed = await verifyRawObservation({ version: 1, ...draft }, bootstrap, true);
    const before = await this.readVerifiedObservations(bootstrap);
    const generations = await this.readAllVerifiedGenerations(bootstrap);
    const merged = mergeSecurityHighWaters([
      ...generations.map(securityHighWaterFromGeneration),
      ...before.map((entry) => securityHighWaterFromObservation(entry.verified)),
      securityHighWaterFromObservation(observed)
    ]);
    if (
      merged.repository.root.version !== observed.root.signed.version ||
      !sameCanonicalValue(merged.repository.root, observed.repositoryHighWater.root)
    ) {
      throw new AttestationTrustError(
        "TUF_ROLLBACK",
        "A newer authenticated root was committed concurrently."
      );
    }
    try {
      assertRepositoryHighWaterBoundToMetadata(merged.repository, observed.repositoryHighWater);
    } catch (error) {
      if (error instanceof AttestationTrustError && error.code === "TRUST_CACHE_INVALID") {
        throw new AttestationTrustError(
          "TUF_ROLLBACK",
          "Authenticated repository metadata is behind the persisted authority-scoped floor.",
          { cause: error }
        );
      }
      throw error;
    }
    const raw: RawObservation = {
      version: 3,
      ...draft,
      repositoryHighWater: merged.repository,
      channelHighWater: merged.channels
    };
    const verified = await verifyRawObservation(raw, bootstrap);
    const serialized = JSON.stringify(raw);
    if (serialized.length > MAX_CACHE_JSON_CHARS) {
      throw new AttestationTrustError(
        "TRUST_CACHE_INVALID",
        "Authenticated repository observation is too large to persist safely."
      );
    }
    const key = await this.observationKey(raw);
    try {
      this.storage.setItem(key, serialized);
    } catch (error) {
      throw new AttestationTrustError(
        "TRUST_CACHE_INVALID",
        "Authenticated repository observation could not be persisted safely.",
        { cause: error }
      );
    }
    const after = await this.readVerifiedObservations(bootstrap);
    const afterGenerations = await this.readAllVerifiedGenerations(bootstrap);
    assertObservationNotBehind(verified, [
      ...after.map((entry) => entry.verified),
      ...afterGenerations.map((generation) => ({
        raw: {
          version: 3 as const,
          ...observationDraftFromGeneration(generation),
          repositoryHighWater: generation.repositoryHighWater,
          channelHighWater: { [generation.raw.environment]: generation.channelHighWater }
        },
        root: generation.root,
        timestamp: generation.timestamp,
        snapshot: generation.snapshot,
        targets: generation.targets,
        repositoryHighWater: generation.repositoryHighWater,
        channelHighWater: { [generation.raw.environment]: generation.channelHighWater }
      }))
    ]);
    return verified;
  }

  private async compactObservations(
    keep: VerifiedObservation,
    bootstrap: RootEnvelope
  ): Promise<void> {
    const entries = await this.readVerifiedObservations(bootstrap);
    const generations = await this.readAllVerifiedGenerations(bootstrap);
    assertObservationNotBehind(
      keep,
      [
        ...entries.map((entry) => entry.verified),
        ...generations.map((generation) => ({
          raw: {
            version: 3 as const,
            ...observationDraftFromGeneration(generation),
            repositoryHighWater: generation.repositoryHighWater,
            channelHighWater: { [generation.raw.environment]: generation.channelHighWater }
          },
          root: generation.root,
          timestamp: generation.timestamp,
          snapshot: generation.snapshot,
          targets: generation.targets,
          repositoryHighWater: generation.repositoryHighWater,
          channelHighWater: { [generation.raw.environment]: generation.channelHighWater }
        }))
      ],
      true
    );
    const keepKey = await this.observationKey(keep.raw);
    for (const entry of entries) {
      if (entry.stored.key === keepKey) continue;
      try {
        this.storage?.removeItem(entry.stored.key);
      } catch {
        // Older immutable observations are safe to retain.
      }
    }
  }

  private async readVerifiedCache(
    environment: AttestationChannel,
    bootstrap: RootEnvelope,
    now: Date
  ): Promise<{
    channel: VerifiedGeneration | undefined;
    global: VerifiedGeneration | undefined;
    usable: boolean;
  }> {
    const verifiedByChannel = new Map<AttestationChannel, VerifiedGeneration>();
    for (const channel of ["prod", "dev"] as const) {
      let storedGeneration: VerifiedGeneration | undefined;
      for (const stored of this.readStorage(channel, true)) {
        const parsed = RawGenerationSchema.safeParse(stored.raw);
        if (!parsed.success) {
          throw new AttestationTrustError(
            "TRUST_CACHE_INVALID",
            "Attestation policy cache is invalid.",
            { cause: parsed.error }
          );
        }
        const verified = await verifyRawGeneration(stored.raw, bootstrap, now, false);
        storedGeneration = storedGeneration
          ? newestGeneration(storedGeneration, verified)
          : verified;
      }
      const memory = this.memory.get(channel);
      const memoryGeneration = memory
        ? await verifyRawGeneration(memory.raw, bootstrap, now, false)
        : undefined;
      const verified =
        storedGeneration && memoryGeneration
          ? newestGeneration(storedGeneration, memoryGeneration)
          : (storedGeneration ?? memoryGeneration);
      if (!verified) continue;
      if (verified.raw.environment !== channel) {
        throw new AttestationTrustError(
          "POLICY_ENVIRONMENT_MISMATCH",
          "Cached attestation policy belongs to another channel."
        );
      }
      this.memory.set(channel, verified);
      verifiedByChannel.set(channel, verified);
    }

    const channel = verifiedByChannel.get(environment);
    let global: VerifiedGeneration | undefined;
    for (const generation of verifiedByChannel.values()) {
      global = global ? newestMetadataGeneration(global, generation) : generation;
    }
    const usable = Boolean(
      channel &&
      global &&
      sameMetadataGeneration(channel, global) &&
      (await this.isUsableCache(channel.raw, bootstrap, now))
    );
    return {
      channel,
      global,
      usable
    };
  }

  private async isUsableCache(
    raw: RawGeneration | LegacyRawGeneration,
    bootstrap: RootEnvelope,
    now: Date
  ): Promise<boolean> {
    try {
      await verifyRawGeneration(raw, bootstrap, now, true);
      return true;
    } catch (error) {
      if (error instanceof AttestationTrustError && error.code === "TUF_EXPIRED") return false;
      throw error;
    }
  }

  private async finalizeGeneration(
    generation: VerifiedGeneration,
    bootstrap: RootEnvelope,
    now: Date
  ): Promise<VerifiedGeneration> {
    const observations = await this.readVerifiedObservations(bootstrap);
    const generations = await this.readAllVerifiedGenerations(bootstrap);
    const merged = mergeSecurityHighWaters([
      ...generations.map(securityHighWaterFromGeneration),
      ...observations.map((entry) => securityHighWaterFromObservation(entry.verified)),
      securityHighWaterFromGeneration(generation)
    ]);
    const environment = generation.raw.environment;
    try {
      assertRepositoryHighWaterBoundToMetadata(merged.repository, generation.repositoryHighWater);
    } catch (error) {
      throw new AttestationTrustError(
        "TUF_ROLLBACK",
        "The candidate policy is behind the authenticated repository floors.",
        { cause: error }
      );
    }
    const mergedChannel = merged.channels[environment];
    if (
      !mergedChannel ||
      mergedChannel.sequence !== generation.channelHighWater.sequence ||
      mergedChannel.policyId !== generation.channelHighWater.policyId
    ) {
      throw new AttestationTrustError(
        "TUF_ROLLBACK",
        "The candidate policy is behind the authenticated channel floor."
      );
    }
    const raw: RawGeneration = {
      version: 4,
      trustedRootVersion: generation.raw.trustedRootVersion,
      environment,
      rootChain: generation.raw.rootChain,
      timestamp: generation.raw.timestamp,
      snapshot: generation.raw.snapshot,
      targets: generation.raw.targets,
      targetBytes: generation.raw.targetBytes,
      repositoryHighWater: merged.repository,
      channelHighWater: mergedChannel
    };
    return await verifyRawGeneration(raw, bootstrap, now, true);
  }

  private async commit(
    generation: VerifiedGeneration,
    bootstrap: RootEnvelope,
    now: Date
  ): Promise<void> {
    const serialized = JSON.stringify(generation.raw);
    if (serialized.length > MAX_CACHE_JSON_CHARS) {
      throw new AttestationTrustError(
        "TRUST_CACHE_INVALID",
        "The verified attestation policy is too large to persist safely."
      );
    }

    const previous = this.commitTail;
    let release!: () => void;
    const gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    this.commitTail = previous.catch(() => undefined).then(() => gate);
    await previous.catch(() => undefined);

    const commitLocked = async (): Promise<void> => {
      const existing: VerifiedGeneration[] = [];
      const storedForEnvironment: StoredGeneration[] = [];
      for (const channel of ["prod", "dev"] as const) {
        const memory = this.memory.get(channel);
        if (memory) existing.push(await verifyRawGeneration(memory.raw, bootstrap, now, false));
        for (const stored of this.readStorage(channel, true)) {
          existing.push(await verifyRawGeneration(stored.raw, bootstrap, now, false));
          if (channel === generation.raw.environment) storedForEnvironment.push(stored);
        }
      }

      const assertCandidateIsNewest = (current: VerifiedGeneration): void => {
        const newest =
          current.raw.environment === generation.raw.environment
            ? newestGeneration(generation, current)
            : newestMetadataGeneration(generation, current);
        if (newest !== generation) {
          throw new AttestationTrustError(
            "TUF_ROLLBACK",
            "A newer attestation repository generation was committed concurrently."
          );
        }
      };
      for (const current of existing) assertCandidateIsNewest(current);

      const assertCandidateCoversJournal = async (): Promise<void> => {
        const observations = await this.readVerifiedObservations(bootstrap);
        const merged = mergeSecurityHighWaters([
          ...existing.map(securityHighWaterFromGeneration),
          ...observations.map((entry) => securityHighWaterFromObservation(entry.verified)),
          securityHighWaterFromGeneration(generation)
        ]);
        if (
          !sameCanonicalValue(merged.repository, generation.repositoryHighWater) ||
          !sameCanonicalValue(
            merged.channels[generation.raw.environment],
            generation.channelHighWater
          )
        ) {
          throw new AttestationTrustError(
            "TUF_ROLLBACK",
            "A newer repository or channel floor was observed concurrently."
          );
        }
      };
      await assertCandidateCoversJournal();

      await verifyRawGeneration(generation.raw, bootstrap, this.currentTime(), true);

      if (this.storage) {
        const key = this.cacheKey(generation);
        try {
          // Generation keys are immutable. A stale tab can add an older key but
          // can never overwrite or erase a newer generation it did not observe.
          this.storage.setItem(key, serialized);
        } catch (error) {
          throw new AttestationTrustError(
            "TRUST_CACHE_INVALID",
            "The verified attestation policy could not be persisted safely.",
            { cause: error }
          );
        }

        // Web Locks are not available in every browser. Re-read after the
        // immutable write so a newer generation committed during asynchronous
        // verification prevents this stale refresh from authorizing a session.
        for (const channel of ["prod", "dev"] as const) {
          for (const stored of this.readStorage(channel, true)) {
            assertCandidateIsNewest(
              await verifyRawGeneration(stored.raw, bootstrap, this.currentTime(), false)
            );
          }
        }
        await assertCandidateCoversJournal();
        for (const stored of storedForEnvironment) {
          if (stored.key === key) continue;
          try {
            this.storage.removeItem(stored.key);
          } catch {
            // Leaving an older immutable entry is safe; readers select the newest.
          }
        }
      }
    };

    try {
      if (this.storage && this.browserLocks) {
        await this.browserLocks.request(
          "opensecret:attestation-tuf:repository",
          { mode: "exclusive" },
          commitLocked
        );
      } else if (this.storage) {
        await withInRealmStorageLock(this.storage, commitLocked);
      } else {
        await commitLocked();
      }
    } finally {
      release();
    }
  }

  private async refreshOnce(environment: AttestationChannel): Promise<VerifiedAttestationPolicy> {
    const now = this.currentTime();
    const bootstrap = rootFromBootstrap(this.bootstrapValue);
    await verifyInitialRoot(bootstrap);
    if (!this.storage) {
      throw new AttestationTrustError(
        "TRUST_CACHE_INVALID",
        "Persistent browser storage is required for attestation authorization."
      );
    }
    this.assertNoLegacyCache();
    const cachedState = await this.readVerifiedCache(environment, bootstrap, now);
    const cached = cachedState.channel;
    const globalCached = cachedState.global;
    const storedObservations = await this.readVerifiedObservations(bootstrap);
    const storedGenerations = await this.readAllVerifiedGenerations(bootstrap);

    try {
      let startingRoot = globalCached?.root ?? bootstrap;
      let startingChain = globalCached?.raw.rootChain ?? [];
      const startingPoints = [
        ...storedGenerations.map((generation) => ({
          root: generation.root,
          rootChain: generation.raw.rootChain,
          security: securityHighWaterFromGeneration(generation)
        })),
        ...storedObservations.map(({ verified }) => ({
          root: verified.root,
          rootChain: verified.raw.rootChain,
          security: securityHighWaterFromObservation(verified)
        }))
      ];
      if (startingPoints.length > 0) {
        // Validate the complete authenticated journal before selecting a root
        // or making a request. In particular, an equal-version root fork must
        // not be hidden by a colliding generation-cache key.
        const startupFloor = mergeSecurityHighWaters(startingPoints.map((point) => point.security));
        const matching = startingPoints
          .filter((point) =>
            sameCanonicalValue(point.security.repository.root, startupFloor.repository.root)
          )
          .sort((left, right) => right.rootChain.length - left.rootChain.length)[0];
        if (!matching) {
          throw new AttestationTrustError(
            "TRUST_CACHE_INVALID",
            "Authenticated local state does not contain its root high-water envelope."
          );
        }
        startingRoot = matching.root;
        startingChain = matching.rootChain;
      }
      let onlineRaw: RawObservationDraft = {
        trustedRootVersion: bootstrap.signed.version,
        rootChain: [...startingChain]
      };
      const { root, rootChain } = await refreshRootChain(
        this.fetcher,
        startingRoot,
        startingChain,
        maximumRootVersionForBootstrap(bootstrap.signed.version),
        async (_authenticatedRoot, authenticatedChain) => {
          onlineRaw = { ...onlineRaw, rootChain: [...authenticatedChain] };
          await this.persistObservation(onlineRaw, bootstrap);
        }
      );
      assertUnexpired(root.signed.expires, now, "root");
      onlineRaw = { ...onlineRaw, rootChain };
      await this.persistObservation(onlineRaw, bootstrap);

      const timestampBytes = await fetchBytes(
        this.fetcher,
        metadataUrl("timestamp.json"),
        MAX_TIMESTAMP_BYTES,
        "TUF timestamp"
      );
      if (timestampBytes === null) throw new Error("unreachable");
      const timestamp = parseJson(timestampBytes, TimestampEnvelopeSchema, "TUF timestamp");
      assertThreshold(timestamp, root.signed, "timestamp");
      assertUnexpired(timestamp.signed.expires, now, "timestamp");
      if (Date.parse(timestamp.signed.expires) - now.getTime() > MAX_TIMESTAMP_VALIDITY_MS) {
        throw new AttestationTrustError(
          "TUF_EXPIRED",
          "Timestamp metadata validity exceeds the SDK's 48-hour freshness window."
        );
      }
      onlineRaw = { ...onlineRaw, timestamp: decodeRawMetadata(timestampBytes) };
      await this.persistObservation(onlineRaw, bootstrap);
      if (
        cached &&
        cachedState.usable &&
        globalCached &&
        sameMetadataGeneration(cached, globalCached) &&
        root.signed.version === globalCached.root.signed.version &&
        timestamp.signed.version === globalCached.timestamp.signed.version &&
        sameSignedPayload(timestamp, globalCached.timestamp) &&
        (await this.isUsableCache(cached.raw, bootstrap, this.currentTime()))
      ) {
        const complete = await this.persistObservation(
          observationDraftFromGeneration(cached),
          bootstrap
        );
        await this.compactObservations(complete, bootstrap);
        return requireActivePolicy(cached.policy);
      }

      const snapshotDescriptor = timestamp.signed.meta["snapshot.json"];
      const snapshotBytes = await fetchBytes(
        this.fetcher,
        metadataUrl(`${snapshotDescriptor.version}.snapshot.json`),
        MAX_SNAPSHOT_BYTES,
        "TUF snapshot"
      );
      if (snapshotBytes === null) throw new Error("unreachable");
      await assertBytesMatch(snapshotBytes, snapshotDescriptor, "TUF snapshot");
      const snapshot = parseJson(snapshotBytes, SnapshotEnvelopeSchema, "TUF snapshot");
      assertMetadataVersion(snapshot.signed.version, snapshotDescriptor.version, "snapshot");
      assertThreshold(snapshot, root.signed, "snapshot");
      assertUnexpired(snapshot.signed.expires, now, "snapshot");
      onlineRaw = { ...onlineRaw, snapshot: decodeRawMetadata(snapshotBytes) };
      await this.persistObservation(onlineRaw, bootstrap);

      const targetsDescriptor = snapshot.signed.meta["targets.json"];
      const targetsBytes = await fetchBytes(
        this.fetcher,
        metadataUrl(`${targetsDescriptor.version}.targets.json`),
        MAX_TARGETS_BYTES,
        "TUF targets"
      );
      if (targetsBytes === null) throw new Error("unreachable");
      await assertBytesMatch(targetsBytes, targetsDescriptor, "TUF targets");
      const targets = parseJson(targetsBytes, TargetsEnvelopeSchema, "TUF targets");
      assertMetadataVersion(targets.signed.version, targetsDescriptor.version, "targets");
      assertThreshold(targets, root.signed, "targets");
      assertUnexpired(targets.signed.expires, now, "targets");
      onlineRaw = { ...onlineRaw, targets: decodeRawMetadata(targetsBytes) };
      await this.persistObservation(onlineRaw, bootstrap);

      const targetBytes = await downloadPolicyTargets(this.fetcher, environment, targets);
      const raw: LegacyRawGeneration = {
        version: 2,
        trustedRootVersion: bootstrap.signed.version,
        environment,
        rootChain,
        timestamp: decodeRawMetadata(timestampBytes),
        snapshot: decodeRawMetadata(snapshotBytes),
        targets: decodeRawMetadata(targetsBytes),
        targetBytes
      };
      const completionTime = this.currentTime();
      const draftCandidate = await verifyRawGeneration(raw, bootstrap, completionTime, true, true);
      const candidate = await this.finalizeGeneration(draftCandidate, bootstrap, completionTime);
      await this.commit(candidate, bootstrap, completionTime);
      const current = await verifyRawGeneration(candidate.raw, bootstrap, this.currentTime(), true);
      const completeObservation = await this.persistObservation(onlineRaw, bootstrap);
      await this.compactObservations(completeObservation, bootstrap);
      this.memory.set(environment, current);
      return requireActivePolicy(current.policy);
    } catch (error) {
      if (error instanceof TrustNetworkError) {
        const fallback = await this.readVerifiedCache(environment, bootstrap, this.currentTime());
        if (fallback.channel && fallback.usable) {
          const complete = await this.persistObservation(
            observationDraftFromGeneration(fallback.channel),
            bootstrap
          );
          await this.compactObservations(complete, bootstrap);
          return requireActivePolicy(fallback.channel.policy);
        }
      }
      throw error;
    }
  }
}

// This is a release invariant, not a cache migration mechanism. Supported
// clients stay anchored at root v1 and authenticate every numbered remote root.
assertOfficialEmbeddedBootstrap(embeddedBootstrapJson);
const defaultClient = new AttestationTufClient();

export function refreshAttestationPolicy(
  environment: AttestationChannel
): Promise<VerifiedAttestationPolicy> {
  return defaultClient.refresh(environment);
}

export function getCachedAttestationPolicy(
  environment: AttestationChannel
): VerifiedAttestationPolicy | undefined {
  return defaultClient.getMemoryPolicy(environment);
}

export function assertAttestationPolicyCurrent(policy: VerifiedAttestationPolicy): Promise<void> {
  return defaultClient.assertPolicyCurrent(policy);
}

/** @internal Test-only client factory; not exported from the package entry point. */
export function createAttestationTufClientForTesting(
  options: Required<Pick<AttestationTufClientOptions, "fetch" | "now" | "bootstrap">> & {
    storage?: Storage | null;
  }
): AttestationTufClient {
  return new AttestationTufClient(options);
}

/** @internal Exercises the official embedded-root release sentinel in tests. */
export function assertOfficialEmbeddedBootstrapForTesting(bootstrap: unknown): void {
  assertOfficialEmbeddedBootstrap(bootstrap);
}
