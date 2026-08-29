import type { ReactNode } from "react";
import {
  Blocks,
  Check,
  ChevronRight,
  FilePenLine,
  FileSearch,
  Globe2,
  Loader2,
  SquareTerminal,
  Wrench,
  X
} from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { toolKindLabel, type ToolActivityStatus, type ToolKind } from "@/services/toolPresentation";
import { cn } from "@/utils/utils";

const TOOL_KIND_ICONS: Record<ToolKind, LucideIcon> = {
  shell: SquareTerminal,
  "file-read": FileSearch,
  "file-write": FilePenLine,
  web: Globe2,
  mcp: Blocks,
  generic: Wrench
};

const TOOL_STATUS_LABELS: Record<ToolActivityStatus, string> = {
  active: "In progress",
  completed: "Completed",
  incomplete: "Incomplete",
  error: "Failed"
};

export function ToolActivityCard({
  kind,
  title,
  status,
  statusLabel = TOOL_STATUS_LABELS[status],
  children
}: {
  kind: ToolKind;
  title: string;
  status: ToolActivityStatus;
  statusLabel?: string;
  children?: ReactNode;
}) {
  const failed = status === "error";
  const incomplete = status === "incomplete";
  const ToolKindIcon = TOOL_KIND_ICONS[kind];
  const kindLabel = toolKindLabel(kind);
  const statusIcon =
    status === "active" ? (
      <Loader2
        aria-hidden="true"
        className="h-3.5 w-3.5 shrink-0 animate-spin text-muted-foreground"
      />
    ) : failed ? (
      <X
        aria-hidden="true"
        className="h-3.5 w-3.5 shrink-0 text-[hsl(var(--maple-error-foreground))]"
      />
    ) : incomplete ? (
      <X
        aria-hidden="true"
        className="h-3.5 w-3.5 shrink-0 text-[hsl(var(--maple-warning-foreground))]"
      />
    ) : (
      <Check aria-hidden="true" className="h-3.5 w-3.5 shrink-0 text-maple-success" />
    );
  const summary = (
    <span
      className="flex min-w-0 flex-1 items-center gap-1.5"
      role="status"
      aria-live="polite"
      aria-atomic="true"
      aria-label={`${kindLabel}: ${title}, ${statusLabel}`}
    >
      <span
        aria-hidden="true"
        title={kindLabel}
        className="flex h-6 w-6 shrink-0 items-center justify-center rounded-md bg-background/70 text-muted-foreground"
      >
        <ToolKindIcon aria-hidden="true" className="h-3.5 w-3.5" />
      </span>
      <span
        aria-hidden="true"
        className="min-w-0 flex-1 truncate text-[13px] font-medium leading-5 text-foreground"
        title={title}
      >
        {title}
      </span>
      <span
        aria-hidden="true"
        className={cn(
          "shrink-0 text-[11px] leading-5 text-muted-foreground",
          failed && "text-[hsl(var(--maple-error-foreground))]",
          incomplete && "text-[hsl(var(--maple-warning-foreground))]"
        )}
      >
        {statusLabel}
      </span>
      {statusIcon}
    </span>
  );

  if (children === undefined || children === null) {
    return (
      <div
        className={cn(
          "flex min-h-8 items-center rounded-xl bg-muted/30 px-2 py-1 text-sm",
          failed && "bg-destructive/5"
        )}
      >
        {summary}
      </div>
    );
  }

  return (
    <details
      open={failed}
      className={cn(
        "group rounded-xl border border-muted/40 bg-muted/20 px-2 py-1 text-sm",
        failed && "border-destructive/35 bg-destructive/5"
      )}
    >
      <summary className="flex min-h-6 cursor-pointer list-none items-center gap-1">
        <ChevronRight
          aria-hidden="true"
          className="h-3.5 w-3.5 shrink-0 text-muted-foreground transition-transform group-open:rotate-90"
        />
        {summary}
      </summary>
      <div className="mt-1.5 space-y-2 border-t border-muted/40 pb-1 pl-7 pr-1 pt-2">
        {children}
      </div>
    </details>
  );
}
