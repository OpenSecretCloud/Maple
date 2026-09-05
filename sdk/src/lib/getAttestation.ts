import { verifyAttestationDocument, isLocalDevelopmentApiUrl } from "./attestation";
import type { AttestationDocument } from "./attestation";
import { getApiUrl, keyExchange } from "./api";
import nacl from "tweetnacl";
import { ChaCha20Poly1305 } from "@stablelib/chacha20poly1305";
import { encode, decode } from "@stablelib/base64";
import {
  requireTrustedPcrsAgainstSnapshot,
  resolveAttestationEnvironment,
  resolveTrustedPcrPolicy,
  serializePcrConfig,
  snapshotPcrConfig,
  type AttestationEnvironment,
  type PcrConfig,
  type TrustedEnclaveReleaseSnapshot
} from "./pcr";

export interface Attestation {
  sessionKey: Uint8Array | null;
  sessionId: string | null;
}

type NaclKeyPair = { publicKey: Uint8Array; secretKey: Uint8Array };

type CachedAttestationSession = {
  version: 1;
  apiUrl: string;
  policyFingerprint: string;
  sessionKey: string;
  sessionId: string;
  verifiedPcr0: string | "local-development";
};

const SESSION_CACHE_PREFIX = "opensecret:attested-session:v1:";
const LEGACY_SESSION_KEYS = ["sessionKey", "sessionId"];
const SESSION_KEY_LENGTH = 32;
const SESSION_ID_MAX_LENGTH = 512;
const SESSION_CACHE_MAX_LENGTH = 4096;
const PCR0_PATTERN = /^[0-9a-f]{96}$/;
const SESSION_ID_PATTERN = /^[\x21-\x7e]+$/;

/** @internal Exported for deterministic handshake tests, not from the package entry point. */
export interface GetAttestationDependencies {
  verifyAttestation: typeof verifyAttestationDocument;
  resolveTrustedPcrPolicy: typeof resolveTrustedPcrPolicy;
  requireTrustedPcrsAgainstSnapshot: typeof requireTrustedPcrsAgainstSnapshot;
  keyExchange: typeof keyExchange;
  generateNaclKeyPair: () => NaclKeyPair;
  decryptSessionKey: (
    encryptedSessionKey: string,
    clientSecretKey: Uint8Array,
    serverPublicKey: Uint8Array
  ) => Uint8Array | null;
  randomUUID: () => string;
}

function generateNaclKeyPair(): NaclKeyPair {
  const testNaclPublicKey = import.meta.env.VITE_TEST_NACL_PUBLIC_KEY;
  const testNaclSecretKey = import.meta.env.VITE_TEST_NACL_SECRET_KEY;

  if (testNaclPublicKey && testNaclSecretKey) {
    return {
      publicKey: decode(testNaclPublicKey),
      secretKey: decode(testNaclSecretKey)
    };
  }

  return nacl.box.keyPair();
}

function decryptSessionKey(
  encryptedSessionKey: string,
  clientSecretKey: Uint8Array,
  serverPublicKey: Uint8Array
): Uint8Array | null {
  const sharedSecret = nacl.scalarMult(clientSecretKey, serverPublicKey);
  const encryptedData = decode(encryptedSessionKey);
  const nonceLength = 12;
  const decryptionNonce = encryptedData.slice(0, nonceLength);
  const ciphertext = encryptedData.slice(nonceLength);
  return new ChaCha20Poly1305(sharedSecret).open(decryptionNonce, ciphertext);
}

const defaultDependencies: GetAttestationDependencies = {
  verifyAttestation: verifyAttestationDocument,
  resolveTrustedPcrPolicy,
  requireTrustedPcrsAgainstSnapshot,
  keyExchange,
  generateNaclKeyPair,
  decryptSessionKey,
  randomUUID: () => window.crypto.randomUUID()
};

function canonicalizeApiUrl(apiUrl: string): string {
  let url: URL;
  try {
    url = new URL(apiUrl);
  } catch {
    throw new Error("Attestation requires a valid API URL.");
  }

  if (url.protocol !== "https:" && url.protocol !== "http:") {
    throw new Error("Attestation API URL must use HTTP or HTTPS.");
  }
  if (url.username || url.password || url.search || url.hash) {
    throw new Error("Attestation API URL must not contain credentials, a query, or a fragment.");
  }

  const path = url.pathname === "/" ? "" : url.pathname.replace(/\/+$/, "");
  return `${url.origin}${path}`;
}

async function sha256Hex(value: string): Promise<string> {
  const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(value));
  return Array.from(new Uint8Array(digest), (byte) => byte.toString(16).padStart(2, "0")).join("");
}

