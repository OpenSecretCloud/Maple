import {
  MAX_AGENT_HISTORY_RECORD_PRESENTATION_BYTES,
  type AgentHistoryRecord,
  type AgentSessionRecordsPage,
  type AgentTimelineItem
} from "./agentRuntimeService";

export type AgentHistoryPageKind = "head" | "older";

export interface AgentHistoryPageToken {
  readonly sessionId: string;
  readonly kind: AgentHistoryPageKind;
  readonly cursor: string | null;
  readonly lifecycleGeneration: number;
  readonly stateInstanceId: number;
  readonly cacheEpoch: number;
  readonly requestId: number;
  readonly eventSequence: number | null;
  readonly eventStateRevision: number;
}

export interface AgentHistorySnapshot {
  readonly records: readonly AgentHistoryRecord[];
  readonly timeline: readonly AgentTimelineItem[];
  readonly nextCursor: string | null;
  readonly historyRevision: string | null;
  readonly headLoaded: boolean;
  readonly isLoading: boolean;
  readonly hasMore: boolean;
  readonly requiresSynchronizedReload: boolean;
}

export type AgentHistoryCommitResult = "applied" | "stale" | "history-replaced";
export type AgentEventAcceptance = "accepted" | "duplicate" | "gap" | "invalid";
export type AgentHistoryOwnerBindResult = "unchanged" | "reset";
export type AgentLiveMergeResult = "applied" | "synchronized-reload-required";

export interface AgentHistoryOwner {
  readonly accountId: string;
  readonly targetId: string;
}

export interface AgentLiveSessionSnapshot {
  readonly sessionId: string;
  readonly liveItems: readonly AgentTimelineItem[];
}

export interface AgentSynchronizedLiveSnapshot {
  readonly liveSessionsComplete: true;
  readonly liveSessionCount: number;
  readonly liveSessions: readonly AgentLiveSessionSnapshot[];
  readonly throughEventCursor: { readonly journalId: string; readonly sequence: number };
}

interface LiveTimelineItem {
  item: AgentTimelineItem;
  deltaOnly: boolean;
  budgetBytes: number;
  textBytes: number;
}

interface SessionHistoryState {
  stateInstanceId: number;
  records: AgentHistoryRecord[];
  nextCursor: string | null;
  historyRevision: string | null;
  headLoaded: boolean;
  cacheEpoch: number;
  nextRequestId: number;
  activeRequestId: number | null;
  persistedProjectionBytes: number;
  retentionOrdinal: number;
  liveItemOrder: string[];
  liveItems: Map<string, LiveTimelineItem>;
  liveProjectionBytes: number;
  projectedTimeline: AgentTimelineItem[];
  projectedIndexById: Map<string, number>;
  authoritativeLiveSuffix: boolean;
  requiresSynchronizedReload: boolean;
}

interface RetainedStateAdmissionPlan {
  readonly requiredSessionIds: readonly string[];
  readonly evictSessionIds: readonly string[];
  readonly createSessionIds: readonly string[];
}

interface SynchronizedCommitAdmission {
  readonly accountLiveProjectionBytes: number;
  readonly protectedSessionIds: ReadonlySet<string>;
  readonly targetLiveProjectionBytes: number;
  readonly targetLiveItemOrder: readonly string[];
  readonly targetLiveItems: ReadonlyMap<string, LiveTimelineItem>;
  readonly targetAuthoritativeLiveSuffix: boolean;
}

export const MAX_AGENT_LIVE_ITEMS_PER_SESSION = 200;
export const MAX_AGENT_LIVE_SESSIONS_PER_ACCOUNT = 64;
export const MAX_AGENT_LIVE_ITEMS_PER_ACCOUNT = 512;
export const MAX_AGENT_LIVE_PROJECTION_BYTES_PER_ITEM = 192 * 1024;
// Native currently permits one session to consume the whole bounded account
// projection. Track the session total independently without inventing a lower
// frontend-only ceiling that a valid synchronized snapshot could never load.
export const MAX_AGENT_LIVE_PROJECTION_BYTES_PER_SESSION = 8 * 1024 * 1024;
export const MAX_AGENT_LIVE_PROJECTION_BYTES_PER_ACCOUNT = 8 * 1024 * 1024;
export const MAX_AGENT_HISTORY_PAGE_PROJECTION_BYTES = 8 * 1024 * 1024;
export const MAX_AGENT_HISTORY_SESSION_PROJECTION_BYTES = 16 * 1024 * 1024;
export const MAX_AGENT_HISTORY_ACCOUNT_PROJECTION_BYTES = 32 * 1024 * 1024;
export const MAX_AGENT_HISTORY_RETAINED_SESSIONS_PER_ACCOUNT = 128;
export const MAX_AGENT_RETIRED_EVENT_EPOCHS = 8;

const AGENT_LIVE_ITEM_PROJECTION_OVERHEAD_BYTES = 256;
const AGENT_LIVE_UTF8_ENCODER = new TextEncoder();

const EMPTY_AGENT_HISTORY_SNAPSHOT: AgentHistorySnapshot = Object.freeze({
  records: Object.freeze([]),
  timeline: Object.freeze([]),
  nextCursor: null,
  historyRevision: null,
  headLoaded: false,
  isLoading: false,
  hasMore: false,
  requiresSynchronizedReload: false
});

function newSessionHistoryState(
  stateInstanceId: number,
  retentionOrdinal: number
): SessionHistoryState {
  return {
    stateInstanceId,
    records: [],
    nextCursor: null,
    historyRevision: null,
    headLoaded: false,
    cacheEpoch: 0,
    nextRequestId: 0,
    activeRequestId: null,
    persistedProjectionBytes: 0,
    retentionOrdinal,
    liveItemOrder: [],
    liveItems: new Map(),
    liveProjectionBytes: 0,
    projectedTimeline: [],
    projectedIndexById: new Map(),
    authoritativeLiveSuffix: false,
    requiresSynchronizedReload: false
  };
}

function utf8ByteLength(value: string | null | undefined): number {
  return value ? AGENT_LIVE_UTF8_ENCODER.encode(value).byteLength : 0;
}

function compareUtf8Bytes(left: string, right: string): number {
  const leftBytes = AGENT_LIVE_UTF8_ENCODER.encode(left);
  const rightBytes = AGENT_LIVE_UTF8_ENCODER.encode(right);
  const commonLength = Math.min(leftBytes.length, rightBytes.length);
  for (let index = 0; index < commonLength; index += 1) {
    const difference = leftBytes[index] - rightBytes[index];
    if (difference !== 0) return difference;
  }
  return leftBytes.length - rightBytes.length;
}

function timelineItemBudget(item: AgentTimelineItem): {
  readonly budgetBytes: number;
  readonly textBytes: number;
} {
  const textBytes = utf8ByteLength(item.text);
  return {
    textBytes,
    budgetBytes:
      AGENT_LIVE_ITEM_PROJECTION_OVERHEAD_BYTES +
      utf8ByteLength(item.id) +
      utf8ByteLength(item.itemType) +
      utf8ByteLength(item.role) +
      utf8ByteLength(item.title) +
      textBytes +
      utf8ByteLength(item.status) +
      utf8ByteLength(item.merge)
  };
}

