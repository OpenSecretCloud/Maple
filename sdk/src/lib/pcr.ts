/**
 * Valid PCR0 values for production environments
 */
const DEFAULT_PCR0_VALUES = [
  // Approved 2026-10-11 (services/opensecret signed history)
  "21d27bd19cdd02632485a6e8874753e42cf7f098f1335bb62a9c23e7f64da9100c02dba582da17ac68052fa6be16b7a9",
  // Approved 2026-10-09 (services/opensecret signed history; rollback)
  "643b3c293a4eac11ec14ab405d2405e46cb714263a12da6c62f485a0fa395cb3c3d5ef07b18d39a1f9715f25e7943b66"
];

/**
 * Valid PCR0 values for development environments
 */
const DEFAULT_PCR0_VALUES_DEV = [
  // Approved 2026-10-11 (services/opensecret signed history)
  "a95956c3fb9457e509be6d614ffefcddf98fe594f77bb6a56891fc1f5bdec41da2add13db22e0908363c8d0929c04def",
  // Approved 2026-10-09 (services/opensecret signed history; rollback)
  "e573a3ddc8a27f387e24cfb02a8fa594fe782674de6ee1fa9385126b9f8ec38f4a236e91c371bdd2b9444623c1c1cc34"
];

/**
 * Public key used to verify PCR history signatures in SPKI DER format (base64-encoded)
 */
const PCR_VERIFICATION_PUBLIC_KEY_B64 =
  "MHYwEAYHKoZIzj0CAQYFK4EEACIDYgAEHiUY9kFWK1GqBGzczohhwEwElXzgWLDZa9R6wBx3JOBocgSt9+UIzZlJbPDjYeGBfDUXh7Z62BG2vVsh2NgclLB5S7A2ucBBtb1wd8vSQHP8jpdPhZX1slauPgbnROIP";

/**
 * Canonical signed PCR histories in the Maple monorepo.
 */
const PCR_HISTORY_URLS = {
  prod: "https://raw.githubusercontent.com/MaplePrivacyLabs/Maple/master/services/opensecret/pcrProdHistory.json",
  dev: "https://raw.githubusercontent.com/MaplePrivacyLabs/Maple/master/services/opensecret/pcrDevHistory.json"
};

const PCR0_HEX_LENGTH = 96;
const PCR0_HEX_PATTERN = /^[0-9a-f]{96}$/;
const PCR_HISTORY_TIMEOUT_MS = 5000;
const PCR_HISTORY_MAX_BYTES = 1024 * 1024;
const PCR_HISTORY_MAX_ENTRIES = 2048;
const PCR_SIGNATURE_BYTES = 96;

export type Pcr0ValidationErrorCode =
  | "PCR0_MISSING"
  | "PCR0_INVALID_LENGTH"
  | "PCR0_INVALID_FORMAT"
  | "PCR0_ALL_ZERO"
  | "PCR0_UNTRUSTED";

/** A hard attestation failure caused by an invalid or untrusted enclave identity. */
export class Pcr0ValidationError extends Error {
  readonly code: Pcr0ValidationErrorCode;

  constructor(code: Pcr0ValidationErrorCode, message: string) {
    super(message);
    this.name = "Pcr0ValidationError";
    this.code = code;
  }
}

/**
 * PCR history entry type
 */
export type PcrHistoryEntry = {
  PCR0: string;
  PCR1: string;
  PCR2: string;
  timestamp: number;
  signature: string;
};

/**
 * Result of PCR0 validation
 */
export type Pcr0ValidationResult = {
  /** Whether the PCR0 hash matches a known good value */
  isMatch: boolean;
  /** Human-readable description of the validation result */
  text: string;
  /** Timestamp of when the PCR was verified (for remote attestation) */
  verifiedAt?: string;
};

/** The OpenSecret deployment environment whose PCR0 roots are trusted. */
export type PcrEnvironment = "production" | "development";

/**
 * Configuration options for PCR validation
 */