async function getAttestationScope(apiUrl: string, pcrConfig?: PcrConfig) {
  const canonicalApiUrl = canonicalizeApiUrl(apiUrl);
  const policyFingerprint = await sha256Hex(serializePcrConfig(pcrConfig));
  const scopeFingerprint = await sha256Hex(`${canonicalApiUrl}\n${policyFingerprint}`);
  return {
    apiUrl: canonicalApiUrl,
    policyFingerprint,
    cacheKey: `${SESSION_CACHE_PREFIX}${scopeFingerprint}`
  };
}

function getSessionStorage(): Storage | undefined {
  try {
    return globalThis.sessionStorage;
  } catch {
    return undefined;
  }
}

function removeStorageItem(key: string): void {
  try {
    getSessionStorage()?.removeItem(key);
  } catch {
    // Storage can be unavailable in sandboxed browser contexts. A fresh,
    // verified in-memory session is still safe to use for the current request.
  }
}

function removeLegacySession(): void {
  for (const key of LEGACY_SESSION_KEYS) removeStorageItem(key);
}

function readCachedSession(
  cacheKey: string,
  apiUrl: string,
  policyFingerprint: string,
  localDevelopment: boolean
): Attestation | undefined {
  let raw: string | null;
  try {
    raw = getSessionStorage()?.getItem(cacheKey) || null;
  } catch {
    return undefined;
  }
  if (!raw) return undefined;

  try {
    if (raw.length > SESSION_CACHE_MAX_LENGTH) {
      throw new Error("Cached attestation session is too large.");
    }
    const cached = JSON.parse(raw) as Partial<CachedAttestationSession>;
    const validPcrEvidence = localDevelopment
      ? cached.verifiedPcr0 === "local-development"
      : typeof cached.verifiedPcr0 === "string" && PCR0_PATTERN.test(cached.verifiedPcr0);
    if (
      cached.version !== 1 ||
      cached.apiUrl !== apiUrl ||
      cached.policyFingerprint !== policyFingerprint ||
      typeof cached.sessionId !== "string" ||
      cached.sessionId.length === 0 ||
      cached.sessionId.length > SESSION_ID_MAX_LENGTH ||
      !SESSION_ID_PATTERN.test(cached.sessionId) ||
      typeof cached.sessionKey !== "string" ||
      !validPcrEvidence
    ) {
      throw new Error("Invalid cached attestation session.");
    }

    const sessionKey = decode(cached.sessionKey);
    if (sessionKey.length !== SESSION_KEY_LENGTH) {
      throw new Error("Invalid cached attestation session key.");
    }
    return { sessionKey, sessionId: cached.sessionId };
  } catch {
    removeStorageItem(cacheKey);
    return undefined;
  }
}

function writeCachedSession(cacheKey: string, cached: CachedAttestationSession): void {
  try {
    getSessionStorage()?.setItem(cacheKey, JSON.stringify(cached));
  } catch {
    // A storage failure must not turn a verified handshake into an unverified
    // one. Continue with the verified in-memory session and simply skip reuse.
  }
}

/** @internal Exported for cache migration and integration tests. */
export async function getAttestationSessionStorageKey(
  apiUrl: string,
  pcrConfig?: PcrConfig
): Promise<string> {
  return (await getAttestationScope(apiUrl, pcrConfig)).cacheKey;
}

/** @internal Test helper; not exported from the package entry point. */
export async function cacheAttestationSessionForTesting(
  apiUrl: string,
  pcrConfig: PcrConfig | undefined,
  attestation: { sessionKey: Uint8Array; sessionId: string },
  verifiedPcr0: string
): Promise<void> {
  const scope = await getAttestationScope(apiUrl, pcrConfig);
  writeCachedSession(scope.cacheKey, {
    version: 1,
    apiUrl: scope.apiUrl,
    policyFingerprint: scope.policyFingerprint,
    sessionKey: encode(attestation.sessionKey),
    sessionId: attestation.sessionId,
    verifiedPcr0
  });
}

/** Clears both vulnerable legacy entries and all policy-scoped SDK sessions. */
export function clearAttestationSessions(): void {
  removeLegacySession();
  const storage = getSessionStorage();
  if (!storage) return;

  const keys: string[] = [];
  try {
    for (let index = 0; index < storage.length; index += 1) {
      const key = storage.key(index);
      if (key && (key.startsWith(SESSION_CACHE_PREFIX) || LEGACY_SESSION_KEYS.includes(key))) {
        keys.push(key);
      }
    }
  } catch {
    return;
  }
  for (const key of keys) removeStorageItem(key);
}

