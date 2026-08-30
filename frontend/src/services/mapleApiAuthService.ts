import { isTauriDesktop } from "@/utils/platform";
import { exportTransportV2AuthBundle, importTransportV2AuthBundle } from "@opensecret/react";

export interface MapleApiAuthSnapshot {
  userId: string;
  authBundle: string;
  nativeInstanceId: string;
  revision: number;
}

export interface MapleApiAuthChanged {
  userId: string;
  revision: number;
  authenticated: boolean;
}

export interface MapleApiAuthInvalidated {
  userId: string;
}

export interface MapleApiAuthMetadata {
  userId: string;
  nativeInstanceId: string;
  nativeRevision: number;
  bundleFingerprint: string;
}

interface SyncedAuth {
  userId: string;
  authBundle: string;
  nativeInstanceId: string;
  revision: number;
}

const AUTH_CHANGED_EVENT = "maple-api-auth-changed";
const AUTH_METADATA_KEY = "maple_api_auth_sync_v2";
const LEGACY_AUTH_METADATA_KEY = "maple_api_auth_sync_v1";
const MAX_SYNC_ATTEMPTS = 3;

function normalizeUserId(userId: string): string {
  const normalized = userId.trim().toLowerCase();
  if (!normalized) throw new Error("Maple API access requires a signed-in account");
  return normalized;
}

function readBrowserMetadata(): MapleApiAuthMetadata | null {
  localStorage.removeItem(LEGACY_AUTH_METADATA_KEY);
  const encoded = localStorage.getItem(AUTH_METADATA_KEY);
  if (!encoded) return null;
  try {
    const metadata = JSON.parse(encoded) as Partial<MapleApiAuthMetadata>;
    if (
      typeof metadata.userId !== "string" ||
      typeof metadata.nativeInstanceId !== "string" ||
      !metadata.nativeInstanceId ||
      typeof metadata.nativeRevision !== "number" ||
      !Number.isSafeInteger(metadata.nativeRevision) ||
      metadata.nativeRevision < 1 ||
      typeof metadata.bundleFingerprint !== "string" ||
      !metadata.bundleFingerprint
    ) {
      return null;
    }
    return metadata as MapleApiAuthMetadata;
  } catch {
    return null;
  }
}

function writeBrowserMetadata(metadata: MapleApiAuthMetadata | null): void {
  if (metadata) {
    localStorage.setItem(AUTH_METADATA_KEY, JSON.stringify(metadata));
  } else {
    localStorage.removeItem(AUTH_METADATA_KEY);
  }
}

// This non-cryptographic fingerprint only detects whether another SDK changed
// the opaque browser bundle across a WebView reload. Account identity remains
// authoritative only after the native client validates it with the backend.
function bundleFingerprint(bundle: string): string {
  let hash = 0xcbf29ce484222325n;
  const bytes = new TextEncoder().encode(bundle);
  for (const byte of bytes) {
    hash ^= BigInt(byte);
    hash = BigInt.asUintN(64, hash * 0x100000001b3n);
  }
  return hash.toString(16).padStart(16, "0");
}

async function invokeNative<T>(command: string, args: Record<string, unknown>): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  return await invoke<T>(command, args);
}

export interface MapleApiAuthBridge {
  isDesktop(): boolean;
  apiUrl(): string;
  exportAuthBundle(): Promise<string>;
  importAuthBundle(bundle: string): Promise<void>;
  readMetadata(): MapleApiAuthMetadata | null;
  writeMetadata(metadata: MapleApiAuthMetadata | null): void;
  invoke<T>(command: string, args: Record<string, unknown>): Promise<T>;
  listen(handler: (event: MapleApiAuthChanged) => Promise<void>): Promise<void>;
}

const defaultBridge: MapleApiAuthBridge = {
  isDesktop: isTauriDesktop,
  apiUrl: () => import.meta.env.VITE_OPEN_SECRET_API_URL,
  exportAuthBundle: () => exportTransportV2AuthBundle(import.meta.env.VITE_OPEN_SECRET_API_URL),
  importAuthBundle: (bundle) =>
    importTransportV2AuthBundle(bundle, import.meta.env.VITE_OPEN_SECRET_API_URL),
  readMetadata: readBrowserMetadata,
  writeMetadata: writeBrowserMetadata,
  invoke: invokeNative,
  async listen(handler) {
    const { listen } = await import("@tauri-apps/api/event");
    await listen<MapleApiAuthChanged>(AUTH_CHANGED_EVENT, (event) => {
      void handler(event.payload);
    });
  }
};

export class MapleApiAuthService {
  private activeUserId: string | null = null;
  private syncedAuth: SyncedAuth | null = null;
  private listenerPromise: Promise<void> | null = null;
  private operationTail: Promise<void> = Promise.resolve();
  private readonly invalidationHandlers = new Set<(event: MapleApiAuthInvalidated) => void>();

