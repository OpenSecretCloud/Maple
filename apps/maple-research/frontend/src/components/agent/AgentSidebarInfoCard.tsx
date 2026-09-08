import { FolderOpen, type LucideIcon } from "lucide-react";

import {
  agentSidebarDateTime,
  agentSidebarDateTitle,
  formatAgentSidebarDate
} from "@/components/agent/agentSidebarInfoCardDate";
import { truncatePathMiddle } from "@/utils/path";
import { cn } from "@/utils/utils";

interface AgentSidebarInfoCardProps {
  folderPath: string;
  icon: LucideIcon;
  isInProgress: boolean;
  metadata: string;
  metadataIcon: LucideIcon;
  onDismiss: () => void;
  onOpenProjectFolder: () => void;
  progressLabel: string;
  title: string;
  updatedMs?: number | null;
}

export function AgentSidebarInfoCard({
  folderPath,
  icon: Icon,
  isInProgress,
  metadata,
  metadataIcon: MetadataIcon,
  onDismiss,
  onOpenProjectFolder,
  progressLabel,
  title,
  updatedMs
}: AgentSidebarInfoCardProps) {
  const dateLabel = updatedMs == null ? null : formatAgentSidebarDate(updatedMs);
  const dateTime = updatedMs == null ? null : agentSidebarDateTime(updatedMs);
  const dateTitle = updatedMs == null ? null : agentSidebarDateTitle(updatedMs);

  return (
    <div>
      <div className="flex min-w-0 items-start gap-2.5">
        <span className="mt-0.5 flex h-8 w-8 shrink-0 items-center justify-center rounded-lg bg-muted/60 text-[hsl(var(--maple-primary))]">
          <Icon aria-hidden="true" className="h-4 w-4" />
        </span>
        <div className="min-w-0 flex-1">
          <div className="flex min-w-0 items-start gap-2">
            <p className="min-w-0 flex-1 break-words text-[13px] font-semibold leading-5">
              {title}
            </p>
            {dateLabel && dateTime ? (
              <time
                className="mt-0.5 shrink-0 whitespace-nowrap text-[11px] font-medium leading-4 text-muted-foreground"
                dateTime={dateTime}
                title={dateTitle ?? undefined}
              >
                {dateLabel}
              </time>
            ) : null}
          </div>
          <div className="mt-0.5 flex min-w-0 items-start gap-1.5 text-xs leading-4 text-muted-foreground">
            <MetadataIcon aria-hidden="true" className="mt-px h-3.5 w-3.5 shrink-0" />
            <span className="min-w-0 break-words">{metadata}</span>
          </div>
        </div>
      </div>

      <div aria-hidden="true" className="my-2.5 h-px bg-border/70" />

      <div className="flex items-center gap-2 px-1 text-xs font-medium text-foreground/90">
        <span aria-hidden="true" className="relative flex h-2 w-2 shrink-0">
          {isInProgress ? (
            <span className="absolute inline-flex h-full w-full rounded-full bg-[hsl(var(--maple-primary))] opacity-40 motion-safe:animate-ping" />
          ) : null}
          <span
            className={cn(
              "relative inline-flex h-2 w-2 rounded-full",
              isInProgress ? "bg-[hsl(var(--maple-primary))]" : "bg-muted-foreground/45"
            )}
          />
        </span>
        <span>{progressLabel}</span>
      </div>

      <button
        type="button"
        className="group/folder -mx-1 mt-1 flex w-[calc(100%+0.5rem)] items-start gap-2 rounded-lg px-2 py-1.5 text-left text-xs text-muted-foreground transition-colors hover:bg-muted/70 hover:text-foreground focus-visible:bg-muted/70 focus-visible:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring/70"
        onClick={(event) => {
          event.preventDefault();
          event.stopPropagation();
          onDismiss();
          onOpenProjectFolder();
        }}
        aria-label={`Open project folder: ${folderPath}`}
      >
        <FolderOpen
          aria-hidden="true"
          className="mt-px h-3.5 w-3.5 shrink-0 transition-colors group-hover/folder:text-foreground"
        />
        <span className="min-w-0 whitespace-nowrap font-mono leading-4">
          <span className="sr-only">{folderPath}</span>
          <span aria-hidden="true">{truncatePathMiddle(folderPath, 40)}</span>
        </span>
      </button>
    </div>
  );
}