export async function getAttestation(
  forceRefresh?: boolean,
  explicitApiUrl?: string,
  pcrConfig?: PcrConfig
): Promise<Attestation> {
  return getAttestationWithDependencies(
    forceRefresh,
    explicitApiUrl,
    pcrConfig,
    defaultDependencies
  );
}

/** @internal Exported for deterministic handshake tests, not from the package entry point. */
export async function getAttestationWithDependencies(
  forceRefresh: boolean | undefined,
  explicitApiUrl: string | undefined,
  pcrConfig: PcrConfig | undefined,
  dependencies: GetAttestationDependencies
): Promise<Attestation> {
  removeLegacySession();

  const configuredApiUrl = explicitApiUrl || getApiUrl();
  if (!configuredApiUrl) {
    throw new Error("Attestation requires a configured API URL.");
  }

  const policy = snapshotPcrConfig(pcrConfig);
  const scope = await getAttestationScope(configuredApiUrl, policy);
  const localDevelopment = isLocalDevelopmentApiUrl(scope.apiUrl);
  const expectedEnvironment: AttestationEnvironment | undefined = localDevelopment
    ? undefined
    : resolveAttestationEnvironment(scope.apiUrl, policy.environment);

  console.groupCollapsed("Attestation");
  try {
    if (forceRefresh) {
      removeStorageItem(scope.cacheKey);
    } else {
      const cached = readCachedSession(
        scope.cacheKey,
        scope.apiUrl,
        scope.policyFingerprint,
        localDevelopment
      );
      if (cached) {
        console.log("Using existing PCR-verified attestation from session storage.");
        return cached;
      }
    }

    let trustedPolicy: TrustedEnclaveReleaseSnapshot | undefined;
    if (!localDevelopment) {
      // The backend's pending attestation secret is short-lived. Resolve TUF
      // before requesting a nonce-bound document so repository refresh latency
      // cannot consume that key-exchange window.
      trustedPolicy = await dependencies.resolveTrustedPcrPolicy(expectedEnvironment!);
    }

    const attestationNonce = dependencies.randomUUID();
    console.log("Generated attestation nonce:", attestationNonce);
    const document: AttestationDocument = await dependencies.verifyAttestation(
      attestationNonce,
      scope.apiUrl,
      expectedEnvironment
    );

    if (!document.public_key) {
      throw new Error("Invalid attestation document: missing enclave public key.");
    }

    let verifiedPcr0: string | "local-development";
    if (localDevelopment) {
      verifiedPcr0 = "local-development";
      console.warn("LOCAL DEVELOPMENT: PCR0 verification is bypassed for exact HTTP loopback.");
    } else {
      await dependencies.requireTrustedPcrsAgainstSnapshot(
        document.pcrs,
        expectedEnvironment!,
        trustedPolicy!
      );
      const pcr0 = document.pcrs.get(0);
      if (!pcr0 || pcr0.length !== 48) {
        throw new Error("Attestation document must contain a 48-byte PCR0 value.");
      }
      verifiedPcr0 = Array.from(pcr0, (byte) => byte.toString(16).padStart(2, "0")).join("");
      console.log("Attestation trusted-release PCR0/PCR1/PCR2 verification succeeded.");
    }

    const clientKeyPair = dependencies.generateNaclKeyPair();
    const { encrypted_session_key, session_id } = await dependencies.keyExchange(
      encode(clientKeyPair.publicKey),
      attestationNonce,
      scope.apiUrl
    );

    if (
      typeof session_id !== "string" ||
      session_id.length === 0 ||
      session_id.length > SESSION_ID_MAX_LENGTH ||
      !SESSION_ID_PATTERN.test(session_id)
    ) {
      throw new Error("Key exchange returned an invalid session ID.");
    }

    const decryptedSessionKey = dependencies.decryptSessionKey(
      encrypted_session_key,
      clientKeyPair.secretKey,
      new Uint8Array(document.public_key)
    );
    if (!decryptedSessionKey || decryptedSessionKey.length !== SESSION_KEY_LENGTH) {
      throw new Error("Failed to decrypt a valid session key.");
    }

    writeCachedSession(scope.cacheKey, {
      version: 1,
      apiUrl: scope.apiUrl,
      policyFingerprint: scope.policyFingerprint,
      sessionKey: encode(decryptedSessionKey),
      sessionId: session_id,
      verifiedPcr0
    });
    return { sessionKey: decryptedSessionKey, sessionId: session_id };
  } catch (error) {
    removeStorageItem(scope.cacheKey);
    console.error("Error verifying attestation:", error);
    throw error;
  } finally {
    console.groupEnd();
  }
}
