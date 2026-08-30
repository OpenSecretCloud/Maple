import {
  CertificateChainVerifier,
  SigstoreVerifier,
  X509Certificate,
  verifyBundleTimestamp,
  type SigstoreBundle,
  type TrustedRoot,
  type VerificationPolicy
} from "@freedomofpress/sigstore-browser";
import { decode as decodeBase64, encode as encodeBase64 } from "@stablelib/base64";
import { z } from "zod";

const BUNDLE_MEDIA_TYPE = "application/vnd.dev.sigstore.bundle.v0.3+json";
const TRUSTED_ROOT_MEDIA_TYPE = "application/vnd.dev.sigstore.trustedroot+json;version=0.1";
const MAX_MANIFEST_BYTES = 128 * 1024;
const MAX_BUNDLE_BYTES = 2 * 1024 * 1024;
const MAX_TRUSTED_ROOT_BYTES = 512 * 1024;
const MAX_CERTIFICATE_BYTES = 64 * 1024;
const MAX_PUBLIC_KEY_BYTES = 16 * 1024;
const MAX_SIGNATURE_BYTES = 16 * 1024;
const MAX_TIMESTAMP_BYTES = 256 * 1024;
const MAX_REKOR_BODY_BYTES = 1024 * 1024;
const MAX_CHECKPOINT_CHARS = 64 * 1024;
const MAX_SAFE_INTEGER_BIGINT = BigInt(Number.MAX_SAFE_INTEGER);

function strictBase64(maxBytes: number, exactBytes?: number): z.ZodType<string> {
  const maxChars = Math.ceil((maxBytes * 4) / 3) + 4;
  return z
    .string()
    .min(4)
    .max(maxChars)
    .regex(/^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/)
    .superRefine((value, context) => {
      try {
        const decoded = decodeBase64(value);
        if (decoded.length === 0 || decoded.length > maxBytes) {
          context.addIssue({ code: z.ZodIssueCode.custom, message: "base64 value is too large" });
          return;
        }
        if (exactBytes !== undefined && decoded.length !== exactBytes) {
          context.addIssue({
            code: z.ZodIssueCode.custom,
            message: `base64 value must decode to ${exactBytes} bytes`
          });
        }
        if (encodeBase64(decoded) !== value) {
          context.addIssue({
            code: z.ZodIssueCode.custom,
            message: "base64 value is not canonically encoded"
          });
        }
      } catch {
        context.addIssue({ code: z.ZodIssueCode.custom, message: "invalid base64 value" });
      }
    });
}

const DecimalIntegerSchema = z
  .string()
  .regex(/^(0|[1-9][0-9]{0,15})$/)
  .refine((value) => BigInt(value) <= MAX_SAFE_INTEGER_BIGINT, "integer exceeds the safe range");
const PositiveDecimalIntegerSchema = DecimalIntegerSchema.refine(
  (value) => value !== "0",
  "integer must be positive"
);
const DateTimeSchema = z.string().max(64).datetime({ offset: true });

const CertificateSchema = z
  .object({
    rawBytes: strictBase64(MAX_CERTIFICATE_BYTES)
  })
  .strict();

const InclusionProofSchema = z
  .object({
    logIndex: DecimalIntegerSchema,
    rootHash: strictBase64(32, 32),
    treeSize: PositiveDecimalIntegerSchema,
    hashes: z.array(strictBase64(32, 32)).max(64),
    checkpoint: z
      .object({
        envelope: z.string().min(1).max(MAX_CHECKPOINT_CHARS)
      })
      .strict()
  })
  .strict()
  .superRefine((proof, context) => {
    if (BigInt(proof.logIndex) >= BigInt(proof.treeSize)) {
      context.addIssue({
        code: z.ZodIssueCode.custom,
        path: ["logIndex"],
        message: "inclusion-proof index must be smaller than its tree size"
      });
    }
  });