function mergedTimelineItemBudget(
  previous: AgentTimelineItem,
  previousTextBytes: number,
  incoming: AgentTimelineItem
): { readonly budgetBytes: number; readonly textBytes: number } {
  const appendText =
    incoming.merge === "append" &&
    (incoming.itemType === "message" || incoming.itemType === "thinking") &&
    incoming.text !== undefined &&
    incoming.text !== null;
  const textBytes = appendText
    ? previousTextBytes + utf8ByteLength(incoming.text)
    : utf8ByteLength(incoming.text ?? previous.text);
  const mergedWithoutText = {
    ...previous,
    ...incoming,
    title: incoming.title ?? previous.title,
    text: undefined
  };
  return {
    textBytes,
    budgetBytes:
      AGENT_LIVE_ITEM_PROJECTION_OVERHEAD_BYTES +
      utf8ByteLength(mergedWithoutText.id) +
      utf8ByteLength(mergedWithoutText.itemType) +
      utf8ByteLength(mergedWithoutText.role) +
      utf8ByteLength(mergedWithoutText.title) +
      textBytes +
      utf8ByteLength(mergedWithoutText.status) +
      utf8ByteLength(mergedWithoutText.merge)
  };
}

function historyRecordBudgetBytes(record: AgentHistoryRecord): number {
  let budgetBytes = 512 + utf8ByteLength(record.recordId) + utf8ByteLength(record.role);
  for (const item of record.items) {
    budgetBytes += timelineItemBudget(item).budgetBytes;
    if (budgetBytes > MAX_AGENT_HISTORY_RECORD_PRESENTATION_BYTES) {
      throw new AgentHistoryProjectionLimitError();
    }
  }
  return budgetBytes;
}

function historyRecordsBudgetBytes(
  records: readonly AgentHistoryRecord[],
  maximumBytes: number
): number {
  let budgetBytes = 0;
  for (const record of records) {
    budgetBytes += historyRecordBudgetBytes(record);
    if (budgetBytes > maximumBytes) throw new AgentHistoryProjectionLimitError();
  }
  return budgetBytes;
}

function mergeTimelineItem(
  previous: AgentTimelineItem,
  incoming: AgentTimelineItem
): AgentTimelineItem {
  const appendText =
    incoming.merge === "append" &&
    (incoming.itemType === "message" || incoming.itemType === "thinking") &&
    incoming.text !== undefined &&
    incoming.text !== null;

  return {
    ...previous,
    ...incoming,
    title: incoming.title ?? previous.title,
    input: incoming.input ?? previous.input,
    output: incoming.output ?? previous.output,
    text: appendText
      ? `${previous.text || ""}${incoming.text || ""}`
      : (incoming.text ?? previous.text)
  };
}

export function mergeAgentTimelineItems(
  current: readonly AgentTimelineItem[],
  incoming: AgentTimelineItem
): AgentTimelineItem[] {
  const index = current.findIndex((item) => item.id === incoming.id);
  if (index < 0) return [...current, incoming];

  const next = [...current];
  next[index] = mergeTimelineItem(next[index], incoming);
  return next;
}

function mergeProjectedItem(
  timeline: AgentTimelineItem[],
  indexById: Map<string, number>,
  budgetById: Map<string, { readonly budgetBytes: number; readonly textBytes: number }>,
  totalBytes: number,
  incoming: AgentTimelineItem,
  maximumBytes: number
): number {
  const index = indexById.get(incoming.id);
  if (index === undefined) {
    const budget = timelineItemBudget(incoming);
    if (
      budget.budgetBytes > MAX_AGENT_HISTORY_RECORD_PRESENTATION_BYTES ||
      totalBytes + budget.budgetBytes > maximumBytes
    ) {
      throw new AgentHistoryProjectionLimitError();
    }
    indexById.set(incoming.id, timeline.length);
    timeline.push(incoming);
    budgetById.set(incoming.id, budget);
    return totalBytes + budget.budgetBytes;
  }
  const previous = timeline[index];
  const previousBudget = budgetById.get(incoming.id) ?? timelineItemBudget(previous);
  const budget = mergedTimelineItemBudget(previous, previousBudget.textBytes, incoming);
  const nextTotalBytes = totalBytes - previousBudget.budgetBytes + budget.budgetBytes;
  if (
    budget.budgetBytes > MAX_AGENT_HISTORY_RECORD_PRESENTATION_BYTES ||
    nextTotalBytes > maximumBytes
  ) {
    throw new AgentHistoryProjectionLimitError();
  }
  timeline[index] = mergeTimelineItem(previous, incoming);
  budgetById.set(incoming.id, budget);
  return nextTotalBytes;
}

function projectedItems(
  records: readonly AgentHistoryRecord[],
  liveItemOrder: readonly string[],
  liveItems: ReadonlyMap<string, LiveTimelineItem>,
  authoritativeLiveSuffix: boolean,
  maximumBytes = MAX_AGENT_HISTORY_SESSION_PROJECTION_BYTES
): { timeline: AgentTimelineItem[]; indexById: Map<string, number>; budgetBytes: number } {
  let timeline: AgentTimelineItem[] = [];
  let indexById = new Map<string, number>();
  let budgetById = new Map<string, { readonly budgetBytes: number; readonly textBytes: number }>();
  let budgetBytes = 0;
  for (const record of records) {
    for (const item of record.items) {
      budgetBytes = mergeProjectedItem(
        timeline,
        indexById,
        budgetById,
        budgetBytes,
        item,
        maximumBytes
      );
    }
  }
  if (authoritativeLiveSuffix) {
    const suffix = liveItemOrder
      .map((itemId) => liveItems.get(itemId)?.item)
      .filter((item): item is AgentTimelineItem => item !== undefined);
    for (const item of suffix) {
      if (item.itemType !== "message" || item.role !== "user") continue;
      const persistedBoundary = indexById.get(item.id);
      if (persistedBoundary !== undefined) {
        timeline = timeline.slice(0, persistedBoundary);
        indexById = new Map(timeline.map((persistedItem, index) => [persistedItem.id, index]));
        budgetById = new Map(timeline.map((item) => [item.id, timelineItemBudget(item)]));
        budgetBytes = [...budgetById.values()].reduce(
          (total, budget) => total + budget.budgetBytes,
          0
        );
        break;
      }
    }
    for (const item of suffix) {
      budgetBytes = mergeProjectedItem(
        timeline,
        indexById,
        budgetById,
        budgetBytes,
        { ...item, merge: "replace" },
        maximumBytes
      );
    }
    return { timeline, indexById, budgetBytes };
  }
  for (const itemId of liveItemOrder) {
    const live = liveItems.get(itemId);
    if (live) {
      budgetBytes = mergeProjectedItem(
        timeline,
        indexById,
        budgetById,
        budgetBytes,
        live.deltaOnly ? live.item : { ...live.item, merge: "replace" },
        maximumBytes
      );
    }
  }
  return { timeline, indexById, budgetBytes };
}