export type PcrConfig = {
  /**
   * OpenSecret deployment environment to trust (defaults to production).
   * Only this environment's embedded roots, additional roots, and signed
   * history are considered during session establishment.
   */
  environment?: PcrEnvironment;
  /**
   * Additional trusted PCR0 values for production environments.
   * These and the SDK's built-in production roots are considered only when
   * `environment` is `"production"`.
   */
  pcr0Values?: string[];
  /**
   * Additional trusted PCR0 values for development environments.
   * These and the SDK's built-in development roots are considered only when
   * `environment` is `"development"`.
   */
  pcr0DevValues?: string[];
  /**
   * Whether to consult pinned-key signed PCR history after local trust roots miss
   * (defaults to true). This does not enable or disable Nitro attestation verification.
   */
  remoteAttestation?: boolean;
  /** Custom URLs for pinned-key signed PCR history. Only the selected environment is fetched. */
  remoteAttestationUrls?: {
    /** URL for production PCR history */
    prod?: string;
    /** URL for development PCR history */
    dev?: string;
  };
};

async function readBoundedUtf8Body(response: Response, maxBytes: number): Promise<string> {
  // Real fetch responses expose a byte stream. Some unit-test and legacy fetch
  // implementations only expose text(), so keep a byte-counted fallback for them.
  if (!response.body) {
    const text = await response.text();
    if (new TextEncoder().encode(text).byteLength > maxBytes) {
      throw new Error("PCR history response is too large");
    }
    return text;
  }

  const reader = response.body.getReader();
  const decoder = new TextDecoder("utf-8", { fatal: true });
  let totalBytes = 0;
  let text = "";

  try {
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      if (!value) continue;

      totalBytes += value.byteLength;
      if (totalBytes > maxBytes) {
        try {
          await reader.cancel("PCR history response is too large");
        } catch {
          // Preserve the size-limit failure even if the stream cannot be cancelled.
        }
        throw new Error("PCR history response is too large");
      }

      text += decoder.decode(value, { stream: true });
    }

    text += decoder.decode();
    return text;
  } finally {
    reader.releaseLock();
  }
}

type CanonicalPcrConfig = {
  version: "pcr0-environment-v1";
  environment: PcrEnvironment;
  verificationKey: string;
  defaultPcr0Values: string[];
  pcr0Values: string[];
  remoteAttestation: boolean;
  remoteAttestationUrl: string;
};

function normalizedValues(values: string[] | undefined): string[] {
  return [...new Set((values || []).map((value) => value.trim().toLowerCase()))].sort();
}

function normalizePcrEnvironment(environment: unknown): PcrEnvironment {
  if (environment === undefined || environment === "production") return "production";
  if (environment === "development") return "development";
  throw new TypeError('PCR environment must be either "production" or "development".');
}

/**
 * Takes an immutable snapshot of a PCR policy before an asynchronous handshake.
 * This prevents caller mutations from changing which policy is validated or cached.
 */
export function snapshotPcrConfig(config?: PcrConfig): PcrConfig {
  return {
    environment: normalizePcrEnvironment(config?.environment),
    pcr0Values: normalizedValues(config?.pcr0Values),
    pcr0DevValues: normalizedValues(config?.pcr0DevValues),
    remoteAttestation: config?.remoteAttestation !== false,
    remoteAttestationUrls: {
      prod: config?.remoteAttestationUrls?.prod || PCR_HISTORY_URLS.prod,
      dev: config?.remoteAttestationUrls?.dev || PCR_HISTORY_URLS.dev
    }
  };
}

/** Stable representation used to bind cached sessions to their trust policy. */
export function serializePcrConfig(config?: PcrConfig): string {
  const snapshot = snapshotPcrConfig(config);
  const environment = snapshot.environment || "production";
  const development = environment === "development";
  const canonical: CanonicalPcrConfig = {
    version: "pcr0-environment-v1",
    environment,
    verificationKey: PCR_VERIFICATION_PUBLIC_KEY_B64,
    defaultPcr0Values: [...(development ? DEFAULT_PCR0_VALUES_DEV : DEFAULT_PCR0_VALUES)].sort(),
    pcr0Values: development ? snapshot.pcr0DevValues || [] : snapshot.pcr0Values || [],
    remoteAttestation: snapshot.remoteAttestation !== false,
    remoteAttestationUrl: development
      ? snapshot.remoteAttestationUrls?.dev || PCR_HISTORY_URLS.dev
      : snapshot.remoteAttestationUrls?.prod || PCR_HISTORY_URLS.prod
  };

  return JSON.stringify(canonical);
}

