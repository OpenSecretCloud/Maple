import type { ReactNode } from "react";

import { cn } from "@/utils/utils";

export function SidebarNewItemButton({
  children,
  hasAction,
  isAgentMode,
  isTemporarilyDisabled = false,
  onClick
}: {
  children: ReactNode;
  hasAction: boolean;
  isAgentMode: boolean;
  isTemporarilyDisabled?: boolean;
  onClick: () => void;
}) {
  const isDisabled = isAgentMode && (!hasAction || isTemporarilyDisabled);
  const isVisuallyDisabled = isAgentMode && !hasAction;

  return (
    <button
      type="button"
      className={cn(
        "flex w-full items-center justify-start gap-2 py-1.5 pr-1 pl-0 text-sm text-[hsl(var(--maple-primary-strong))] transition-colors hover:text-[hsl(var(--maple-primary))] disabled:cursor-not-allowed dark:text-[hsl(var(--maple-primary))] dark:hover:text-[hsl(var(--maple-primary-strong))]",
        isVisuallyDisabled && "opacity-50"
      )}
      disabled={isDisabled}
      onClick={onClick}
    >
      {children}
    </button>
  );
}
