import type { MouseEvent } from "react";
import { Link } from "@tanstack/react-router";
import { ArrowRight, ExternalLink, Home } from "lucide-react";
import { MapleWordmark } from "@/components/MapleWordmark";
import { marketingUrl } from "@/config/domains";
import { openExternalUrl } from "@/utils/openUrl";
import { isTauri } from "@/utils/platform";
import { cn } from "@/utils/utils";

const marketingHomeUrl = marketingUrl("/");
const rebrandAnnouncementUrl =
  "https://blog.trymaple.ai/meet-maple-the-personal-intelligence-platform/";

function openExternalLink(event: MouseEvent<HTMLAnchorElement>, url: string) {
  if (!isTauri()) {
    return;
  }

  event.preventDefault();
  openExternalUrl(url);
}

function RebrandAnnouncementLink({ className }: { className?: string }) {
  return (
    <a
      href={rebrandAnnouncementUrl}
      target="_blank"
      rel="noopener noreferrer"
      onClick={(event) => openExternalLink(event, rebrandAnnouncementUrl)}
      className={cn(
        "group inline-flex min-h-10 w-full max-w-full items-center justify-center gap-2 rounded-md border border-neutral-900/10 bg-white/65 px-3 py-2 text-[11px] font-semibold text-[#221a18] shadow-sm backdrop-blur transition hover:border-neutral-900/15 hover:bg-white/85 dark:border-white/10 dark:bg-white/[0.06] dark:text-foreground dark:hover:border-white/15 dark:hover:bg-white/[0.1] sm:w-auto sm:text-xs",
        className
      )}
      aria-label="Maple AI is now Maple Research. Read the announcement."
    >
      <span className="hidden items-center gap-1.5 md:inline-flex" aria-hidden="true">
        <span className="inline-flex h-6 w-6 overflow-hidden rounded border border-neutral-900/10 bg-[#111111] dark:border-white/10">
          <img
            src="/maple-icon-nobg.png"
            alt=""
            className="h-full w-full scale-[1.28] object-contain"
          />
        </span>
        <ArrowRight className="h-3 w-3 text-[#747474] dark:text-muted-foreground" />
        <img src="/maple-research-icon.svg" alt="" className="h-6 w-6 rounded-[6px]" />
      </span>
      <span className="min-w-0 truncate">
        <span className="hidden sm:inline">Maple AI is now Maple Research</span>
        <span className="sm:hidden">Maple AI -&gt; Maple Research</span>
      </span>
      <span
        className="hidden h-1 w-1 shrink-0 rounded-full bg-neutral-400/80 dark:bg-white/30 sm:block"
        aria-hidden="true"
      />
      <span className="inline-flex shrink-0 items-center gap-1 whitespace-nowrap text-[#d65f35] transition group-hover:text-[#b94c26] dark:text-[#ff9b72] dark:group-hover:text-[#ffb095]">
        <span className="hidden sm:inline">Read announcement</span>
        <span className="sm:hidden">Read</span>
        <ExternalLink className="h-3 w-3" aria-hidden="true" />
      </span>
    </a>
  );
}

function MarketingHomeLink({ className }: { className?: string }) {
  return (
    <a
      href={marketingHomeUrl}
      onClick={(event) => openExternalLink(event, marketingHomeUrl)}
      className={className}
    >
      <Home className="h-4 w-4" />
      <span className="hidden sm:inline">Learn about Maple</span>
      <span className="sm:hidden">Home</span>
    </a>
  );
}

export function AuthHeader() {
  return (
    <header className="mx-auto grid w-full max-w-6xl grid-cols-[minmax(0,1fr)_auto] items-center gap-x-3 gap-y-3 px-4 py-5 sm:px-6 lg:grid-cols-[1fr_auto_1fr] lg:px-8">
      <Link
        to="/"
        aria-label="Maple app home"
        className="col-start-1 row-start-1 flex items-center justify-self-start"
      >
        <MapleWordmark className="h-5 w-auto text-[#221a18] dark:text-foreground" />
      </Link>

      <RebrandAnnouncementLink className="col-span-2 row-start-2 justify-self-stretch sm:justify-self-center lg:col-span-1 lg:col-start-2 lg:row-start-1" />

      <MarketingHomeLink className="col-start-2 row-start-1 inline-flex items-center gap-2 justify-self-end rounded-md px-3 py-2 text-sm font-semibold text-[#747474] transition hover:bg-black/5 hover:text-[#221a18] dark:text-muted-foreground dark:hover:bg-white/5 dark:hover:text-foreground lg:col-start-3" />
    </header>
  );
}