function rebuildProjection(state: SessionHistoryState): void {
  const projection = projectedItems(
    state.records,
    state.liveItemOrder,
    state.liveItems,
    state.authoritativeLiveSuffix
  );
  state.projectedTimeline = projection.timeline;
  state.projectedIndexById = projection.indexById;
}

export class AgentHistoryProjectionLimitError extends Error {
  constructor() {
    super("Agent history projection exceeds the bounded frontend window");
    this.name = "AgentHistoryProjectionLimitError";
  }
}

function persistedTimelineItem(
  state: SessionHistoryState,
  itemId: string
): AgentTimelineItem | undefined {
  let persisted: AgentTimelineItem | undefined;
  for (const record of state.records) {
    for (const item of record.items) {
      if (item.id !== itemId) continue;
      persisted = persisted ? mergeTimelineItem(persisted, item) : item;
    }
  }
  return persisted;
}

function updateProjectedLiveItem(
  state: SessionHistoryState,
  live: LiveTimelineItem,
  wasNewLiveItem: boolean
): void {
  if (
    wasNewLiveItem &&
    state.authoritativeLiveSuffix &&
    live.item.itemType === "message" &&
    live.item.role === "user" &&
    state.projectedIndexById.has(live.item.id)
  ) {
    rebuildProjection(state);
    return;
  }

  const index = state.projectedIndexById.get(live.item.id);
  const absoluteItem = live.deltaOnly ? live.item : { ...live.item, merge: "replace" };
  if (index === undefined) {
    state.projectedIndexById.set(live.item.id, state.projectedTimeline.length);
    state.projectedTimeline = [...state.projectedTimeline, absoluteItem];
    return;
  }
  const next = [...state.projectedTimeline];
  next[index] = { ...live.item, merge: "replace" };
  state.projectedTimeline = next;
}

function uniqueRecords(records: readonly AgentHistoryRecord[]): AgentHistoryRecord[] {
  const result: AgentHistoryRecord[] = [];
  const indexById = new Map<string, number>();
  for (const record of records) {
    const existingIndex = indexById.get(record.recordId);
    if (existingIndex === undefined) {
      indexById.set(record.recordId, result.length);
      result.push(record);
    } else {
      result[existingIndex] = record;
    }
  }
  return result;
}

function recordsInChronologicalOrder(page: AgentSessionRecordsPage): AgentHistoryRecord[] {
  // The native page is newest-first. Reverse record containers as units so a
  // record's internal projected-item ordering remains untouched.
  return uniqueRecords([...page.records].reverse());
}

/**
 * Account-owned instances keep bounded, per-session history projections. The
 * cache never interprets native cursors and never turns projected timeline
 * items into pagination units.
 */
export class AgentHistoryPaginationCache {
  private readonly sessions = new Map<string, SessionHistoryState>();
  private owner: AgentHistoryOwner;
  private lifecycleGeneration = 0;
  private nextStateInstanceId = 0;
  private nextRetentionOrdinal = 0;
  private eventEpoch: string | null = null;
  private eventSequence: number | null = null;
  private eventStateRevision = 0;
  private readonly retiredEventEpochs = new Set<string>();
  private readonly retiredEventEpochOrder: string[] = [];
  private accountRequiresSynchronizedReload = false;
  private accountLiveProjectionBytes = 0;
  private accountPersistedProjectionBytes = 0;
  private protectedSessionIds = new Set<string>();

  constructor(owner: AgentHistoryOwner) {
    this.owner = this.validOwner(owner);
  }

  bindOwner(owner: AgentHistoryOwner): AgentHistoryOwnerBindResult {
    const validOwner = this.validOwner(owner);
    if (
      this.owner.accountId === validOwner.accountId &&
      this.owner.targetId === validOwner.targetId
    ) {
      return "unchanged";
    }

    this.owner = validOwner;
    this.resetState();
    return "reset";
  }

  beginHead(sessionId: string): AgentHistoryPageToken {
    return this.begin(sessionId, "head", null);
  }

  beginOlder(sessionId: string): AgentHistoryPageToken | null {
    const state = this.sessions.get(sessionId);
    if (!state) return null;
    if (!state.headLoaded || !state.nextCursor || state.activeRequestId !== null) return null;
    return this.begin(sessionId, "older", state.nextCursor);
  }

  commit(token: AgentHistoryPageToken, page: AgentSessionRecordsPage): AgentHistoryCommitResult {
    return this.commitWithAdmission(token, page);
  }

  private commitWithAdmission(
    token: AgentHistoryPageToken,
    page: AgentSessionRecordsPage,
    admission?: SynchronizedCommitAdmission
  ): AgentHistoryCommitResult {
    if (token.lifecycleGeneration !== this.lifecycleGeneration) return "stale";
    const state = this.sessions.get(token.sessionId);
    if (!state) return "stale";
    if (token.stateInstanceId !== state.stateInstanceId) return "stale";
    if (state.cacheEpoch !== token.cacheEpoch || state.activeRequestId !== token.requestId) {
      return "stale";
    }

    const chronologicalPage = recordsInChronologicalOrder(page);
    // A page remains an indivisible vector of native records. Preflight its
    // complete retained presentation before touching records, cursors, or
    // request state. Counting each row prevents repeated replacement items
    // from collapsing into an artificially small page budget.
    historyRecordsBudgetBytes(chronologicalPage, MAX_AGENT_HISTORY_PAGE_PROJECTION_BYTES);
    let candidateRecords: AgentHistoryRecord[];
    let candidateHistoryRevision = state.historyRevision;
    let candidateHeadLoaded = state.headLoaded;
    let candidateNextCursor = state.nextCursor;
    if (token.kind === "older") {
      if (!state.historyRevision || state.historyRevision !== page.historyRevision) {
        this.invalidate(token.sessionId);
        return "history-replaced";
      }
      const existingIds = new Set(state.records.map((record) => record.recordId));
      candidateRecords = [
        ...chronologicalPage.filter((record) => !existingIds.has(record.recordId)),
        ...state.records
      ];
      candidateNextCursor = page.nextCursor ?? null;
    } else {
      const isFirstHead = !state.headLoaded;
      const revisionChanged =
        state.historyRevision !== null && state.historyRevision !== page.historyRevision;
      const retainedRecords = revisionChanged ? [] : state.records;
      const headRecordIds = new Set(chronologicalPage.map((record) => record.recordId));
      candidateRecords = [
        ...retainedRecords.filter((record) => !headRecordIds.has(record.recordId)),
        ...chronologicalPage
      ];
      candidateHistoryRevision = page.historyRevision;
      candidateHeadLoaded = true;
      if (isFirstHead || revisionChanged) candidateNextCursor = page.nextCursor ?? null;
    }

    const persistedProjectionBytes = historyRecordsBudgetBytes(
      candidateRecords,
      MAX_AGENT_HISTORY_SESSION_PROJECTION_BYTES
    );
    const targetLiveProjectionBytes =
      admission?.targetLiveProjectionBytes ?? state.liveProjectionBytes;
    if (
      persistedProjectionBytes + targetLiveProjectionBytes >
      MAX_AGENT_HISTORY_SESSION_PROJECTION_BYTES
    ) {
      throw new AgentHistoryProjectionLimitError();
    }
    projectedItems(
      candidateRecords,
      [],
      new Map(),
      false,
      MAX_AGENT_HISTORY_SESSION_PROJECTION_BYTES
    );
    projectedItems(
      candidateRecords,
      admission?.targetLiveItemOrder ?? state.liveItemOrder,
      admission?.targetLiveItems ?? state.liveItems,
      admission?.targetAuthoritativeLiveSuffix ?? state.authoritativeLiveSuffix,
      MAX_AGENT_HISTORY_SESSION_PROJECTION_BYTES
    );
    this.admitPersistedProjection(
      token.sessionId,
      state,
      persistedProjectionBytes,
      admission?.accountLiveProjectionBytes ?? this.accountLiveProjectionBytes,
      admission?.protectedSessionIds ?? this.protectedSessionIds
    );

    state.activeRequestId = null;
    this.accountPersistedProjectionBytes +=
      persistedProjectionBytes - state.persistedProjectionBytes;
    state.records = candidateRecords;
    state.nextCursor = candidateNextCursor;
    state.historyRevision = candidateHistoryRevision;
    state.headLoaded = candidateHeadLoaded;
    state.persistedProjectionBytes = persistedProjectionBytes;
    this.touchState(state);
    this.rebaseDeltaOnlyLiveItems(state);
    rebuildProjection(state);
    return "applied";
  }

