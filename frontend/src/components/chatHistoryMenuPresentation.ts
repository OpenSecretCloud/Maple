const SIDEBAR_ELLIPSIS_BUTTON_CLASS =
  "relative z-10 shrink-0 rounded-full border-0 bg-muted p-1.5 text-foreground/40 transition-colors dark:bg-[hsl(var(--sidebar))] hover:text-foreground group-hover:text-foreground focus-visible:text-foreground focus-visible:outline-none";

const PAGE_ELLIPSIS_BUTTON_CLASS =
  "relative z-10 flex h-11 w-11 shrink-0 items-center justify-center rounded-full border-0 bg-muted p-0 text-foreground/40 transition-colors dark:bg-[hsl(var(--sidebar))] data-[state=open]:text-foreground focus-visible:text-foreground focus-visible:outline-none";

export function getChatHistoryEllipsisButtonClass(pagePresentation: boolean): string {
  return pagePresentation ? PAGE_ELLIPSIS_BUTTON_CLASS : SIDEBAR_ELLIPSIS_BUTTON_CLASS;
}