  constructor(private readonly bridge: MapleApiAuthBridge = defaultBridge) {}

  async activate(userId: string): Promise<void> {
    if (!this.bridge.isDesktop()) return;
    const normalizedUserId = normalizeUserId(userId);
    await this.ensureListener();
    await this.enqueue(async () => {
      this.activeUserId = normalizedUserId;
      if (this.syncedAuth?.userId !== normalizedUserId) this.syncedAuth = null;
      try {
        await this.reconcileActivationNow(normalizedUserId);
      } catch (error) {
        if (this.activeUserId === normalizedUserId) {
          this.activeUserId = null;
          this.syncedAuth = null;
        }
        throw error;
      }
    });
  }

  async sync(userId: string, force = false): Promise<void> {
    if (!this.bridge.isDesktop()) return;
    const normalizedUserId = normalizeUserId(userId);
    await this.enqueue(() => this.syncNow(normalizedUserId, force));
  }

  async clear(userId: string): Promise<void> {
    if (!this.bridge.isDesktop()) return;
    const normalizedUserId = normalizeUserId(userId);
    await this.enqueue(async () => {
      await this.bridge.invoke<void>("maple_api_clear_auth", { userId: normalizedUserId });
      if (this.activeUserId === normalizedUserId) {
        this.activeUserId = null;
        this.syncedAuth = null;
      }
      if (this.bridge.readMetadata()?.userId === normalizedUserId) {
        this.bridge.writeMetadata(null);
      }
    });
  }

  subscribeInvalidation(handler: (event: MapleApiAuthInvalidated) => void): () => void {
    this.invalidationHandlers.add(handler);
    return () => this.invalidationHandlers.delete(handler);
  }

  private enqueue<T>(operation: () => Promise<T>): Promise<T> {
    const result = this.operationTail.then(operation, operation);
    this.operationTail = result.then(
      () => undefined,
      () => undefined
    );
    return result;
  }

  private async ensureListener(): Promise<void> {
    if (this.listenerPromise) return await this.listenerPromise;
    const attempt = this.bridge.listen(async (event) => {
      if (!event.authenticated) {
        await this.enqueue(() => this.invalidateNativeAuthNow(event.userId));
        return;
      }
      try {
        await this.enqueue(() => this.reconcileRefreshNow(event));
      } catch (error) {
        console.warn("Maple could not reconcile refreshed API credentials", error);
      }
    });
    this.listenerPromise = attempt;
    try {
      await attempt;
    } catch (error) {
      if (this.listenerPromise === attempt) this.listenerPromise = null;
      throw error;
    }
  }

  private async invalidateNativeAuthNow(eventUserId: string): Promise<void> {
    const userId = this.activeUserId;
    if (!userId || normalizeUserId(eventUserId) !== userId) return;

    // A browser refresh can win while an older native generation is failing.
    // Reinstall that exact newer opaque bundle instead of signing out the
    // matching account. If it cannot validate, fall through to fail closed.
    try {
      const browserBundle = await this.bridge.exportAuthBundle();
      if (this.syncedAuth?.userId === userId && browserBundle !== this.syncedAuth.authBundle) {
        await this.syncNow(userId, true);
        return;
      }
    } catch {
      // Browser credentials are absent or unreadable; native cleanup and the
      // UI invalidation notification remain mandatory.
    }

    try {
      await this.bridge.invoke<void>("maple_api_clear_auth", { userId });
    } finally {
      if (this.activeUserId === userId) {
        this.activeUserId = null;
        this.syncedAuth = null;
        if (this.bridge.readMetadata()?.userId === userId) {
          this.bridge.writeMetadata(null);
        }
        for (const handler of this.invalidationHandlers) {
          try {
            handler({ userId });
          } catch {
            // One UI observer cannot prevent the remaining account lifecycle
            // observers from receiving this fail-closed transition.
          }
        }
      }
    }
  }

  private async reconcileActivationNow(userId: string): Promise<void> {
    let snapshot: MapleApiAuthSnapshot;
    try {
      snapshot = await this.bridge.invoke<MapleApiAuthSnapshot>("maple_api_get_auth", { userId });
    } catch {
      await this.syncNow(userId, true);
      return;
    }
    this.assertCurrentSnapshot(userId, snapshot);

    // Read after the native await so a concurrent browser refresh wins unless
    // durable metadata proves the native session advanced from this exact bundle.
    const browserBundle = await this.bridge.exportAuthBundle();
    const metadata = this.bridge.readMetadata();
    if (browserBundle === snapshot.authBundle) {
      await this.acceptSnapshot(snapshot, false);
      return;
    }

    const browserMatchesLastAcknowledgedNative =
      metadata?.userId === userId &&
      metadata.nativeInstanceId === snapshot.nativeInstanceId &&
      metadata.bundleFingerprint === bundleFingerprint(browserBundle);
    if (
      browserMatchesLastAcknowledgedNative &&
      snapshot.revision > (metadata?.nativeRevision ?? 0)
    ) {
      await this.acceptSnapshot(snapshot, true);
      return;
    }

    await this.syncNow(userId, true);
  }

