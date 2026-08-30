import { describe, expect, test } from "bun:test";
import { verifyTufAuthorizedSigstoreBundle } from "../sigstoreBrowser";

const encoder = new TextEncoder();
const bundleUrl = new URL(
  "../../../rust/tests/fixtures/cosign-v3-blob.sigstore.json",
  import.meta.url
);
const rootUrl = new URL("./fixtures/sigstore-production-root.json", import.meta.url);
const manifestBytes = encoder.encode("test content for cosign\n");
const rekorV2ArtifactUrl = new URL(
  "../../../rust/tests/fixtures/rekor-v2-artifact.txt",
  import.meta.url
);
const rekorV2BundleUrl = new URL(
  "../../../rust/tests/fixtures/rekor-v2-bundle.sigstore.fixture",
  import.meta.url
);
const rekorV2RootUrl = new URL(
  "../../../rust/tests/fixtures/rekor-v2-trusted-root.fixture",
  import.meta.url
);

async function fixtureBytes(url: URL): Promise<Uint8Array> {
  return new Uint8Array(await Bun.file(url).arrayBuffer());
}

async function fixtureJson(url: URL): Promise<Record<string, any>> {
  return JSON.parse(await Bun.file(url).text()) as Record<string, any>;
}

function jsonBytes(value: unknown): Uint8Array {
  return encoder.encode(JSON.stringify(value));
}

async function sha256Hex(bytes: Uint8Array): Promise<string> {
  const digest = new Uint8Array(await crypto.subtle.digest("SHA-256", bytes));
  return Array.from(digest, (byte) => byte.toString(16).padStart(2, "0")).join("");
}

