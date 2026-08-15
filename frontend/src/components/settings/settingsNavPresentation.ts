export type SettingsNavLinkPresentation = {
  activeClassName: string;
  inactiveClassName: string;
  containerClassName: string;
};

export function getSettingsNavLinkPresentation(compact: boolean): SettingsNavLinkPresentation {
  if (compact) {
    // Mobile WebKit can latch :hover after a touch scroll, so compact rows paint only route state.
    return {
      activeClassName: "bg-[hsl(var(--sidebar-row-selected))] text-foreground",
      inactiveClassName: "text-muted-foreground",
      containerClassName: "-mx-2 gap-2 rounded-2xl px-2 text-base leading-6"
    };
  }

  return {
    activeClassName:
      "bg-[hsl(var(--sidebar-chrome))] text-foreground shadow-sm dark:bg-[hsl(var(--sidebar-chrome-hover))]",
    inactiveClassName: "text-muted-foreground hover:bg-background/70 hover:text-foreground",
    containerClassName: "gap-3 rounded-lg px-3 py-2 text-sm"
  };
}
