import { isTauriDesktop } from "@/utils/platform";

export interface MapleApiAuthSnapshot {
  userId: string;
  accessToken: string;
  refreshToken?: string | null;
  nativeInstanceId: string;
  revision: number;
}

export interface MapleApiAuthChanged {
  userId: string;
  revision: number;
}

export interface BrowserTokenPair {
  accessToken: string;
  refreshToken: string | null;
}

export interface MapleApiAuthMetadata {
  userId: string;
  nativeInstanceId: string;
  nativeRevision: number;
  tokenFingerprint: string;
}

interface SyncedAuth extends BrowserTokenPair {
  userId: string;
  nativeInstanceId: string;
  revision: number;
}

const AUTH_CHANGED_EVENT = "maple-api-auth-changed";
const AUTH_METADATA_KEY = "maple_api_auth_sync_v1";
const MAX_SYNC_ATTEMPTS = 3;

function normalizeUserId(userId: string): string {
  const normalized = userId.trim().toLowerCase();
  if (!normalized) throw new Error("Maple API access requires a signed-in account");
  return normalized;
}

function readBrowserTokens(): BrowserTokenPair {
  const accessToken = localStorage.getItem("access_token")?.trim() || "";
  if (!accessToken) {
    throw new Error("Maple API access requires a signed-in session");
  }
  return {
    accessToken,
    refreshToken: localStorage.getItem("refresh_token")?.trim() || null
  };
}

function writeBrowserTokens(tokens: BrowserTokenPair): void {
  localStorage.setItem("access_token", tokens.accessToken);
  if (tokens.refreshToken) {
    localStorage.setItem("refresh_token", tokens.refreshToken);
  } else {
    localStorage.removeItem("refresh_token");
  }
}

function readBrowserMetadata(): MapleApiAuthMetadata | null {
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
      typeof metadata.tokenFingerprint !== "string" ||
      !metadata.tokenFingerprint
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

function sameTokens(left: BrowserTokenPair, right: BrowserTokenPair): boolean {
  return left.accessToken === right.accessToken && left.refreshToken === right.refreshToken;
}

// This fingerprint only detects whether another SDK changed the browser pair
// across a WebView reload. Account identity is always verified by the backend
// before native credentials are published.
function tokenFingerprint(tokens: BrowserTokenPair): string {
  let hash = 0xcbf29ce484222325n;
  const bytes = new TextEncoder().encode(`${tokens.accessToken}\u0000${tokens.refreshToken ?? ""}`);
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
  readTokens(): BrowserTokenPair;
  writeTokens(tokens: BrowserTokenPair): void;
  readMetadata(): MapleApiAuthMetadata | null;
  writeMetadata(metadata: MapleApiAuthMetadata | null): void;
  invoke<T>(command: string, args: Record<string, unknown>): Promise<T>;
  listen(handler: (event: MapleApiAuthChanged) => Promise<void>): Promise<void>;
}

const defaultBridge: MapleApiAuthBridge = {
  isDesktop: isTauriDesktop,
  apiUrl: () => import.meta.env.VITE_OPEN_SECRET_API_URL,
  readTokens: readBrowserTokens,
  writeTokens: writeBrowserTokens,
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
    // durable metadata proves the native session advanced from this exact pair.
    const browserTokens = this.bridge.readTokens();
    const nativeTokens = this.snapshotTokens(snapshot);
    const metadata = this.bridge.readMetadata();
    if (sameTokens(browserTokens, nativeTokens)) {
      this.acceptSnapshot(snapshot, false);
      return;
    }

    const browserMatchesLastAcknowledgedNative =
      metadata?.userId === userId &&
      metadata.nativeInstanceId === snapshot.nativeInstanceId &&
      metadata.tokenFingerprint === tokenFingerprint(browserTokens);
    if (
      browserMatchesLastAcknowledgedNative &&
      snapshot.revision > (metadata?.nativeRevision ?? 0)
    ) {
      this.acceptSnapshot(snapshot, true);
      return;
    }

    await this.syncNow(userId, true);
  }

  private async syncNow(userId: string, force: boolean): Promise<void> {
    if (this.activeUserId !== userId) {
      throw new Error("Maple API authentication changed before the operation started");
    }

    for (let attempt = 0; attempt < MAX_SYNC_ATTEMPTS; attempt += 1) {
      const tokens = this.bridge.readTokens();
      if (!force && this.syncedAuth?.userId === userId && sameTokens(tokens, this.syncedAuth)) {
        return;
      }

      const snapshot = await this.bridge.invoke<MapleApiAuthSnapshot>("maple_api_set_auth", {
        request: {
          userId,
          apiUrl: this.bridge.apiUrl(),
          accessToken: tokens.accessToken,
          refreshToken: tokens.refreshToken
        }
      });
      this.assertCurrentSnapshot(userId, snapshot);

      // The browser SDK can rotate its pair while native candidate validation
      // is in flight. Retry that newer pair before allowing the Agent command
      // waiting on this sync to continue.
      if (!sameTokens(this.bridge.readTokens(), tokens)) {
        force = true;
        continue;
      }

      const acceptedTokens = this.snapshotTokens(snapshot);
      if (!sameTokens(tokens, acceptedTokens)) {
        // Candidate validation may itself refresh an expired JWT.
        this.bridge.writeTokens(acceptedTokens);
      }
      this.acceptSnapshot(snapshot, false);
      return;
    }

    throw new Error("Maple API credentials changed repeatedly during synchronization");
  }

  private async reconcileRefreshNow(event: MapleApiAuthChanged): Promise<void> {
    const userId = this.activeUserId;
    if (!userId || normalizeUserId(event.userId) !== userId) return;

    const browserTokens = this.bridge.readTokens();
    const synced = this.syncedAuth;
    if (!synced || synced.userId !== userId || !sameTokens(browserTokens, synced)) {
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
    const latestBrowserTokens = this.bridge.readTokens();
    const latestSynced = this.syncedAuth;
    if (
      !latestSynced ||
      latestSynced.userId !== userId ||
      !sameTokens(latestBrowserTokens, latestSynced)
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
    this.acceptSnapshot(snapshot, true);
  }

  private assertCurrentSnapshot(userId: string, snapshot: MapleApiAuthSnapshot): void {
    if (
      this.activeUserId !== userId ||
      normalizeUserId(snapshot.userId) !== userId ||
      !snapshot.nativeInstanceId ||
      !Number.isSafeInteger(snapshot.revision) ||
      snapshot.revision < 1
    ) {
      throw new Error("Maple API authentication changed while credentials were being installed");
    }
  }

  private snapshotTokens(snapshot: MapleApiAuthSnapshot): BrowserTokenPair {
    return {
      accessToken: snapshot.accessToken,
      refreshToken: snapshot.refreshToken || null
    };
  }

  private acceptSnapshot(snapshot: MapleApiAuthSnapshot, writeTokens: boolean): void {
    const tokens = this.snapshotTokens(snapshot);
    if (writeTokens) this.bridge.writeTokens(tokens);
    this.syncedAuth = {
      userId: normalizeUserId(snapshot.userId),
      ...tokens,
      nativeInstanceId: snapshot.nativeInstanceId,
      revision: snapshot.revision
    };
    this.bridge.writeMetadata({
      userId: normalizeUserId(snapshot.userId),
      nativeInstanceId: snapshot.nativeInstanceId,
      nativeRevision: snapshot.revision,
      tokenFingerprint: tokenFingerprint(tokens)
    });
  }
}

export const mapleApiAuthService = new MapleApiAuthService();