/** Converts the authenticated Nitro PCR0 value into its canonical SHA-384 hex form. */
export function pcr0BytesToHex(pcr0: Uint8Array | undefined): string {
  if (!pcr0) {
    throw new Pcr0ValidationError("PCR0_MISSING", "Attestation document is missing PCR0.");
  }
  if (pcr0.length !== PCR0_HEX_LENGTH / 2) {
    throw new Pcr0ValidationError(
      "PCR0_INVALID_LENGTH",
      "Attestation document contains an invalid PCR0 length."
    );
  }
  if (pcr0.every((byte) => byte === 0)) {
    throw new Pcr0ValidationError(
      "PCR0_ALL_ZERO",
      "Attestation document contains an all-zero PCR0."
    );
  }

  const hash = Array.from(pcr0, (byte) => byte.toString(16).padStart(2, "0")).join("");
  if (!PCR0_HEX_PATTERN.test(hash)) {
    throw new Pcr0ValidationError(
      "PCR0_INVALID_FORMAT",
      "Attestation document contains an invalid PCR0."
    );
  }
  return hash;
}

/**
 * Enforces PCR0 trust for the already Nitro-authenticated attestation document.
 * Callers must run this before key exchange or session persistence.
 */
export async function requireTrustedPcr0(
  pcrs: Map<number, Uint8Array>,
  config?: PcrConfig,
  validate: typeof validatePcr0Hash = validatePcr0Hash
): Promise<{ hash: string; validation: Pcr0ValidationResult }> {
  const hash = pcr0BytesToHex(pcrs.get(0));
  const validation = await validate(hash, config);
  if (!validation.isMatch) {
    throw new Pcr0ValidationError(
      "PCR0_UNTRUSTED",
      "Attestation PCR0 is not in the configured trust roots."
    );
  }
  return { hash, validation };
}

/**
 * Imports the verification public key into the Web Crypto API
 */
async function importVerificationKey(): Promise<CryptoKey> {
  try {
    // Decode the base64 key to binary
    const binaryKey = new Uint8Array(
      atob(PCR_VERIFICATION_PUBLIC_KEY_B64)
        .split("")
        .map((c) => c.charCodeAt(0))
    );

    // Import as SPKI format
    return await crypto.subtle.importKey(
      "spki", // The format: SubjectPublicKeyInfo
      binaryKey, // Pass the Uint8Array directly, not .buffer
      {
        name: "ECDSA", // The algorithm
        namedCurve: "P-384" // The curve (must be P-384 to match our backend)
      },
      false, // Not extractable
      ["verify"] // Only for verification
    );
  } catch (error) {
    console.error("Error importing verification key:", error);
    throw new Error("Failed to import PCR verification key");
  }
}

/**
 * Fetches PCR history from repository
 */
async function fetchPcrHistory(
  env: "prod" | "dev",
  urls?: { prod?: string; dev?: string }
): Promise<PcrHistoryEntry[]> {
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), PCR_HISTORY_TIMEOUT_MS);
  try {
    const baseUrl = urls?.[env] || PCR_HISTORY_URLS[env];
    const parsedUrl = new URL(baseUrl);
    if (
      (parsedUrl.protocol !== "https:" && parsedUrl.protocol !== "http:") ||
      parsedUrl.username ||
      parsedUrl.password ||
      parsedUrl.hash ||
      baseUrl.length > 2048
    ) {
      throw new Error("Invalid PCR history URL");
    }

    const response = await fetch(baseUrl, {
      redirect: "error",
      signal: controller.signal
    });

    if (!response.ok || response.redirected) {
      throw new Error(`Failed to fetch PCR history: ${response.status}`);
    }

    const contentLength = response.headers.get("content-length");
    if (contentLength && Number(contentLength) > PCR_HISTORY_MAX_BYTES) {
      throw new Error("PCR history response is too large");
    }

    const text = await readBoundedUtf8Body(response, PCR_HISTORY_MAX_BYTES);

    const history: unknown = JSON.parse(text);
    if (
      !Array.isArray(history) ||
      history.length === 0 ||
      history.length > PCR_HISTORY_MAX_ENTRIES ||
      !history.every(isValidPcrHistoryEntry)
    ) {
      throw new Error("PCR history response has an invalid schema");
    }
    return history;
  } catch (error) {
    console.error("Error fetching PCR history:", error);
    throw new Error("Failed to fetch PCR history");
  } finally {
    clearTimeout(timeout);
  }
}