  /**
   * Trusted attach/head coordinator boundary. A plain page commit only mutates
   * persisted records: it must never imply that an arbitrary page carried an
   * atomic live-journal checkpoint. The coordinator calls this method for the
   * ordered `head -> absolute live@C0 -> replay(C0, C1]` attach sequence.
   */
  installSynchronizedAccountHead(
    token: AgentHistoryPageToken,
    page: AgentSessionRecordsPage,
    snapshot: AgentSynchronizedLiveSnapshot
  ): AgentHistoryCommitResult {
    if (token.kind !== "head") {
      throw new Error("A synchronized Agent history snapshot must be installed from a head page");
    }
    // Any accepted event or competing checkpoint after this request began may
    // be newer than the page's C0. Preserve it and require the coordinator to
    // retry instead of regressing the account-wide watermark or live overlay.
    if (
      token.eventStateRevision !== this.eventStateRevision ||
      token.eventSequence !== this.eventSequence
    ) {
      this.fail(token);
      return "stale";
    }
    const absoluteLiveSessions = this.validateSynchronizedLiveSnapshot(snapshot);
    const synchronizedLiveSessionIds = new Set(
      absoluteLiveSessions.map((liveSession) => liveSession.sessionId)
    );
    this.assertCheckpointCanInstall(snapshot.throughEventCursor);
    const stateBeforeCommit = this.sessions.get(token.sessionId);
    if (!stateBeforeCommit || token.stateInstanceId !== stateBeforeCommit.stateInstanceId) {
      return "stale";
    }
    if (
      token.lifecycleGeneration !== this.lifecycleGeneration ||
      stateBeforeCommit.cacheEpoch !== token.cacheEpoch ||
      stateBeforeCommit.activeRequestId !== token.requestId
    ) {
      return "stale";
    }
    const chronologicalPage = recordsInChronologicalOrder(page);
    const revisionChanged =
      stateBeforeCommit.historyRevision !== null &&
      stateBeforeCommit.historyRevision !== page.historyRevision;
    const retainedRecords = revisionChanged ? [] : stateBeforeCommit.records;
    const headRecordIds = new Set(chronologicalPage.map((record) => record.recordId));
    const candidateHeadRecords = [
      ...retainedRecords.filter((record) => !headRecordIds.has(record.recordId)),
      ...chronologicalPage
    ];
    let snapshotLiveProjectionBytes = 0;
    let targetLiveProjectionBytes = 0;
    let targetLiveItemOrder: readonly string[] = [];
    let targetLiveItems: ReadonlyMap<string, LiveTimelineItem> = new Map();
    for (const liveSession of absoluteLiveSessions) {
      let sessionLiveProjectionBytes = 0;
      const liveItems = new Map(
        liveSession.liveItems.map((item) => {
          const budget = timelineItemBudget(item);
          sessionLiveProjectionBytes += budget.budgetBytes;
          snapshotLiveProjectionBytes += budget.budgetBytes;
          return [item.id, { item, deltaOnly: false, ...budget }] as const;
        })
      );
      const records =
        liveSession.sessionId === token.sessionId
          ? candidateHeadRecords
          : (this.sessions.get(liveSession.sessionId)?.records ?? []);
      const retainedProjectionBytes = historyRecordsBudgetBytes(
        records,
        MAX_AGENT_HISTORY_SESSION_PROJECTION_BYTES
      );
      if (
        retainedProjectionBytes + sessionLiveProjectionBytes >
        MAX_AGENT_HISTORY_SESSION_PROJECTION_BYTES
      ) {
        throw new AgentHistoryProjectionLimitError();
      }
      if (liveSession.sessionId === token.sessionId) {
        targetLiveProjectionBytes = sessionLiveProjectionBytes;
        targetLiveItemOrder = liveSession.liveItems.map((item) => item.id);
        targetLiveItems = liveItems;
      }
      projectedItems(
        records,
        liveSession.liveItems.map((item) => item.id),
        liveItems,
        true,
        MAX_AGENT_HISTORY_SESSION_PROJECTION_BYTES
      );
    }
    const retainedStateAdmissionPlan = this.planRetainedStateAdmission(
      synchronizedLiveSessionIds,
      token.sessionId
    );
    const result = this.commitWithAdmission(token, page, {
      accountLiveProjectionBytes: snapshotLiveProjectionBytes,
      protectedSessionIds: synchronizedLiveSessionIds,
      targetLiveProjectionBytes,
      targetLiveItemOrder,
      targetLiveItems,
      targetAuthoritativeLiveSuffix: true
    });
    if (result !== "applied") return result;

    const synchronizedStates = this.applyRetainedStateAdmissionPlan(retainedStateAdmissionPlan);

    // The snapshot is explicitly complete for this account-target checkpoint:
    // clear every cached overlay first, including sessions absent at C0.
    for (const state of this.sessions.values()) {
      state.liveItemOrder = [];
      state.liveItems.clear();
      state.liveProjectionBytes = 0;
      state.authoritativeLiveSuffix = true;
      state.requiresSynchronizedReload = false;
    }
    this.accountLiveProjectionBytes = 0;
    for (const liveSession of absoluteLiveSessions) {
      const state = synchronizedStates.get(liveSession.sessionId)!;
      for (const item of liveSession.liveItems) {
        const budget = timelineItemBudget(item);
        state.liveItemOrder.push(item.id);
        state.liveItems.set(item.id, { item, deltaOnly: false, ...budget });
        state.liveProjectionBytes += budget.budgetBytes;
        this.accountLiveProjectionBytes += budget.budgetBytes;
      }
    }
    for (const state of this.sessions.values()) rebuildProjection(state);
    this.applyEventCheckpoint(snapshot.throughEventCursor);
    this.accountRequiresSynchronizedReload = false;
    return "applied";
  }

