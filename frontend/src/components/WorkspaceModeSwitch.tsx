import { Bot, MessageCircle, type LucideIcon } from "lucide-react";

import { cn } from "@/utils/utils";

export type WorkspaceMode = "chat" | "agent";

type WorkspaceModeOption = {
  mode: WorkspaceMode;
  label: string;
  icon: LucideIcon;
  iconClassName?: string;
  selectedIconClassName: string;
};

const WORKSPACE_MODE_OPTIONS: WorkspaceModeOption[] = [
  {
    mode: "chat",
    label: "Chat",
    icon: MessageCircle,
    selectedIconClassName:
      "fill-[hsl(var(--maple-primary))] text-[hsl(var(--maple-primary-strong))]"
  },
  {
    mode: "agent",
    label: "Agent",
    icon: Bot,
    iconClassName:
      "[&>rect]:fill-transparent [&>rect]:transition-[fill] [&>rect]:duration-200 motion-reduce:[&>rect]:transition-none",
    selectedIconClassName:
      "text-[hsl(var(--maple-primary-strong))] [&>rect]:fill-[hsl(var(--maple-primary))]"
  }
];

export function WorkspaceModeSwitch({
  mode,
  onModeChange,
  onModeTransitionEnd
}: {
  mode: WorkspaceMode;
  onModeChange: (mode: WorkspaceMode) => void;
  onModeTransitionEnd?: (mode: WorkspaceMode) => void;
}) {
  return (
    <fieldset className="m-0 min-w-0 border-0 p-0">
      <legend className="sr-only">Workspace mode</legend>
      {/* Keep both segments even-width so their centered icons land on whole CSS pixels. */}
      <div className="relative grid w-[calc(100%-1px)] grid-cols-2 gap-1 rounded-[10px] border border-foreground/10 bg-foreground/5 px-0.5 py-px shadow-inner dark:border-transparent dark:bg-foreground/[0.07] dark:shadow-none">
        <span
          aria-hidden="true"
          className={cn(
            "pointer-events-none absolute inset-y-px left-0.5 w-[calc(50%-0.25rem)] rounded-lg bg-[hsl(var(--sidebar-chrome))] shadow-sm transition-transform duration-200 ease-out motion-reduce:transition-none",
            mode === "agent" ? "translate-x-[calc(100%+0.25rem)]" : "translate-x-0"
          )}
          onTransitionEnd={(event) => {
            if (event.propertyName === "transform") onModeTransitionEnd?.(mode);
          }}
        />
        {WORKSPACE_MODE_OPTIONS.map((option) => {
          const isSelected = option.mode === mode;
          const Icon = option.icon;
          const id = `workspace-mode-${option.mode}`;

          return (
            <div key={option.mode} className="relative z-10 min-w-0">
              <input
                id={id}
                type="radio"
                name="workspace-mode"
                value={option.mode}
                checked={isSelected}
                className="peer sr-only"
                onChange={() => {
                  if (!isSelected) onModeChange(option.mode);
                }}
              />
              <label
                htmlFor={id}
                className={cn(
                  "group flex h-8 w-full cursor-pointer items-center justify-center gap-2 rounded-lg px-2 text-sm peer-focus-visible:outline-none peer-focus-visible:ring-2 peer-focus-visible:ring-ring peer-focus-visible:ring-offset-1 peer-focus-visible:ring-offset-muted",
                  isSelected ? "font-semibold" : "font-medium"
                )}
              >
                <Icon
                  aria-hidden="true"
                  className={cn(
                    "h-4 w-4 shrink-0 duration-200 motion-reduce:transition-none",
                    option.mode === "agent" ? "transition-[color]" : "transition-colors",
                    option.iconClassName,
                    isSelected
                      ? option.selectedIconClassName
                      : "text-muted-foreground group-hover:text-foreground"
                  )}
                  strokeWidth={2}
                />
                <span
                  className={cn(
                    "w-10 shrink-0 text-left transition-colors duration-200 motion-reduce:transition-none",
                    isSelected
                      ? "text-foreground"
                      : "text-muted-foreground group-hover:text-foreground"
                  )}
                >
                  {option.label}
                </span>
              </label>
            </div>
          );
        })}
      </div>
    </fieldset>
  );
}