function isValidPcrHistoryEntry(entry: unknown): entry is PcrHistoryEntry {
  if (!entry || typeof entry !== "object") return false;
  const candidate = entry as Partial<PcrHistoryEntry>;
  if (
    typeof candidate.PCR0 !== "string" ||
    typeof candidate.PCR1 !== "string" ||
    typeof candidate.PCR2 !== "string" ||
    !PCR0_HEX_PATTERN.test(candidate.PCR0) ||
    !PCR0_HEX_PATTERN.test(candidate.PCR1) ||
    !PCR0_HEX_PATTERN.test(candidate.PCR2) ||
    typeof candidate.timestamp !== "number" ||
    !Number.isSafeInteger(candidate.timestamp) ||
    candidate.timestamp <= 0 ||
    typeof candidate.signature !== "string" ||
    candidate.signature.length > 256
  ) {
    return false;
  }

  try {
    return atob(candidate.signature).length === PCR_SIGNATURE_BYTES;
  } catch {
    return false;
  }
}

/**
 * Verifies a PCR0 signature
 */
async function verifyPcr0Signature(
  pcr0: string,
  signatureBase64: string,
  publicKey: CryptoKey
): Promise<boolean> {
  try {
    // Convert PCR0 string to binary
    const encoder = new TextEncoder();
    const pcr0Binary = encoder.encode(pcr0);

    // Convert signature from base64 to binary
    const signatureBinary = new Uint8Array(
      atob(signatureBase64)
        .split("")
        .map((c) => c.charCodeAt(0))
    );

    // Verify using Web Crypto API
    return await crypto.subtle.verify(
      {
        name: "ECDSA",
        hash: { name: "SHA-384" } // Must match the hash used for signing
      },
      publicKey,
      signatureBinary,
      pcr0Binary
    );
  } catch (error) {
    console.error("Signature verification error:", error);
    return false;
  }
}

/**
 * Validates a PCR0 against remote history
 */
async function validatePcrAgainstRemoteHistory(
  pcr0: string,
  env: "prod" | "dev",
  urls?: { prod?: string; dev?: string }
): Promise<Pcr0ValidationResult | null> {
  try {
    // Import the verification key
    const publicKey = await importVerificationKey();

    // Fetch the PCR history
    const history = await fetchPcrHistory(env, urls);

    // Find a matching entry in the history
    for (const entry of history) {
      // Only check if PCR0 matches - we don't care about PCR1 or PCR2
      if (entry.PCR0 === pcr0) {
        // Verify the signature (only of PCR0)
        const isValid = await verifyPcr0Signature(entry.PCR0, entry.signature, publicKey);
        if (isValid) {
          return {
            isMatch: true,
            text: "PCR0 matches remotely attested value",
            verifiedAt: new Date(entry.timestamp * 1000).toLocaleString()
          };
        }
      }
    }

    // No valid match found
    return null;
  } catch (error) {
    console.error("PCR remote validation error:", error);
    // We return null for remote validation errors so we can fall back to local validation
    return null;
  }
}

/**
 * Validates a PCR0 hash and returns information about the match
 * @param hash - The PCR0 hash to validate
 * @param config - Optional configuration with custom PCR0 values
 * @returns Object containing match status and descriptive text
 */
export async function validatePcr0Hash(
  hash: string,
  config?: PcrConfig
): Promise<Pcr0ValidationResult> {
  const normalizedHash = hash.trim().toLowerCase();
  const snapshot = snapshotPcrConfig(config);
  const development = snapshot.environment === "development";
  const validPcr0Values = development
    ? [...(snapshot.pcr0DevValues || []), ...DEFAULT_PCR0_VALUES_DEV]
    : [...(snapshot.pcr0Values || []), ...DEFAULT_PCR0_VALUES];

  if (validPcr0Values.includes(normalizedHash)) {
    return {
      isMatch: true,
      text: development ? "PCR0 matches development enclave" : "PCR0 matches a known good value"
    };
  }

  // If remote attestation is enabled (default is true), check against remote PCR history
  const remoteAttestationEnabled = snapshot.remoteAttestation !== false;

  if (remoteAttestationEnabled) {
    try {
      const remoteResult = await validatePcrAgainstRemoteHistory(
        normalizedHash,
        development ? "dev" : "prod",
        snapshot.remoteAttestationUrls
      );

      if (remoteResult) return remoteResult;
    } catch (error) {
      console.error("Error during remote PCR validation:", error);
      // We continue with default behavior if remote validation fails
    }
  }

  return {
    isMatch: false,
    text: "PCR0 does not match a known good value"
  };
}
