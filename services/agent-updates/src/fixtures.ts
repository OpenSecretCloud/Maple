import type { Manifest } from "./manifest";

export const PAYLOAD = new TextEncoder().encode("a signed package fixture\n");
export function fixture(linux = false): {
  manifest: Manifest;
  bytes: Uint8Array<ArrayBuffer>;
} {
  const prefix = "/agent/stable/releases/0.0.2/";
  const artifact = (name: string) => ({
    path: prefix + name,
    size: PAYLOAD.length,
    sha256: "a".repeat(64),
  });
  const manifest: Manifest = {
    format: "maple-agent-update-v1",
    product: "maple-agent",
    channel: "stable",
    version: "0.0.2",
    build: "2",
    source_sha: "b".repeat(40),
    pub_date: "2026-10-21T12:00:00Z",
    notes: "Preview update\nFixes.",
    artifacts: { "darwin-aarch64": artifact("Maple-Agent.app.tar.gz") },
    downloads: { macos: artifact("Maple-Agent.dmg") },
  };
  if (linux)
    manifest.artifacts["linux-x86_64"] = manifest.downloads["linux-appimage"] =
      artifact("Maple-Agent.AppImage");
  return { manifest, bytes: encodeFixture(manifest) };
}
export function encodeFixture(manifest: Manifest): Uint8Array<ArrayBuffer> {
  const signature = btoa("ED" + "\0".repeat(72));
  return new TextEncoder().encode(
    JSON.stringify({
      manifest: JSON.stringify(manifest),
      signature: `untrusted comment: fixture\n${signature}\ntrusted comment: product=maple-agent channel=stable version=${manifest.version}\n${btoa("\0".repeat(64))}\n`,
    }),
  );
}
