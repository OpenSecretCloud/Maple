import type { ManifestV2 } from "./manifest";
import { fixture, PAYLOAD } from "./fixtures";

export function encodeLinuxFixture(
  manifest: ManifestV2,
): Uint8Array<ArrayBuffer> {
  const envelope = JSON.parse(new TextDecoder().decode(fixture().bytes));
  envelope.manifest = JSON.stringify(manifest);
  envelope.signature = envelope.signature.replace(
    "version=0.0.2",
    `version=${manifest.version}`,
  );
  return new TextEncoder().encode(JSON.stringify(envelope));
}

export function linuxFixture(): {
  manifest: ManifestV2;
  bytes: Uint8Array<ArrayBuffer>;
} {
  const artifact = {
    path: "/agent/stable/releases/0.0.2/Maple-Agent-linux-x86_64.tar.gz",
    size: PAYLOAD.length,
    sha256: "c".repeat(64),
  };
  const manifest: ManifestV2 = {
    ...fixture().manifest,
    format: "maple-agent-update-v2",
    artifacts: { "linux-x86_64": artifact },
    downloads: { "linux-bundle": { ...artifact } },
  };
  return { manifest, bytes: encodeLinuxFixture(manifest) };
}