const TLogEntrySchema = z
  .object({
    logIndex: DecimalIntegerSchema,
    logId: z.object({ keyId: strictBase64(32, 32) }).strict(),
    kindVersion: z
      .object({
        kind: z.literal("hashedrekord"),
        version: z.enum(["0.0.1", "0.0.2"])
      })
      .strict(),
    integratedTime: z.union([DecimalIntegerSchema, z.literal(0)]).nullish(),
    inclusionPromise: z
      .object({ signedEntryTimestamp: strictBase64(MAX_SIGNATURE_BYTES) })
      .strict()
      .optional(),
    inclusionProof: InclusionProofSchema,
    canonicalizedBody: strictBase64(MAX_REKOR_BODY_BYTES)
  })
  .strict()
  .superRefine((entry, context) => {
    if (entry.kindVersion.version === "0.0.1") {
      if (entry.integratedTime === null || entry.integratedTime === undefined) {
        context.addIssue({
          code: z.ZodIssueCode.custom,
          path: ["integratedTime"],
          message: "Rekor v1 entries require an integrated time"
        });
      } else if (entry.integratedTime === "0" || entry.integratedTime === 0) {
        context.addIssue({
          code: z.ZodIssueCode.custom,
          path: ["integratedTime"],
          message: "Rekor v1 integrated time must be positive"
        });
      }
      if (entry.inclusionPromise === undefined) {
        context.addIssue({
          code: z.ZodIssueCode.custom,
          path: ["inclusionPromise"],
          message: "Rekor v1 integrated time requires a signed entry timestamp"
        });
      }
    } else if (
      entry.integratedTime !== null &&
      entry.integratedTime !== undefined &&
      entry.integratedTime !== "0" &&
      entry.integratedTime !== 0
    ) {
      context.addIssue({
        code: z.ZodIssueCode.custom,
        path: ["integratedTime"],
        message: "Rekor v2 integrated time must be absent, null, or zero"
      });
    }
  });

const TimestampVerificationDataSchema = z
  .object({
    rfc3161Timestamps: z
      .array(z.object({ signedTimestamp: strictBase64(MAX_TIMESTAMP_BYTES) }).strict())
      .min(1)
      .max(4)
  })
  .strict();

const BundleSchema = z
  .object({
    mediaType: z.literal(BUNDLE_MEDIA_TYPE),
    verificationMaterial: z
      .object({
        certificate: CertificateSchema,
        tlogEntries: z.array(TLogEntrySchema).length(1),
        timestampVerificationData: TimestampVerificationDataSchema
      })
      .strict(),
    messageSignature: z
      .object({
        messageDigest: z
          .object({
            algorithm: z.literal("SHA2_256"),
            digest: strictBase64(32, 32)
          })
          .strict(),
        signature: strictBase64(MAX_SIGNATURE_BYTES)
      })
      .strict()
  })
  .strict();

const ValidForSchema = z
  .object({
    start: DateTimeSchema,
    end: DateTimeSchema.optional()
  })
  .strict()
  .superRefine((validFor, context) => {
    if (validFor.end !== undefined && Date.parse(validFor.end) <= Date.parse(validFor.start)) {
      context.addIssue({
        code: z.ZodIssueCode.custom,
        path: ["end"],
        message: "validity end must be after its start"
      });
    }
  });

const SubjectSchema = z
  .object({
    organization: z.string().min(1).max(512),
    commonName: z.string().min(1).max(512)
  })
  .strict();

const CertificateChainSchema = z
  .object({
    certificates: z.array(CertificateSchema).min(1).max(8)
  })
  .strict();

const LogSchema = z
  .object({
    baseUrl: z
      .string()
      .url()
      .max(2048)
      .refine(isExactHttpsUrl, "transparency-log URL must be exact HTTPS"),
    hashAlgorithm: z.literal("SHA2_256"),
    publicKey: z
      .object({
        rawBytes: strictBase64(MAX_PUBLIC_KEY_BYTES),
        keyDetails: z.string().min(1).max(128),
        validFor: ValidForSchema
      })
      .strict(),
    logId: z.object({ keyId: strictBase64(32, 32) }).strict()
  })
  .strict();