describe("browser Sigstore verification", () => {
  test("pins the test-only bundle and trusted-root fixture bytes", async () => {
    expect(await sha256Hex(await fixtureBytes(bundleUrl))).toBe(
      "ed70e4cadbe916b31d1c9fe913f6ae8d799b5cc3336b104ec839f65cd16befdd"
    );
    expect(await sha256Hex(await fixtureBytes(rootUrl))).toBe(
      "84d95b8389e45dc35f9d22f2a2f30d3f427644ad348c97e3b9f43f49efcb02ad"
    );
    expect(await sha256Hex(await fixtureBytes(rekorV2ArtifactUrl))).toBe(
      "a0cfc71271d6e278e57cd332ff957c3f7043fdda354c4cbb190a30d56efa01bf"
    );
    expect(await sha256Hex(await fixtureBytes(rekorV2BundleUrl))).toBe(
      "3a5ce62cee2653969be846a41e8332eae82c633cdfafe6db48b10e60939518dd"
    );
    expect(await sha256Hex(await fixtureBytes(rekorV2RootUrl))).toBe(
      "ed6a9cf4e7c2e3297a4b5974fce0d17132f03c63512029d7aa3a402b43acab49"
    );
  });

  test("verifies the exact blob, Fulcio path/SCT, Rekor proof/checkpoint, and TSA timestamp", async () => {
    const evidence = await verifyTufAuthorizedSigstoreBundle(
      manifestBytes,
      await fixtureBytes(bundleUrl),
      await fixtureBytes(rootUrl)
    );

    expect(evidence).toEqual({
      logIndex: "738312748",
      logId: "wNI9atQGlz+VWfO6LRygH4QUfY/8W4RFwiT5i5WRgB0=",
      observerTimestamp: "2025-12-03T18:36:42.000Z"
    });
  });

  test("selects the TUF-authenticated Rekor key by log ID instead of root array order", async () => {
    const root = await fixtureJson(rootUrl);
    root.tlogs.reverse();

    await expect(
      verifyTufAuthorizedSigstoreBundle(
        manifestBytes,
        await fixtureBytes(bundleUrl),
        jsonBytes(root)
      )
    ).resolves.toMatchObject({ logIndex: "738312748" });
  });

  test("verifies the official Rekor v2 conformance bundle with omitted integrated time", async () => {
    await expect(
      verifyTufAuthorizedSigstoreBundle(
        await fixtureBytes(rekorV2ArtifactUrl),
        await fixtureBytes(rekorV2BundleUrl),
        await fixtureBytes(rekorV2RootUrl)
      )
    ).resolves.toEqual({
      logIndex: "735",
      logId: "8w1amZ2S5mJIQkQmPxdMuOrL/oJkvFg9MnQXmeOCXck=",
      observerTimestamp: "2025-06-12T12:02:20.000Z"
    });
  });

  test('normalizes Rekor v2 integrated time "0" to the required RFC3161 time', async () => {
    const bundle = await fixtureJson(rekorV2BundleUrl);
    bundle.verificationMaterial.tlogEntries[0].integratedTime = "0";

    await expect(
      verifyTufAuthorizedSigstoreBundle(
        await fixtureBytes(rekorV2ArtifactUrl),
        jsonBytes(bundle),
        await fixtureBytes(rekorV2RootUrl)
      )
    ).resolves.toMatchObject({ observerTimestamp: "2025-06-12T12:02:20.000Z" });
  });

  test("normalizes numeric Rekor v2 integrated time 0 to the required RFC3161 time", async () => {
    const bundle = await fixtureJson(rekorV2BundleUrl);
    bundle.verificationMaterial.tlogEntries[0].integratedTime = 0;

    await expect(
      verifyTufAuthorizedSigstoreBundle(
        await fixtureBytes(rekorV2ArtifactUrl),
        jsonBytes(bundle),
        await fixtureBytes(rekorV2RootUrl)
      )
    ).resolves.toMatchObject({ observerTimestamp: "2025-06-12T12:02:20.000Z" });
  });

  test("rejects nonzero Rekor v2 integrated time instead of treating it as authority", async () => {
    const bundle = await fixtureJson(rekorV2BundleUrl);
    bundle.verificationMaterial.tlogEntries[0].integratedTime = "1";

    await expect(
      verifyTufAuthorizedSigstoreBundle(
        await fixtureBytes(rekorV2ArtifactUrl),
        jsonBytes(bundle),
        await fixtureBytes(rekorV2RootUrl)
      )
    ).rejects.toThrow("strict Sigstore profile");
  });

  test("rejects any change to the exact TUF-selected manifest bytes", async () => {
    await expect(
      verifyTufAuthorizedSigstoreBundle(
        encoder.encode("test content for cosign\n "),
        await fixtureBytes(bundleUrl),
        await fixtureBytes(rootUrl)
      )
    ).rejects.toThrow();
  });

  test.each([
    ["unknown media type", (bundle: Record<string, any>) => (bundle.mediaType = "unknown")],
    [
      "legacy certificate chain",
      (bundle: Record<string, any>) => {
        bundle.verificationMaterial.x509CertificateChain = {
          certificates: [bundle.verificationMaterial.certificate]
        };
      }
    ],
    [
      "DSSE envelope",
      (bundle: Record<string, any>) => {
        delete bundle.messageSignature;
        bundle.dsseEnvelope = { payload: "e30=", payloadType: "test", signatures: [] };
      }
    ],
    [
      "missing checkpoint",
      (bundle: Record<string, any>) => {
        delete bundle.verificationMaterial.tlogEntries[0].inclusionProof.checkpoint;
      }
    ],
    [
      "multiple log entries",
      (bundle: Record<string, any>) => {
        bundle.verificationMaterial.tlogEntries.push(
          structuredClone(bundle.verificationMaterial.tlogEntries[0])
        );
      }
    ],
    [
      "missing TSA timestamp",
      (bundle: Record<string, any>) => {
        bundle.verificationMaterial.timestampVerificationData.rfc3161Timestamps = [];
      }
    ],
    [
      "non-SHA256 digest profile",
      (bundle: Record<string, any>) => {
        bundle.messageSignature.messageDigest.algorithm = "SHA2_512";
      }
    ],
    [
      "zero Rekor v1 integrated time",
      (bundle: Record<string, any>) => {
        bundle.verificationMaterial.tlogEntries[0].integratedTime = "0";
      }
    ],
    [
      "Rekor v1 integrated time without a signed entry timestamp",
      (bundle: Record<string, any>) => {
        delete bundle.verificationMaterial.tlogEntries[0].inclusionPromise;
      }
    ]
  ])("rejects a %s bundle before release authorization", async (_description, mutate) => {
    const bundle = await fixtureJson(bundleUrl);
    mutate(bundle);

    await expect(
      verifyTufAuthorizedSigstoreBundle(
        manifestBytes,
        jsonBytes(bundle),
        await fixtureBytes(rootUrl)
      )
    ).rejects.toThrow();
  });

  test("rejects a bundle whose log ID is absent from the TUF-selected root", async () => {
    const root = await fixtureJson(rootUrl);
    root.tlogs = root.tlogs.filter(
      (log: Record<string, any>) =>
        log.logId.keyId !== "wNI9atQGlz+VWfO6LRygH4QUfY/8W4RFwiT5i5WRgB0="
    );

    await expect(
      verifyTufAuthorizedSigstoreBundle(
        manifestBytes,
        await fixtureBytes(bundleUrl),
        jsonBytes(root)
      )
    ).rejects.toThrow("exactly one TUF-authorized transparency log");
  });
});