  fail(token: AgentHistoryPageToken): void {
    if (token.lifecycleGeneration !== this.lifecycleGeneration) return;
    const state = this.sessions.get(token.sessionId);
    if (!state) return;
    if (token.stateInstanceId !== state.stateInstanceId) return;
    if (state.cacheEpoch === token.cacheEpoch && state.activeRequestId === token.requestId) {
      state.activeRequestId = null;
    }
  }

  mergeLiveItem(sessionId: string, incoming: AgentTimelineItem): AgentLiveMergeResult {
    if (this.accountRequiresSynchronizedReload) return "synchronized-reload-required";
    let state = this.sessions.get(sessionId);
    if (!state) {
      if (this.liveSessionCount() >= MAX_AGENT_LIVE_SESSIONS_PER_ACCOUNT) {
        this.accountRequiresSynchronizedReload = true;
        return "synchronized-reload-required";
      }
      state = this.createState(sessionId);
    }
    if (state.requiresSynchronizedReload) return "synchronized-reload-required";
    const current = state.liveItems.get(incoming.id);
    if (!current) {
      if (
        state.liveItems.size >= MAX_AGENT_LIVE_ITEMS_PER_SESSION ||
        this.liveItemCount() >= MAX_AGENT_LIVE_ITEMS_PER_ACCOUNT ||
        (state.liveItems.size === 0 &&
          this.liveSessionCount() >= MAX_AGENT_LIVE_SESSIONS_PER_ACCOUNT)
      ) {
        state.requiresSynchronizedReload = true;
        this.accountRequiresSynchronizedReload = true;
        return "synchronized-reload-required";
      }
    }

    let persisted: AgentTimelineItem | undefined;
    if (!current) {
      persisted = persistedTimelineItem(state, incoming.id);
    }
    const previous = current?.item ?? persisted;
    const previousTextBytes =
      current?.textBytes ?? (persisted ? timelineItemBudget(persisted).textBytes : 0);
    const budget = previous
      ? mergedTimelineItemBudget(previous, previousTextBytes, incoming)
      : timelineItemBudget(incoming);
    const previousBudgetBytes = current?.budgetBytes ?? 0;
    const nextSessionBytes = state.liveProjectionBytes - previousBudgetBytes + budget.budgetBytes;
    const nextAccountBytes =
      this.accountLiveProjectionBytes - previousBudgetBytes + budget.budgetBytes;
    if (
      budget.textBytes > MAX_AGENT_LIVE_PROJECTION_BYTES_PER_ITEM ||
      nextSessionBytes > MAX_AGENT_LIVE_PROJECTION_BYTES_PER_SESSION ||
      nextAccountBytes > MAX_AGENT_LIVE_PROJECTION_BYTES_PER_ACCOUNT ||
      state.persistedProjectionBytes + nextSessionBytes >
        MAX_AGENT_HISTORY_SESSION_PROJECTION_BYTES ||
      this.accountPersistedProjectionBytes + nextAccountBytes >
        MAX_AGENT_HISTORY_ACCOUNT_PROJECTION_BYTES
    ) {
      state.requiresSynchronizedReload = true;
      this.accountRequiresSynchronizedReload = true;
      return "synchronized-reload-required";
    }

    const live: LiveTimelineItem = {
      item: previous ? mergeTimelineItem(previous, incoming) : incoming,
      deltaOnly: current
        ? current.deltaOnly && incoming.merge === "append"
        : !persisted && incoming.merge === "append",
      ...budget
    };
    if (!current) state.liveItemOrder.push(incoming.id);
    state.liveItems.set(incoming.id, live);
    state.liveProjectionBytes = nextSessionBytes;
    this.accountLiveProjectionBytes = nextAccountBytes;
    updateProjectedLiveItem(state, live, !current);
    return "applied";
  }

  acceptEvent(event: {
    eventEpoch?: string | null;
    eventSequence?: number | null;
  }): AgentEventAcceptance {
    const epoch = event.eventEpoch;
    const sequence = event.eventSequence;
    // Embedded events are temporarily unsequenced. Remote/journal events must
    // provide both fields together before ordering claims are made.
    if (epoch === undefined && sequence === undefined) {
      this.eventStateRevision += 1;
      return "accepted";
    }
    if (!epoch || !Number.isSafeInteger(sequence) || (sequence as number) < 0) return "invalid";

    if (this.eventEpoch !== epoch) {
      if (this.retiredEventEpochs.has(epoch)) return "duplicate";
      // Once an account-target stream has an established journal, only the
      // trusted attach/replay coordinator may rotate it. A live event cannot
      // prove that every event from the replacement epoch was observed.
      if (this.eventEpoch) return "gap";
      if (sequence !== 1) return "gap";
      this.eventEpoch = epoch;
      this.eventSequence = sequence as number;
      this.eventStateRevision += 1;
      return "accepted";
    }
    if (this.eventSequence !== null && (sequence as number) <= this.eventSequence) {
      return "duplicate";
    }
    if (this.eventSequence !== null && sequence !== this.eventSequence + 1) return "gap";
    this.eventSequence = sequence as number;
    this.eventStateRevision += 1;
    return "accepted";
  }

  installEventCheckpoint(cursor: { journalId: string; sequence: number }): void {
    this.assertCheckpointCanInstall(cursor);
    this.applyEventCheckpoint(cursor);
  }

  eventCursor(): { journalId: string; sequence: number } | null {
    return this.eventEpoch !== null && this.eventSequence !== null
      ? { journalId: this.eventEpoch, sequence: this.eventSequence }
      : null;
  }

  requireSynchronizedReload(): void {
    this.accountRequiresSynchronizedReload = true;
  }

  private applyEventCheckpoint(cursor: { journalId: string; sequence: number }): void {
    if (this.eventEpoch === cursor.journalId) {
      if (this.eventSequence === cursor.sequence) return;
    } else if (this.eventEpoch) {
      this.retireEventEpoch(this.eventEpoch);
    }
    this.eventEpoch = cursor.journalId;
    this.eventSequence = cursor.sequence;
    this.eventStateRevision += 1;
  }

