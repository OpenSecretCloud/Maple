import type { AgentSessionSummary, RecentProjectRoot } from "@/services/agentRuntimeService";

export const PROJECT_DRAG_THRESHOLD_PX = 6;

export interface ProjectHeaderCenter {
  path: string;
  centerY: number;
}

export interface ProjectOrderState<T extends { path: string }> {
  visible: T[];
  confirmed: T[];
  pendingRequestId: number | null;
}

export type ProjectOrderAction<T extends { path: string }> =
  | { type: "replace"; roots: T[] }
  | { type: "optimistic"; requestId: number; roots: T[] }
  | { type: "confirmed"; requestId: number; roots: T[] }
  | { type: "rejected"; requestId: number };

export function createProjectOrderState<T extends { path: string }>(
  roots: T[]
): ProjectOrderState<T> {
  return {
    visible: roots,
    confirmed: roots,
    pendingRequestId: null
  };
}

export function projectOrderReducer<T extends { path: string }>(
  state: ProjectOrderState<T>,
  action: ProjectOrderAction<T>
): ProjectOrderState<T> {
  switch (action.type) {
    case "replace":
      return createProjectOrderState(action.roots);
    case "optimistic":
      return {
        visible: action.roots,
        confirmed: state.confirmed,
        pendingRequestId: action.requestId
      };
    case "confirmed":
      if (state.pendingRequestId !== action.requestId) return state;
      return createProjectOrderState(action.roots);
    case "rejected":
      if (state.pendingRequestId !== action.requestId) return state;
      return {
        visible: state.confirmed,
        confirmed: state.confirmed,
        pendingRequestId: null
      };
  }
}

export function mergeAgentProjectRoots(
  savedRoots: readonly RecentProjectRoot[],
  activeRoot: string,
  sessions: readonly AgentSessionSummary[]
): RecentProjectRoot[] {
  const dedupedSavedRoots: RecentProjectRoot[] = [];
  const savedPaths = new Set<string>();

  for (const root of savedRoots) {
    if (!validPath(root.path) || savedPaths.has(root.path)) continue;
    savedPaths.add(root.path);
    dedupedSavedRoots.push(root);
  }

  const merged: RecentProjectRoot[] = [];
  const visiblePaths = new Set<string>();
  const addRoot = (root: RecentProjectRoot) => {
    if (!validPath(root.path) || visiblePaths.has(root.path)) return;
    visiblePaths.add(root.path);
    merged.push(root);
  };

  dedupedSavedRoots.forEach(addRoot);

  const unseenPaths = new Set<string>();
  if (validPath(activeRoot) && !visiblePaths.has(activeRoot)) unseenPaths.add(activeRoot);
  for (const session of sessions) {
    if (validPath(session.projectRoot) && !visiblePaths.has(session.projectRoot)) {
      unseenPaths.add(session.projectRoot);
    }
  }
  [...unseenPaths].sort(comparePaths).forEach((path) => addRoot(syntheticRoot(path)));

  return merged;
}

export function groupAgentSessionsByRoot(
  sessions: readonly AgentSessionSummary[]
): Map<string, AgentSessionSummary[]> {
  const sessionsByRoot = new Map<string, AgentSessionSummary[]>();
  for (const session of sessions) {
    const rootSessions = sessionsByRoot.get(session.projectRoot) || [];
    rootSessions.push(session);
    sessionsByRoot.set(session.projectRoot, rootSessions);
  }
  sessionsByRoot.forEach((rootSessions) => {
    rootSessions.sort((a, b) => b.updatedMs - a.updatedMs);
  });
  return sessionsByRoot;
}

export function projectOrderForExistingRegistration<T extends { path: string }>(
  roots: readonly T[],
  selectedPath: string
): string[] | null {
  if (!validPath(selectedPath) || !roots.some((root) => root.path === selectedPath)) return null;

  const paths: string[] = [];
  const seen = new Set<string>();
  for (const root of roots) {
    if (!validPath(root.path) || seen.has(root.path)) continue;
    seen.add(root.path);
    paths.push(root.path);
  }
  return paths;
}

export function reorderProjectRoots<T extends { path: string }>(
  roots: readonly T[],
  draggedPath: string,
  insertionIndex: number | null
): readonly T[] {
  if (insertionIndex === null || !Number.isInteger(insertionIndex) || insertionIndex < 0) {
    return roots;
  }

  const sourceIndex = roots.findIndex((root) => root.path === draggedPath);
  if (sourceIndex < 0) return roots;

  const remaining = roots.filter((_, index) => index !== sourceIndex);
  if (insertionIndex > remaining.length) return roots;

  const next = [...remaining];
  next.splice(insertionIndex, 0, roots[sourceIndex]);
  if (next.every((root, index) => root === roots[index])) return roots;
  return next;
}

export function projectInsertionIndex(
  pointerY: number,
  centers: readonly ProjectHeaderCenter[],
  draggedPath: string
): number | null {
  if (!Number.isFinite(pointerY)) return null;
  const candidates = centers.filter(
    (row) => row.path !== draggedPath && Number.isFinite(row.centerY)
  );
  const index = candidates.findIndex((row) => pointerY < row.centerY);
  return index < 0 ? candidates.length : index;
}

export function hasExceededProjectDragThreshold(
  startX: number,
  startY: number,
  currentX: number,
  currentY: number,
  threshold = PROJECT_DRAG_THRESHOLD_PX
): boolean {
  return Math.hypot(currentX - startX, currentY - startY) >= threshold;
}

function syntheticRoot(path: string): RecentProjectRoot {
  return {
    path,
    name: basename(path),
    lastUsedMs: 0
  };
}

function validPath(path: string): boolean {
  return typeof path === "string" && path.trim().length > 0;
}

function comparePaths(a: string, b: string): number {
  if (a < b) return -1;
  if (a > b) return 1;
  return 0;
}

function basename(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() || path;
}