  private async syncNow(userId: string, force: boolean): Promise<void> {
    if (this.activeUserId !== userId) {
      throw new Error("Maple API authentication changed before the operation started");
    }

    for (let attempt = 0; attempt < MAX_SYNC_ATTEMPTS; attempt += 1) {
      const authBundle = await this.bridge.exportAuthBundle();
      if (
        !force &&
        this.syncedAuth?.userId === userId &&
        authBundle === this.syncedAuth.authBundle
      ) {
        return;
      }

      const snapshot = await this.bridge.invoke<MapleApiAuthSnapshot>("maple_api_set_auth", {
        request: {
          userId,
          apiUrl: this.bridge.apiUrl(),
          authBundle
        }
      });
      this.assertCurrentSnapshot(userId, snapshot);

      // The browser SDK can rotate its bundle while native candidate validation
      // is in flight. Retry that newer bundle before allowing the Agent command
      // waiting on this sync to continue.
      if ((await this.bridge.exportAuthBundle()) !== authBundle) {
        force = true;
        continue;
      }

      if (authBundle !== snapshot.authBundle) {
        // Candidate validation may itself rotate a descriptor or resumption
        // credential. Import the native SDK's complete opaque replacement.
        await this.bridge.importAuthBundle(snapshot.authBundle);
      }
      await this.acceptSnapshot(snapshot, false);
      return;
    }

    throw new Error("Maple API credentials changed repeatedly during synchronization");
  }

  private async reconcileRefreshNow(event: MapleApiAuthChanged): Promise<void> {
    const userId = this.activeUserId;
    if (!userId || normalizeUserId(event.userId) !== userId) return;

    const browserBundle = await this.bridge.exportAuthBundle();
    const synced = this.syncedAuth;
    if (!synced || synced.userId !== userId || browserBundle !== synced.authBundle) {
      // The browser refreshed independently. Its current session remains
      // canonical, so install that pair instead of consuming a late native
      // refresh notification.
      await this.syncNow(userId, true);
      return;
    }
    if (event.revision <= synced.revision) return;

    const snapshot = await this.bridge.invoke<MapleApiAuthSnapshot>("maple_api_get_auth", {
      userId
    });
    if (this.activeUserId !== userId) return;

    // Re-read both sources after the await. Otherwise a browser rotation that
    // happened during get_auth could be overwritten by this stale snapshot.
    const latestBrowserBundle = await this.bridge.exportAuthBundle();
    const latestSynced = this.syncedAuth;
    if (
      !latestSynced ||
      latestSynced.userId !== userId ||
      latestBrowserBundle !== latestSynced.authBundle
    ) {
      await this.syncNow(userId, true);
      return;
    }

    this.assertCurrentSnapshot(userId, snapshot);
    if (snapshot.nativeInstanceId !== latestSynced.nativeInstanceId) {
      await this.syncNow(userId, true);
      return;
    }
    if (snapshot.revision < latestSynced.revision) return;
    await this.acceptSnapshot(snapshot, true);
  }

  private assertCurrentSnapshot(userId: string, snapshot: MapleApiAuthSnapshot): void {
    if (
      this.activeUserId !== userId ||
      normalizeUserId(snapshot.userId) !== userId ||
      !snapshot.nativeInstanceId ||
      !snapshot.authBundle ||
      !Number.isSafeInteger(snapshot.revision) ||
      snapshot.revision < 1
    ) {
      throw new Error("Maple API authentication changed while credentials were being installed");
    }
  }

  private async acceptSnapshot(
    snapshot: MapleApiAuthSnapshot,
    importBundle: boolean
  ): Promise<void> {
    if (importBundle) await this.bridge.importAuthBundle(snapshot.authBundle);
    this.syncedAuth = {
      userId: normalizeUserId(snapshot.userId),
      authBundle: snapshot.authBundle,
      nativeInstanceId: snapshot.nativeInstanceId,
      revision: snapshot.revision
    };
    this.bridge.writeMetadata({
      userId: normalizeUserId(snapshot.userId),
      nativeInstanceId: snapshot.nativeInstanceId,
      nativeRevision: snapshot.revision,
      bundleFingerprint: bundleFingerprint(snapshot.authBundle)
    });
  }
}

export const mapleApiAuthService = new MapleApiAuthService();