  seedLiveTimeline(sessionId: string, items: readonly AgentTimelineItem[]): AgentLiveMergeResult {
    if (this.accountRequiresSynchronizedReload) {
      return "synchronized-reload-required";
    }
    if (items.length === 0) return "applied";
    let state = this.sessions.get(sessionId);
    const existingItemIds = new Set(state?.liveItems.keys() ?? []);
    const additionalItemIds = new Set<string>();
    for (const item of items) {
      if (!existingItemIds.has(item.id)) additionalItemIds.add(item.id);
    }
    const nextSessionItemCount = (state?.liveItems.size ?? 0) + additionalItemIds.size;
    const nextAccountItemCount = this.liveItemCount() + additionalItemIds.size;
    const createsLiveSession =
      !state || (state.liveItems.size === 0 && !state.requiresSynchronizedReload);
    if (
      nextSessionItemCount > MAX_AGENT_LIVE_ITEMS_PER_SESSION ||
      nextAccountItemCount > MAX_AGENT_LIVE_ITEMS_PER_ACCOUNT ||
      (createsLiveSession && this.liveSessionCount() >= MAX_AGENT_LIVE_SESSIONS_PER_ACCOUNT)
    ) {
      if (state) state.requiresSynchronizedReload = true;
      this.accountRequiresSynchronizedReload = true;
      return "synchronized-reload-required";
    }
    const plannedItems = new Map(state?.liveItems ?? []);
    let plannedSessionBytes = state?.liveProjectionBytes ?? 0;
    let plannedAccountBytes = this.accountLiveProjectionBytes;
    for (const item of items) {
      const current = plannedItems.get(item.id);
      let persisted: AgentTimelineItem | undefined;
      if (!current && state) {
        persisted = persistedTimelineItem(state, item.id);
      }
      const previous = current?.item ?? persisted;
      const previousTextBytes =
        current?.textBytes ?? (persisted ? timelineItemBudget(persisted).textBytes : 0);
      const budget = previous
        ? mergedTimelineItemBudget(previous, previousTextBytes, item)
        : timelineItemBudget(item);
      const previousBudgetBytes = current?.budgetBytes ?? 0;
      plannedSessionBytes += budget.budgetBytes - previousBudgetBytes;
      plannedAccountBytes += budget.budgetBytes - previousBudgetBytes;
      if (
        budget.textBytes > MAX_AGENT_LIVE_PROJECTION_BYTES_PER_ITEM ||
        plannedSessionBytes > MAX_AGENT_LIVE_PROJECTION_BYTES_PER_SESSION ||
        plannedAccountBytes > MAX_AGENT_LIVE_PROJECTION_BYTES_PER_ACCOUNT ||
        (state?.persistedProjectionBytes ?? 0) + plannedSessionBytes >
          MAX_AGENT_HISTORY_SESSION_PROJECTION_BYTES ||
        this.accountPersistedProjectionBytes + plannedAccountBytes >
          MAX_AGENT_HISTORY_ACCOUNT_PROJECTION_BYTES
      ) {
        if (state) state.requiresSynchronizedReload = true;
        this.accountRequiresSynchronizedReload = true;
        return "synchronized-reload-required";
      }
      plannedItems.set(item.id, {
        item: previous ? mergeTimelineItem(previous, item) : item,
        deltaOnly: current
          ? current.deltaOnly && item.merge === "append"
          : !persisted && item.merge === "append",
        ...budget
      });
    }
    if (!state) state = this.createState(sessionId);
    for (const item of items) {
      if (this.mergeLiveItem(sessionId, item) !== "applied") {
        throw new Error("Agent live seed changed after its capacity preflight");
      }
    }
    return "applied";
  }

  /** Legacy embedded compatibility: its old runStarted event implied a clear. */
  startLiveSuffix(sessionId: string): void {
    const state = this.sessions.get(sessionId);
    if (!state) return;
    this.releaseLiveProjection(state);
    state.authoritativeLiveSuffix = false;
    rebuildProjection(state);
  }

  /**
   * Apply the closed stream's explicit timelineCleared mutation. The event is
   * ordered but is not a synchronized account snapshot, so it must never clear
   * an overflow/gap marker or advance/reset the account checkpoint itself.
   */
  clearLiveTimeline(sessionId: string): void {
    const state = this.sessions.get(sessionId);
    if (!state) return;
    this.releaseLiveProjection(state);
    state.authoritativeLiveSuffix = true;
    rebuildProjection(state);
  }

  invalidate(sessionId: string): void {
    const state = this.sessions.get(sessionId);
    if (!state) return;
    this.accountPersistedProjectionBytes -= state.persistedProjectionBytes;
    state.persistedProjectionBytes = 0;
    state.records = [];
    state.nextCursor = null;
    state.historyRevision = null;
    state.headLoaded = false;
    state.cacheEpoch += 1;
    state.activeRequestId = null;
    rebuildProjection(state);
  }

  remove(sessionId: string): void {
    const state = this.sessions.get(sessionId);
    if (state) {
      this.accountLiveProjectionBytes -= state.liveProjectionBytes;
      this.accountPersistedProjectionBytes -= state.persistedProjectionBytes;
    }
    this.sessions.delete(sessionId);
    this.protectedSessionIds.delete(sessionId);
  }

  clear(): void {
    this.resetState();
  }

  private resetState(): void {
    this.sessions.clear();
    this.eventEpoch = null;
    this.eventSequence = null;
    this.retiredEventEpochs.clear();
    this.retiredEventEpochOrder.splice(0);
    this.accountRequiresSynchronizedReload = false;
    this.accountLiveProjectionBytes = 0;
    this.accountPersistedProjectionBytes = 0;
    this.protectedSessionIds.clear();
    this.eventStateRevision += 1;
    this.bumpLifecycleGeneration();
  }

  snapshot(sessionId: string): AgentHistorySnapshot {
    const state = this.sessions.get(sessionId);
    if (!state) return EMPTY_AGENT_HISTORY_SNAPSHOT;
    this.touchState(state);
    return {
      records: state.records,
      timeline: state.projectedTimeline,
      nextCursor: state.nextCursor,
      historyRevision: state.historyRevision,
      headLoaded: state.headLoaded,
      isLoading: state.activeRequestId !== null,
      hasMore: Boolean(state.nextCursor),
      requiresSynchronizedReload:
        this.accountRequiresSynchronizedReload || state.requiresSynchronizedReload
    };
  }

  /**
   * Only explicitly protected sessions retain arbitrary paged records. Every
   * inactive projection releases persisted scrollback while preserving its
   * bounded live suffix/reload marker and any request that is still in flight.
   */
  reconcileRetention(protectedSessionIds: ReadonlySet<string>): readonly string[] {
    this.protectedSessionIds = new Set(protectedSessionIds);
    const released: string[] = [];
    for (const [sessionId, state] of this.sessions) {
      if (protectedSessionIds.has(sessionId) || state.activeRequestId !== null) continue;
      if (state.records.length === 0 && !state.headLoaded) continue;
      this.releasePersistedProjection(state);
      released.push(sessionId);
      if (state.liveItems.size === 0 && !state.requiresSynchronizedReload) {
        this.sessions.delete(sessionId);
      }
    }
    return released;
  }

