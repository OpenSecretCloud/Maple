import type { CSSProperties } from "react";

export const SIDEBAR_WIDTH_PX = 296;

const SIDEBAR_WIDTH = `${SIDEBAR_WIDTH_PX}px`;
const SIDEBAR_CONTENT_CENTER_OFFSET = `${SIDEBAR_WIDTH_PX / 2}px`;

export const SIDEBAR_LAYOUT_STYLE = {
  "--sidebar-width": SIDEBAR_WIDTH,
  "--sidebar-grid-template": `${SIDEBAR_WIDTH} minmax(0, 1fr)`,
  "--sidebar-content-center-offset": SIDEBAR_CONTENT_CENTER_OFFSET
} as CSSProperties;

export function getSidebarLayoutStyle({ offsetContent = true } = {}) {
  return {
    ...SIDEBAR_LAYOUT_STYLE,
    "--sidebar-content-center-offset": offsetContent ? SIDEBAR_CONTENT_CENTER_OFFSET : "0px"
  } as CSSProperties;
}

export const SIDEBAR_GRID_COLUMNS_CLASS = "md:grid-cols-[var(--sidebar-grid-template)]";
export const SIDEBAR_WIDTH_CLASS = "w-[var(--sidebar-width)]";
export const SIDEBAR_MAX_WIDTH_CLASS = "max-w-[var(--sidebar-width)]";
export const SIDEBAR_ACCOUNT_MENU_WIDTH_CLASS = "w-[calc(var(--sidebar-width)_-_2rem)]";
export const SIDEBAR_AWARE_FIXED_CENTER_CLASS =
  "md:left-[calc(50%_+_var(--sidebar-content-center-offset))]";