const CertificateAuthoritySchema = z
  .object({
    subject: SubjectSchema,
    uri: z.string().url().max(2048).refine(isExactHttpsUrl, "Fulcio URL must be exact HTTPS"),
    certChain: CertificateChainSchema,
    validFor: ValidForSchema
  })
  .strict();

const TimestampAuthoritySchema = z
  .object({
    subject: SubjectSchema,
    uri: z.string().url().max(2048).refine(isExactHttpsUrl, "TSA URL must be exact HTTPS"),
    certChain: CertificateChainSchema,
    validFor: ValidForSchema
  })
  .strict();

const TrustedRootSchema = z
  .object({
    mediaType: z.literal(TRUSTED_ROOT_MEDIA_TYPE),
    tlogs: z.array(LogSchema).min(1).max(16),
    certificateAuthorities: z.array(CertificateAuthoritySchema).min(1).max(16),
    ctlogs: z.array(LogSchema).min(1).max(16),
    timestampAuthorities: z.array(TimestampAuthoritySchema).min(1).max(16)
  })
  .strict();

export type VerifiedSigstoreEvidence = {
  logIndex: string;
  logId: string;
  observerTimestamp: string;
};

class ExactTufTargetPolicy implements VerificationPolicy {
  verify(_certificate: X509Certificate): void {
    // The application policy was already applied when Maple TUF selected the
    // exact manifest, bundle, and trusted-root bytes. Fulcio still authenticates
    // the ephemeral signing key, but signer claims are audit data, not a second
    // repository/workflow/issuer/SAN authorization layer.
  }
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

function parseStrictJson<T>(bytes: Uint8Array, schema: z.ZodType<T>, description: string): T {
  let decoded: string;
  try {
    decoded = new TextDecoder("utf-8", { fatal: true }).decode(bytes);
  } catch (error) {
    throw new Error(`${description} is not valid UTF-8.`, { cause: error });
  }
  let value: unknown;
  try {
    value = JSON.parse(decoded) as unknown;
  } catch (error) {
    throw new Error(`${description} is not valid JSON.`, { cause: error });
  }
  const parsed = schema.safeParse(value);
  if (!parsed.success) {
    throw new Error(`${description} does not match Maple's strict Sigstore profile.`, {
      cause: parsed.error
    });
  }
  return parsed.data;
}

function assertSize(bytes: Uint8Array, maximum: number, description: string): void {
  if (bytes.length === 0 || bytes.length > maximum) {
    throw new Error(`${description} exceeds Maple's ${maximum}-byte limit.`);
  }
}

function withinValidity(
  timestamp: Date,
  validFor: { start: string; end?: string | undefined }
): boolean {
  const instant = timestamp.getTime();
  return (
    instant >= Date.parse(validFor.start) &&
    (validFor.end === undefined || instant <= Date.parse(validFor.end))
  );
}

async function assertFullCertificatePathAtObserverTime(
  certificateBase64: string,
  trustedRoot: z.infer<typeof TrustedRootSchema>,
  observerTimes: readonly Date[]
): Promise<void> {
  const leaf = X509Certificate.parse(decodeBase64(certificateBase64));
  for (const observerTime of observerTimes) {
    let verified = false;
    let lastError: unknown;
    for (const authority of trustedRoot.certificateAuthorities) {
      if (!withinValidity(observerTime, authority.validFor)) continue;
      try {
        const trustedCerts = authority.certChain.certificates.map((certificate) =>
          X509Certificate.parse(decodeBase64(certificate.rawBytes))
        );
        await new CertificateChainVerifier({
          untrustedCert: leaf,
          trustedCerts,
          timestamp: observerTime
        }).verify();
        verified = true;
        break;
      } catch (error) {
        lastError = error;
      }
    }
    if (!verified) {
      throw new Error(
        "Sigstore certificate chain was not valid at an authenticated observer timestamp.",
        { cause: lastError }
      );
    }
  }
}

/**
 * Verifies one exact TUF-authorized manifest and its portable Sigstore bundle.
 *
 * TUF selects the bytes. This adapter deliberately does not authorize a
 * repository, workflow, OIDC issuer, or SAN value: builder admission belongs
 * to release promotion. It still verifies the Fulcio path and SCT, Rekor body,
 * inclusion proof and checkpoint, RFC3161 timestamp, and blob signature.
 */
export async function verifyTufAuthorizedSigstoreBundle(
  manifestBytes: Uint8Array,
  bundleBytes: Uint8Array,
  trustedRootBytes: Uint8Array
): Promise<VerifiedSigstoreEvidence> {
  assertSize(manifestBytes, MAX_MANIFEST_BYTES, "Release manifest");
  assertSize(bundleBytes, MAX_BUNDLE_BYTES, "Sigstore bundle");
  assertSize(trustedRootBytes, MAX_TRUSTED_ROOT_BYTES, "Sigstore trusted root");

  const bundle = parseStrictJson(bundleBytes, BundleSchema, "Sigstore bundle");
  const trustedRoot = parseStrictJson(trustedRootBytes, TrustedRootSchema, "Sigstore trusted root");
  const bundleLogId = bundle.verificationMaterial.tlogEntries[0].logId.keyId;
  const matchingLogs = trustedRoot.tlogs.filter((log) => log.logId.keyId === bundleLogId);
  if (matchingLogs.length !== 1) {
    throw new Error("Sigstore bundle must identify exactly one TUF-authorized transparency log.");
  }
  // Upstream 0.1.14 retains only the first currently valid Rekor key when it
  // loads a root. Selecting the already TUF-authenticated key by the bundle's
  // one required log ID avoids making root array order an accidental pin.
  const verificationRoot = { ...trustedRoot, tlogs: matchingLogs };
  const bundleEntry = bundle.verificationMaterial.tlogEntries[0];
  // Rekor v2 has no authenticated integrated time. Protobuf JSON may omit the
  // field, encode it as null, or expose the wire value as either 0 or "0".
  // Upstream 0.1.14 treats the non-empty string "0" as Unix epoch, so
  // normalize only these version-coupled sentinels and let the required
  // RFC3161 timestamp supply authenticated observer time.
  const verificationBundle =
    bundleEntry.kindVersion.version === "0.0.2"
      ? {
          ...bundle,
          verificationMaterial: {
            ...bundle.verificationMaterial,
            tlogEntries: [{ ...bundleEntry, integratedTime: null }]
          }
        }
      : bundle;
  const verifier = new SigstoreVerifier({
    tlogThreshold: 1,
    ctlogThreshold: 1,
    tsaThreshold: 1
  });
  await verifier.loadSigstoreRoot(verificationRoot as TrustedRoot);
  const verified = await verifier.verifyArtifactPolicy(
    new ExactTufTargetPolicy(),
    verificationBundle as SigstoreBundle,
    manifestBytes,
    false
  );
  if (!verified) throw new Error("Sigstore verifier did not authenticate the manifest.");

  const signatureBytes = decodeBase64(bundle.messageSignature.signature);
  const observerTimes = await verifyBundleTimestamp(
    bundle.verificationMaterial.timestampVerificationData,
    signatureBytes,
    trustedRoot.timestampAuthorities
  );
  if (observerTimes.length < 1) {
    throw new Error("Sigstore bundle has no authenticated observer timestamp.");
  }
  await assertFullCertificatePathAtObserverTime(
    bundle.verificationMaterial.certificate.rawBytes,
    trustedRoot,
    observerTimes
  );

  const entry = bundle.verificationMaterial.tlogEntries[0];
  return Object.freeze({
    logIndex: entry.logIndex,
    logId: entry.logId.keyId,
    observerTimestamp: observerTimes[0].toISOString()
  });
}