  private begin(
    sessionId: string,
    kind: AgentHistoryPageKind,
    cursor: string | null
  ): AgentHistoryPageToken {
    const state = this.ensureState(sessionId);
    this.touchState(state);
    state.nextRequestId += 1;
    state.activeRequestId = state.nextRequestId;
    return Object.freeze({
      sessionId,
      kind,
      cursor,
      lifecycleGeneration: this.lifecycleGeneration,
      stateInstanceId: state.stateInstanceId,
      cacheEpoch: state.cacheEpoch,
      requestId: state.nextRequestId,
      eventSequence: this.eventSequence,
      eventStateRevision: this.eventStateRevision
    });
  }

  private validOwner(owner: AgentHistoryOwner): AgentHistoryOwner {
    if (!owner.accountId || !owner.targetId) {
      throw new Error("Agent history owner requires an account and execution target");
    }
    return Object.freeze({ accountId: owner.accountId, targetId: owner.targetId });
  }

  private assertCheckpointCanInstall(cursor: { journalId: string; sequence: number }): void {
    if (!cursor.journalId || !Number.isSafeInteger(cursor.sequence) || cursor.sequence < 0) {
      throw new Error("Agent event checkpoint is invalid");
    }
    if (this.retiredEventEpochs.has(cursor.journalId)) {
      throw new Error("Agent event checkpoint belongs to a retired journal");
    }
    if (
      this.eventEpoch === cursor.journalId &&
      this.eventSequence !== null &&
      cursor.sequence < this.eventSequence
    ) {
      throw new Error("Agent event checkpoint would regress the journal sequence");
    }
  }

  private planRetainedStateAdmission(
    requiredSessionIds: ReadonlySet<string>,
    excludedSessionId: string
  ): RetainedStateAdmissionPlan {
    const required = [...requiredSessionIds];
    const createSessionIds = required.filter((sessionId) => !this.sessions.has(sessionId));
    const requiredEvictions = Math.max(
      0,
      this.sessions.size + createSessionIds.length - MAX_AGENT_HISTORY_RETAINED_SESSIONS_PER_ACCOUNT
    );
    const candidates = this.inactiveSessionStateCandidates(excludedSessionId, requiredSessionIds);
    if (candidates.length < requiredEvictions) {
      throw new AgentHistoryProjectionLimitError();
    }
    return {
      requiredSessionIds: required,
      createSessionIds,
      evictSessionIds: candidates.slice(0, requiredEvictions).map(([sessionId]) => sessionId)
    };
  }

  private applyRetainedStateAdmissionPlan(
    plan: RetainedStateAdmissionPlan
  ): ReadonlyMap<string, SessionHistoryState> {
    for (const sessionId of plan.evictSessionIds) {
      const state = this.sessions.get(sessionId);
      if (!state) continue;
      this.releasePersistedProjection(state);
      this.sessions.delete(sessionId);
    }
    for (const sessionId of plan.createSessionIds) {
      if (!this.sessions.has(sessionId)) this.createState(sessionId);
    }
    return new Map(
      plan.requiredSessionIds.map((sessionId) => [sessionId, this.sessions.get(sessionId)!])
    );
  }

  private ensureState(
    sessionId: string,
    admissionProtectedSessionIds: ReadonlySet<string> = this.protectedSessionIds
  ): SessionHistoryState {
    let state = this.sessions.get(sessionId);
    if (!state) {
      this.evictInactiveSessionStates(
        sessionId,
        () => this.sessions.size < MAX_AGENT_HISTORY_RETAINED_SESSIONS_PER_ACCOUNT,
        admissionProtectedSessionIds
      );
      if (this.sessions.size >= MAX_AGENT_HISTORY_RETAINED_SESSIONS_PER_ACCOUNT) {
        throw new AgentHistoryProjectionLimitError();
      }
      state = this.createState(sessionId);
    }
    return state;
  }

  private createState(sessionId: string): SessionHistoryState {
    const state = newSessionHistoryState(++this.nextStateInstanceId, ++this.nextRetentionOrdinal);
    this.sessions.set(sessionId, state);
    return state;
  }

  private touchState(state: SessionHistoryState): void {
    state.retentionOrdinal = ++this.nextRetentionOrdinal;
  }

  private admitPersistedProjection(
    sessionId: string,
    state: SessionHistoryState,
    candidateBytes: number,
    admissionLiveProjectionBytes: number,
    admissionProtectedSessionIds: ReadonlySet<string>
  ): void {
    const candidateAccountBytes =
      this.accountPersistedProjectionBytes -
      state.persistedProjectionBytes +
      candidateBytes +
      admissionLiveProjectionBytes;
    if (candidateAccountBytes <= MAX_AGENT_HISTORY_ACCOUNT_PROJECTION_BYTES) return;

    let plannedAccountBytes = candidateAccountBytes;
    const evictionPlan: [string, SessionHistoryState][] = [];
    for (const candidate of this.inactiveSessionStateCandidates(
      sessionId,
      admissionProtectedSessionIds
    )) {
      evictionPlan.push(candidate);
      plannedAccountBytes -= candidate[1].persistedProjectionBytes;
      if (plannedAccountBytes <= MAX_AGENT_HISTORY_ACCOUNT_PROJECTION_BYTES) break;
    }
    if (plannedAccountBytes > MAX_AGENT_HISTORY_ACCOUNT_PROJECTION_BYTES) {
      throw new AgentHistoryProjectionLimitError();
    }
    for (const [evictedSessionId, evictedState] of evictionPlan) {
      this.releasePersistedProjection(evictedState);
      this.sessions.delete(evictedSessionId);
    }
  }

  private evictInactiveSessionStates(
    excludedSessionId: string,
    stop = () => this.sessions.size < MAX_AGENT_HISTORY_RETAINED_SESSIONS_PER_ACCOUNT,
    admissionProtectedSessionIds: ReadonlySet<string> = this.protectedSessionIds
  ): void {
    for (const [sessionId, state] of this.inactiveSessionStateCandidates(
      excludedSessionId,
      admissionProtectedSessionIds
    )) {
      if (stop()) break;
      this.releasePersistedProjection(state);
      this.sessions.delete(sessionId);
    }
  }

  private inactiveSessionStateCandidates(
    excludedSessionId: string,
    admissionProtectedSessionIds: ReadonlySet<string>
  ): [string, SessionHistoryState][] {
    return [...this.sessions.entries()]
      .filter(
        ([sessionId, state]) =>
          sessionId !== excludedSessionId &&
          !this.protectedSessionIds.has(sessionId) &&
          !admissionProtectedSessionIds.has(sessionId) &&
          state.activeRequestId === null &&
          state.liveItems.size === 0 &&
          !state.requiresSynchronizedReload
      )
      .sort(
        ([leftId, left], [rightId, right]) =>
          left.retentionOrdinal - right.retentionOrdinal || compareUtf8Bytes(leftId, rightId)
      );
  }

