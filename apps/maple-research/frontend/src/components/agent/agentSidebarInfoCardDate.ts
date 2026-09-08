const SAME_YEAR_DATE_FORMAT: Intl.DateTimeFormatOptions = {
  month: "short",
  day: "numeric"
};

const OTHER_YEAR_DATE_FORMAT: Intl.DateTimeFormatOptions = {
  month: "short",
  day: "numeric",
  year: "numeric"
};

const FULL_DATE_TITLE_FORMAT: Intl.DateTimeFormatOptions = {
  month: "short",
  day: "numeric",
  year: "numeric",
  hour: "numeric",
  minute: "2-digit"
};

function validAgentSidebarDate(updatedMs: number): Date | null {
  if (!Number.isFinite(updatedMs) || updatedMs <= 0) return null;
  const date = new Date(updatedMs);
  if (Number.isNaN(date.getTime())) return null;
  return date;
}

export function formatAgentSidebarDate(updatedMs: number, nowMs = Date.now()): string | null {
  const date = validAgentSidebarDate(updatedMs);
  if (!date) return null;
  const now = new Date(nowMs);
  const options =
    date.getFullYear() === now.getFullYear() ? SAME_YEAR_DATE_FORMAT : OTHER_YEAR_DATE_FORMAT;
  return date.toLocaleDateString(undefined, options);
}

export function agentSidebarDateTime(updatedMs: number): string | null {
  return validAgentSidebarDate(updatedMs)?.toISOString() ?? null;
}

export function agentSidebarDateTitle(updatedMs: number): string | null {
  const date = validAgentSidebarDate(updatedMs);
  if (!date) return null;
  return date.toLocaleString(undefined, FULL_DATE_TITLE_FORMAT);
}

export function latestAgentSidebarUpdatedMs(updatedMsList: number[]): number | undefined {
  const latest = updatedMsList.reduce((max, value) => {
    if (!Number.isFinite(value) || value <= 0) return max;
    return Math.max(max, value);
  }, 0);
  return latest > 0 ? latest : undefined;
}
