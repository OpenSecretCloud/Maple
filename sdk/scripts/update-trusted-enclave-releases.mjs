#!/usr/bin/env node

import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { readFileSync, renameSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { verify } from "sigstore";

const SNAPSHOT_SCHEMA = "https://opensecret.cloud/sdk/trusted-enclave-releases/v1";
const MANIFEST_SCHEMA = "https://opensecret.cloud/attestations/nitro-eif-release/v1";
const BUNDLE_MEDIA_TYPE = "application/vnd.dev.sigstore.bundle.v0.3+json";
const OIDC_ISSUER = "https://token.actions.githubusercontent.com";
const SOURCE_REPOSITORY = "OpenSecretCloud/opensecret";
const SOURCE_REPOSITORY_URI = `https://github.com/${SOURCE_REPOSITORY}`;
const SOURCE_REPOSITORY_ID = 921901924;
const SOURCE_REPOSITORY_OWNER_ID = 185423582;
const SOURCE_REPOSITORY_OWNER_URI = "https://github.com/OpenSecretCloud";
const WORKFLOW_PATH = ".github/workflows/release-nitro-eif.yml";
const WORKFLOW_NAME = "Nitro EIF Release";
const WORKFLOW_TRIGGER = "workflow_dispatch";
const WORKFLOW_ENVIRONMENT = "production-release";
const REQUIRED_COSIGN_VERSION = [3, 1, 2];

const projectRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const defaultOutputs = [
  resolve(projectRoot, "src/lib/trusted-enclave-releases.generated.json"),
  resolve(projectRoot, "rust/assets/trusted_enclave_releases.generated.json")
];

const SNAPSHOT_POLICY = {
  oidcIssuer: OIDC_ISSUER,
  sourceRepository: SOURCE_REPOSITORY,
  sourceRepositoryId: SOURCE_REPOSITORY_ID,
  sourceRepositoryOwnerId: SOURCE_REPOSITORY_OWNER_ID,
  workflow: {
    path: WORKFLOW_PATH,
    name: WORKFLOW_NAME,
    trigger: WORKFLOW_TRIGGER,
    environment: WORKFLOW_ENVIRONMENT
  }
};

function usage() {
  return `Usage:
  node scripts/update-trusted-enclave-releases.mjs \\
    --manifest <release.manifest.json> --bundle <release.manifest.sigstore.json> \\
    [--manifest <...> --bundle <...>] [--cosign <path>] [--output <path> ...]

Every desired trusted release must be supplied on each run. The updater verifies
each exact manifest byte sequence with both official sigstore-js and Cosign,
then atomically rewrites the TypeScript and Rust embedded snapshots.
`;
}

function fail(message) {
  throw new Error(message);
}

function isPlainObject(value) {
  return (
    value !== null &&
    typeof value === "object" &&
    !Array.isArray(value) &&
    Object.getPrototypeOf(value) === Object.prototype
  );
}

function assertPlainObject(value, path) {
  if (!isPlainObject(value)) {
    fail(`${path} must be a JSON object`);
  }
  return value;
}

function assertExactKeys(value, expectedKeys, path) {
  const object = assertPlainObject(value, path);
  const actual = Object.keys(object).sort();
  const expected = [...expectedKeys].sort();
  if (actual.length !== expected.length || actual.some((key, index) => key !== expected[index])) {
    fail(`${path} must contain exactly: ${expected.join(", ")}`);
  }
  return object;
}

function assertString(value, path, pattern) {
  if (typeof value !== "string" || (pattern && !pattern.test(value))) {
    fail(`${path} has an invalid string value`);
  }
  return value;
}

function assertInteger(value, path, { positive = false } = {}) {
  if (!Number.isSafeInteger(value) || (positive && value <= 0)) {
    fail(`${path} must be a ${positive ? "positive " : ""}safe integer`);
  }
  return value;
}

function assertLiteral(value, expected, path) {
  if (value !== expected) {
    fail(`${path} must equal ${JSON.stringify(expected)}`);
  }
  return value;
}

function sortJson(value) {
  if (Array.isArray(value)) {
    return value.map(sortJson);
  }
  if (isPlainObject(value)) {
    return Object.fromEntries(
      Object.keys(value)
        .sort()
        .map((key) => [key, sortJson(value[key])])
    );
  }
  return value;
}

function canonicalJson(value) {
  return `${JSON.stringify(sortJson(value), null, 2)}\n`;
}

function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

function parseCanonicalManifest(rawBytes, path) {
  let parsed;
  try {
    parsed = JSON.parse(rawBytes.toString("utf8"));
  } catch (error) {
    fail(`${path} is not valid JSON: ${error instanceof Error ? error.message : String(error)}`);
  }

  const canonicalBytes = Buffer.from(canonicalJson(parsed));
  if (!rawBytes.equals(canonicalBytes)) {
    fail(
      `${path} is not canonical key-sorted two-space JSON with one trailing LF (duplicate keys are also rejected)`
    );
  }

  const manifest = assertExactKeys(
    parsed,
    ["schema", "environment", "source", "release", "artifact", "measurements", "build"],
    "manifest"
  );
  assertLiteral(manifest.schema, MANIFEST_SCHEMA, "manifest.schema");
  if (manifest.environment !== "prod" && manifest.environment !== "dev") {
    fail("manifest.environment must be prod or dev");
  }

  const source = assertExactKeys(
    manifest.source,
    ["repository", "repositoryId", "ownerId", "ref", "commit"],
    "manifest.source"
  );
  assertLiteral(source.repository, SOURCE_REPOSITORY, "manifest.source.repository");
  assertLiteral(source.repositoryId, SOURCE_REPOSITORY_ID, "manifest.source.repositoryId");
  assertLiteral(source.ownerId, SOURCE_REPOSITORY_OWNER_ID, "manifest.source.ownerId");
  assertString(source.commit, "manifest.source.commit", /^[0-9a-f]{40}$/);

  const release = assertExactKeys(manifest.release, ["tag"], "manifest.release");
  assertString(release.tag, "manifest.release.tag", /^v(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/);
  assertLiteral(source.ref, `refs/tags/${release.tag}`, "manifest.source.ref");

  const artifact = assertExactKeys(
    manifest.artifact,
    ["name", "mediaType", "sha256", "size"],
    "manifest.artifact"
  );
  assertLiteral(
    artifact.name,
    `opensecret-${release.tag}-${manifest.environment}.eif`,
    "manifest.artifact.name"
  );
  assertLiteral(artifact.mediaType, "application/vnd.aws.nitro.eif", "manifest.artifact.mediaType");
  assertString(artifact.sha256, "manifest.artifact.sha256", /^[0-9a-f]{64}$/);
  assertInteger(artifact.size, "manifest.artifact.size", { positive: true });

  const measurements = assertExactKeys(
    manifest.measurements,
    ["algorithm", "requiredPcrs", "pcrs"],
    "manifest.measurements"
  );
  assertLiteral(measurements.algorithm, "sha384", "manifest.measurements.algorithm");
  if (
    !Array.isArray(measurements.requiredPcrs) ||
    measurements.requiredPcrs.length !== 3 ||
    measurements.requiredPcrs.some((value, index) => value !== index)
  ) {
    fail("manifest.measurements.requiredPcrs must equal [0, 1, 2]");
  }
  const pcrs = assertExactKeys(measurements.pcrs, ["0", "1", "2"], "manifest.measurements.pcrs");
  for (const pcr of ["0", "1", "2"]) {
    const value = assertString(pcrs[pcr], `manifest.measurements.pcrs.${pcr}`, /^[0-9a-f]{96}$/);
    if (/^0+$/.test(value)) {
      fail(`manifest.measurements.pcrs.${pcr} must not be all zero`);
    }
  }

  const build = assertExactKeys(
    manifest.build,
    ["system", "flakeLockSha256", "derivation", "workflowRun"],
    "manifest.build"
  );
  assertLiteral(build.system, "nix", "manifest.build.system");
  assertString(build.flakeLockSha256, "manifest.build.flakeLockSha256", /^[0-9a-f]{64}$/);
  assertLiteral(build.derivation, `eif-${manifest.environment}`, "manifest.build.derivation");
  const workflowRun = assertString(
    build.workflowRun,
    "manifest.build.workflowRun",
    /^https:\/\/github\.com\/OpenSecretCloud\/opensecret\/actions\/runs\/[1-9]\d*\/attempts\/[1-9]\d*$/
  );
  new URL(workflowRun);

  return manifest;
}

function parseBundle(rawBytes, path, expectedManifestSha256) {
  let bundle;
  try {
    bundle = JSON.parse(rawBytes.toString("utf8"));
  } catch (error) {
    fail(`${path} is not valid JSON: ${error instanceof Error ? error.message : String(error)}`);
  }

  assertPlainObject(bundle, "bundle");
  assertLiteral(bundle.mediaType, BUNDLE_MEDIA_TYPE, "bundle.mediaType");
  if (!isPlainObject(bundle.messageSignature) || bundle.dsseEnvelope !== undefined) {
    fail("bundle must contain a v0.3 messageSignature and no DSSE envelope");
  }
  const messageDigest = assertPlainObject(
    bundle.messageSignature.messageDigest,
    "bundle.messageSignature.messageDigest"
  );
  assertLiteral(
    messageDigest.algorithm,
    "SHA2_256",
    "bundle.messageSignature.messageDigest.algorithm"
  );
  const encodedDigest = assertString(
    messageDigest.digest,
    "bundle.messageSignature.messageDigest.digest"
  );
  let digest;
  try {
    digest = Buffer.from(encodedDigest, "base64");
  } catch {
    fail("bundle.messageSignature.messageDigest.digest must be base64");
  }
  if (digest.length !== 32 || digest.toString("hex") !== expectedManifestSha256) {
    fail("bundle message digest does not match the exact manifest bytes");
  }

  const verificationMaterial = assertPlainObject(
    bundle.verificationMaterial,
    "bundle.verificationMaterial"
  );
  if (!isPlainObject(verificationMaterial.certificate)) {
    fail("bundle must contain exactly one Fulcio certificate");
  }
  if (verificationMaterial.x509CertificateChain !== undefined) {
    fail("legacy x509CertificateChain bundles are not accepted");
  }

  const tlogEntries = verificationMaterial.tlogEntries;
  if (!Array.isArray(tlogEntries) || tlogEntries.length !== 1) {
    fail("bundle must contain exactly one transparency-log entry");
  }
  const tlogEntry = assertPlainObject(tlogEntries[0], "bundle.verificationMaterial.tlogEntries[0]");
  const inclusionProof = assertPlainObject(
    tlogEntry.inclusionProof,
    "bundle.verificationMaterial.tlogEntries[0].inclusionProof"
  );
  const checkpoint = assertPlainObject(
    inclusionProof.checkpoint,
    "bundle.verificationMaterial.tlogEntries[0].inclusionProof.checkpoint"
  );
  assertString(
    checkpoint.envelope,
    "bundle.verificationMaterial.tlogEntries[0].inclusionProof.checkpoint.envelope",
    /[\S]/
  );

  const rawLogIndex = tlogEntry.logIndex;
  const encodedLogIndex =
    typeof rawLogIndex === "number" && Number.isSafeInteger(rawLogIndex) && rawLogIndex >= 0
      ? String(rawLogIndex)
      : assertString(rawLogIndex, "bundle.verificationMaterial.tlogEntries[0].logIndex", /^\d+$/);
  const logIndex = BigInt(encodedLogIndex).toString();
  const logId = assertPlainObject(
    tlogEntry.logId,
    "bundle.verificationMaterial.tlogEntries[0].logId"
  );
  const encodedLogIdKey = assertString(
    logId.keyId,
    "bundle.verificationMaterial.tlogEntries[0].logId.keyId"
  );
  const logIdBytes = Buffer.from(encodedLogIdKey, "base64");
  if (logIdBytes.length !== 32) {
    fail("bundle.verificationMaterial.tlogEntries[0].logId.keyId must encode 32 bytes");
  }

  return {
    bundle,
    transparencyLog: { logIndex, logId: logIdBytes.toString("hex") }
  };
}

function parseArgs(argv) {
  const manifests = [];
  const bundles = [];
  const outputs = [];
  let cosign = process.env.COSIGN_BIN || "cosign";

  for (let index = 0; index < argv.length; index += 1) {
    const argument = argv[index];
    const value = argv[index + 1];
    if (argument === "--help" || argument === "-h") {
      process.stdout.write(usage());
      process.exit(0);
    }
    if (!value || value.startsWith("--")) {
      fail(`missing value for ${argument}`);
    }
    if (argument === "--manifest") {
      manifests.push(resolve(value));
    } else if (argument === "--bundle") {
      bundles.push(resolve(value));
    } else if (argument === "--output") {
      outputs.push(resolve(value));
    } else if (argument === "--cosign") {
      cosign = resolve(value);
    } else {
      fail(`unknown argument: ${argument}`);
    }
    index += 1;
  }

  if (manifests.length === 0 || manifests.length !== bundles.length) {
    fail("supply the same non-zero number of --manifest and --bundle arguments");
  }

  return { manifests, bundles, outputs: outputs.length > 0 ? outputs : defaultOutputs, cosign };
}

function parseVersion(text) {
  const match = text.match(/v?(\d+)\.(\d+)\.(\d+)/);
  if (!match) {
    fail(`could not parse Cosign version from: ${text.trim()}`);
  }
  return match.slice(1).map(Number);
}

function requireCosign(cosign) {
  const result = spawnSync(cosign, ["version", "--json"], { encoding: "utf8" });
  if (result.error) {
    fail(`failed to execute Cosign at ${cosign}: ${result.error.message}`);
  }
  if (result.status !== 0) {
    fail(`Cosign version check failed: ${result.stderr || result.stdout}`);
  }
  const version = parseVersion(result.stdout || result.stderr);
  if (version.some((part, index) => part !== REQUIRED_COSIGN_VERSION[index])) {
    fail(
      `Cosign ${version.join(".")} is not supported; exactly ${REQUIRED_COSIGN_VERSION.join(".")} is required`
    );
  }
}

function expectedSignerIdentity(manifest) {
  return `${SOURCE_REPOSITORY_URI}/${WORKFLOW_PATH}@${manifest.source.ref}`;
}

function verifyWithCosign(cosign, manifestPath, bundlePath, manifest) {
  const identity = expectedSignerIdentity(manifest);
  const arguments_ = [
    "verify-blob",
    "--bundle",
    bundlePath,
    "--certificate-identity",
    identity,
    "--certificate-oidc-issuer",
    OIDC_ISSUER,
    "--certificate-github-workflow-name",
    WORKFLOW_NAME,
    "--certificate-github-workflow-repository",
    SOURCE_REPOSITORY,
    "--certificate-github-workflow-ref",
    manifest.source.ref,
    "--certificate-github-workflow-sha",
    manifest.source.commit,
    "--certificate-github-workflow-trigger",
    WORKFLOW_TRIGGER,
    manifestPath
  ];
  const result = spawnSync(cosign, arguments_, { encoding: "utf8" });
  if (result.error) {
    fail(`failed to execute Cosign: ${result.error.message}`);
  }
  if (result.status !== 0) {
    fail(`Cosign rejected ${manifestPath}: ${result.stderr || result.stdout}`);
  }
}

function escapeRegex(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function expectedGenericFulcioOids(manifest) {
  return {
    "1.3.6.1.4.1.57264.1.8": OIDC_ISSUER,
    "1.3.6.1.4.1.57264.1.9": expectedSignerIdentity(manifest),
    "1.3.6.1.4.1.57264.1.10": manifest.source.commit,
    "1.3.6.1.4.1.57264.1.11": "github-hosted",
    "1.3.6.1.4.1.57264.1.12": SOURCE_REPOSITORY_URI,
    "1.3.6.1.4.1.57264.1.13": manifest.source.commit,
    "1.3.6.1.4.1.57264.1.14": manifest.source.ref,
    "1.3.6.1.4.1.57264.1.15": String(SOURCE_REPOSITORY_ID),
    "1.3.6.1.4.1.57264.1.16": SOURCE_REPOSITORY_OWNER_URI,
    "1.3.6.1.4.1.57264.1.17": String(SOURCE_REPOSITORY_OWNER_ID),
    "1.3.6.1.4.1.57264.1.18": expectedSignerIdentity(manifest),
    "1.3.6.1.4.1.57264.1.19": manifest.source.commit,
    "1.3.6.1.4.1.57264.1.20": WORKFLOW_TRIGGER,
    "1.3.6.1.4.1.57264.1.21": manifest.build.workflowRun,
    "1.3.6.1.4.1.57264.1.22": "public",
    "1.3.6.1.4.1.57264.1.23": WORKFLOW_ENVIRONMENT,
    "1.3.6.1.4.1.57264.1.24": `repo:${SOURCE_REPOSITORY}:environment:${WORKFLOW_ENVIRONMENT}`
  };
}

export function decodeDerUtf8String(value, context = "Fulcio extension") {
  if (!(value instanceof Uint8Array)) {
    fail(`${context} must be a byte string`);
  }

  const bytes = Buffer.from(value);
  if (bytes.length < 2 || bytes[0] !== 0x0c) {
    fail(`${context} must be a DER UTF8String`);
  }

  const firstLengthByte = bytes[1];
  let headerLength;
  let contentLength;
  if (firstLengthByte < 0x80) {
    headerLength = 2;
    contentLength = firstLengthByte;
  } else {
    const lengthByteCount = firstLengthByte & 0x7f;
    if (lengthByteCount === 0) {
      fail(`${context} uses an indefinite DER length`);
    }
    if (lengthByteCount > 4 || bytes.length < 2 + lengthByteCount) {
      fail(`${context} has an invalid DER length`);
    }
    if (bytes[2] === 0) {
      fail(`${context} has a non-minimal DER length`);
    }

    contentLength = 0;
    for (let index = 0; index < lengthByteCount; index += 1) {
      contentLength = contentLength * 256 + bytes[2 + index];
    }
    if (contentLength < 0x80) {
      fail(`${context} has a non-minimal DER length`);
    }
    headerLength = 2 + lengthByteCount;
  }

  if (headerLength + contentLength !== bytes.length) {
    fail(`${context} DER length does not match its value`);
  }

  try {
    return new TextDecoder("utf-8", { fatal: true }).decode(bytes.subarray(headerLength));
  } catch {
    fail(`${context} is not valid UTF-8`);
  }
}

function oidToString(oid) {
  const components = oid?.id;
  if (
    !Array.isArray(components) ||
    components.length === 0 ||
    components.some((component) => !Number.isSafeInteger(component) || component < 0)
  ) {
    return undefined;
  }
  return components.join(".");
}

export function verifyGenericFulcioOids(signer, expectedOids) {
  const signerOids = signer?.identity?.oids;
  if (!Array.isArray(signerOids)) {
    fail("verified Fulcio signer did not expose certificate OIDs");
  }

  const expected = new Map(Object.entries(expectedOids));
  const observed = new Map();
  for (const signerOid of signerOids) {
    const oid = oidToString(signerOid?.oid);
    if (!oid || !expected.has(oid)) {
      continue;
    }
    if (observed.has(oid)) {
      fail(`verified Fulcio signer contains duplicate OID ${oid}`);
    }
    observed.set(oid, decodeDerUtf8String(signerOid.value, `Fulcio OID ${oid}`));
  }

  for (const [oid, expectedValue] of expected) {
    if (!observed.has(oid)) {
      fail(`verified Fulcio signer is missing OID ${oid}`);
    }
    const observedValue = observed.get(oid);
    if (observedValue !== expectedValue) {
      fail(
        `verified Fulcio signer has unexpected OID ${oid}: expected ${JSON.stringify(expectedValue)}, got ${JSON.stringify(observedValue)}`
      );
    }
  }
}

async function verifyWithSigstoreJs(bundle, manifestBytes, manifest) {
  const identity = expectedSignerIdentity(manifest);
  const signer = await verify(bundle, manifestBytes, {
    certificateIssuer: OIDC_ISSUER,
    certificateIdentityURI: `^${escapeRegex(identity)}$`,
    // sigstore-js compares these deprecated extensions as raw strings. Fulcio's
    // provider-generic extensions are DER UTF8Strings, so validate those below.
    certificateOIDs: {
      "1.3.6.1.4.1.57264.1.2": WORKFLOW_TRIGGER,
      "1.3.6.1.4.1.57264.1.3": manifest.source.commit,
      "1.3.6.1.4.1.57264.1.4": WORKFLOW_NAME,
      "1.3.6.1.4.1.57264.1.5": SOURCE_REPOSITORY,
      "1.3.6.1.4.1.57264.1.6": manifest.source.ref
    },
    tlogThreshold: 1,
    ctLogThreshold: 1
  });
  verifyGenericFulcioOids(signer, expectedGenericFulcioOids(manifest));
}

function validateNoDuplicateReleases(releases) {
  const unique = {
    release: new Set(),
    manifest: new Set()
  };

  for (const release of releases) {
    const manifest = release.manifest;
    const releaseKey = `${manifest.environment}:${manifest.release.tag}`;
    for (const [kind, key] of [
      ["release", releaseKey],
      ["manifest", release.manifestSha256]
    ]) {
      if (unique[kind].has(key)) {
        fail(`duplicate ${kind} entry in trusted-release inputs: ${key}`);
      }
      unique[kind].add(key);
    }
  }
}

function compareReleaseTags(left, right) {
  const leftParts = left.slice(1).split(".").map(BigInt);
  const rightParts = right.slice(1).split(".").map(BigInt);
  for (let index = 0; index < 3; index += 1) {
    if (leftParts[index] > rightParts[index]) return 1;
    if (leftParts[index] < rightParts[index]) return -1;
  }
  return 0;
}

function writeSnapshotAtomically(path, contents) {
  const temporaryPath = `${path}.tmp-${process.pid}`;
  writeFileSync(temporaryPath, contents, { encoding: "utf8", mode: 0o644 });
  renameSync(temporaryPath, path);
}

function assertSupportedNodeVersion() {
  const [major, minor] = process.versions.node.split(".").map(Number);
  if (!((major === 24 && minor >= 15) || major >= 26)) {
    fail("trusted-release updates require Node 24.15 or newer supported by sigstore-js 5");
  }
}

async function main() {
  assertSupportedNodeVersion();
  const { manifests, bundles, outputs, cosign } = parseArgs(process.argv.slice(2));
  requireCosign(cosign);

  const releases = [];
  for (let index = 0; index < manifests.length; index += 1) {
    const manifestPath = manifests[index];
    const bundlePath = bundles[index];
    const manifestBytes = readFileSync(manifestPath);
    const bundleBytes = readFileSync(bundlePath);
    const manifestSha256 = sha256(manifestBytes);
    const manifest = parseCanonicalManifest(manifestBytes, manifestPath);
    const { bundle, transparencyLog } = parseBundle(bundleBytes, bundlePath, manifestSha256);

    verifyWithCosign(cosign, manifestPath, bundlePath, manifest);
    await verifyWithSigstoreJs(bundle, manifestBytes, manifest);

    releases.push({
      manifestSha256,
      bundleSha256: sha256(bundleBytes),
      signer: {
        oidcIssuer: OIDC_ISSUER,
        identity: expectedSignerIdentity(manifest)
      },
      transparencyLog,
      manifest
    });
  }

  releases.sort((left, right) => {
    const environmentOrder =
      left.manifest.environment < right.manifest.environment
        ? -1
        : left.manifest.environment > right.manifest.environment
          ? 1
          : 0;
    return (
      environmentOrder || compareReleaseTags(left.manifest.release.tag, right.manifest.release.tag)
    );
  });
  validateNoDuplicateReleases(releases);

  const snapshotWithoutId = {
    schema: SNAPSHOT_SCHEMA,
    policy: SNAPSHOT_POLICY,
    releases
  };
  const snapshot = {
    ...snapshotWithoutId,
    snapshotId: sha256(Buffer.from(canonicalJson(snapshotWithoutId)))
  };
  const output = canonicalJson(snapshot);

  for (const outputPath of outputs) {
    writeSnapshotAtomically(outputPath, output);
    process.stdout.write(`Wrote ${outputPath}\n`);
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  main().catch((error) => {
    process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`);
    process.exitCode = 1;
  });
}