  private releasePersistedProjection(state: SessionHistoryState): void {
    this.accountPersistedProjectionBytes -= state.persistedProjectionBytes;
    state.persistedProjectionBytes = 0;
    state.records = [];
    state.nextCursor = null;
    state.historyRevision = null;
    state.headLoaded = false;
    state.cacheEpoch += 1;
    rebuildProjection(state);
  }

  private liveSessionCount(): number {
    let count = 0;
    for (const state of this.sessions.values()) {
      if (state.liveItems.size > 0 || state.requiresSynchronizedReload) count += 1;
    }
    return count;
  }

  private liveItemCount(): number {
    let count = 0;
    for (const state of this.sessions.values()) count += state.liveItems.size;
    return count;
  }

  private releaseLiveProjection(state: SessionHistoryState): void {
    this.accountLiveProjectionBytes -= state.liveProjectionBytes;
    state.liveProjectionBytes = 0;
    state.liveItemOrder = [];
    state.liveItems.clear();
  }

  private rebaseDeltaOnlyLiveItems(state: SessionHistoryState): void {
    if (state.liveItems.size === 0 || state.records.length === 0) return;
    const persistedById = new Map<string, AgentTimelineItem>();
    for (const record of state.records) {
      for (const persisted of record.items) {
        const previous = persistedById.get(persisted.id);
        persistedById.set(
          persisted.id,
          previous ? mergeTimelineItem(previous, persisted) : persisted
        );
      }
    }
    const rejectedIds = new Set<string>();
    for (const [itemId, live] of state.liveItems) {
      if (!live.deltaOnly) continue;
      const persisted = persistedById.get(itemId);
      if (!persisted) continue;
      const budget = mergedTimelineItemBudget(
        persisted,
        timelineItemBudget(persisted).textBytes,
        live.item
      );
      const nextSessionBytes = state.liveProjectionBytes - live.budgetBytes + budget.budgetBytes;
      const nextAccountBytes =
        this.accountLiveProjectionBytes - live.budgetBytes + budget.budgetBytes;
      if (
        budget.textBytes > MAX_AGENT_LIVE_PROJECTION_BYTES_PER_ITEM ||
        nextSessionBytes > MAX_AGENT_LIVE_PROJECTION_BYTES_PER_SESSION ||
        nextAccountBytes > MAX_AGENT_LIVE_PROJECTION_BYTES_PER_ACCOUNT ||
        state.persistedProjectionBytes + nextSessionBytes >
          MAX_AGENT_HISTORY_SESSION_PROJECTION_BYTES ||
        this.accountPersistedProjectionBytes + nextAccountBytes >
          MAX_AGENT_HISTORY_ACCOUNT_PROJECTION_BYTES
      ) {
        rejectedIds.add(itemId);
        state.liveItems.delete(itemId);
        state.liveProjectionBytes -= live.budgetBytes;
        this.accountLiveProjectionBytes -= live.budgetBytes;
        state.requiresSynchronizedReload = true;
        this.accountRequiresSynchronizedReload = true;
        continue;
      }
      const rebased: LiveTimelineItem = {
        item: mergeTimelineItem(persisted, live.item),
        deltaOnly: false,
        ...budget
      };
      state.liveItems.set(itemId, rebased);
      state.liveProjectionBytes = nextSessionBytes;
      this.accountLiveProjectionBytes = nextAccountBytes;
    }
    if (rejectedIds.size > 0) {
      state.liveItemOrder = state.liveItemOrder.filter((itemId) => !rejectedIds.has(itemId));
    }
  }

  private bumpLifecycleGeneration(): void {
    this.lifecycleGeneration += 1;
    for (const state of this.sessions.values()) state.activeRequestId = null;
  }

  private validateSynchronizedLiveSnapshot(
    snapshot: AgentSynchronizedLiveSnapshot
  ): AgentLiveSessionSnapshot[] {
    if (snapshot.liveSessionsComplete !== true) {
      throw new Error("Agent synchronized live snapshot must be complete");
    }
    if (
      !Number.isSafeInteger(snapshot.liveSessionCount) ||
      snapshot.liveSessionCount < 0 ||
      snapshot.liveSessionCount !== snapshot.liveSessions.length ||
      snapshot.liveSessionCount > MAX_AGENT_LIVE_SESSIONS_PER_ACCOUNT
    ) {
      throw new Error("Agent synchronized live session count is invalid");
    }
    let previousSessionId: string | null = null;
    let totalItemCount = 0;
    let totalProjectionBytes = 0;
    const absoluteSessions: AgentLiveSessionSnapshot[] = [];
    for (const liveSession of snapshot.liveSessions) {
      if (
        !liveSession.sessionId ||
        (previousSessionId !== null &&
          compareUtf8Bytes(previousSessionId, liveSession.sessionId) >= 0)
      ) {
        throw new Error("Agent synchronized live sessions must have unique sorted IDs");
      }
      previousSessionId = liveSession.sessionId;
      if (liveSession.liveItems.length > MAX_AGENT_LIVE_ITEMS_PER_SESSION) {
        throw new Error("Agent synchronized live suffix exceeds its session limit");
      }
      const itemIds = new Set<string>();
      const absoluteItems: AgentTimelineItem[] = [];
      let sessionProjectionBytes = 0;
      for (const item of liveSession.liveItems) {
        if (item.merge !== "replace") {
          throw new Error("Agent synchronized live suffix must contain absolute items");
        }
        if (itemIds.has(item.id)) {
          throw new Error("Agent synchronized live suffix contains a duplicate item ID");
        }
        const budget = timelineItemBudget(item);
        sessionProjectionBytes += budget.budgetBytes;
        totalProjectionBytes += budget.budgetBytes;
        if (
          budget.textBytes > MAX_AGENT_LIVE_PROJECTION_BYTES_PER_ITEM ||
          sessionProjectionBytes > MAX_AGENT_LIVE_PROJECTION_BYTES_PER_SESSION ||
          totalProjectionBytes > MAX_AGENT_LIVE_PROJECTION_BYTES_PER_ACCOUNT
        ) {
          throw new Error("Agent synchronized live suffix exceeds its byte budget");
        }
        itemIds.add(item.id);
        absoluteItems.push(item);
      }
      totalItemCount += absoluteItems.length;
      if (totalItemCount > MAX_AGENT_LIVE_ITEMS_PER_ACCOUNT) {
        throw new Error("Agent synchronized live suffix exceeds the account item limit");
      }
      absoluteSessions.push({ sessionId: liveSession.sessionId, liveItems: absoluteItems });
    }
    return absoluteSessions;
  }

  private retireEventEpoch(epoch: string): void {
    if (this.retiredEventEpochs.has(epoch)) return;
    this.retiredEventEpochs.add(epoch);
    this.retiredEventEpochOrder.push(epoch);
    while (this.retiredEventEpochOrder.length > MAX_AGENT_RETIRED_EVENT_EPOCHS) {
      const oldest = this.retiredEventEpochOrder.shift();
      if (oldest) this.retiredEventEpochs.delete(oldest);
    }
  }
}
