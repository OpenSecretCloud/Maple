import type { TransportV2SessionInfo } from "./client";
import { transportV2Client } from "./client";
import {
  commitTransportV2AuthBundleImport,
  exportTransportV2AuthBundle as exportStoredBundle,
  prepareTransportV2AuthBundleImport,
  snapshotTransportV2Auth
} from "./auth";

/**
 * Exports the current user resumption descriptors and stable cache namespace
 * root as a portable, origin-bound Transport V2 bundle for trusted local IPC.
 * The result is encoded, not encrypted, and must never be placed in a URL,
 * log, clipboard, or other untrusted transport.
 */
export async function exportTransportV2AuthBundle(apiUrl: string): Promise<string> {
  return exportStoredBundle(apiUrl);
}

/**
 * Installs a sensitive Transport V2 user auth bundle received through trusted
 * local IPC for the exact configured API URL. The imported resumption
 * credential remains authoritative; no client-provided user identifier is
 * trusted.
 */
export async function importTransportV2AuthBundle(bundle: string, apiUrl: string): Promise<void> {
  const expected = snapshotTransportV2Auth(apiUrl, "user");
  const prepared = prepareTransportV2AuthBundleImport(bundle, apiUrl);
  transportV2Client.retireAuthenticationState(prepared.apiOrigin, "user");
  commitTransportV2AuthBundleImport(prepared, expected);
}

export type { TransportV2SessionInfo };
