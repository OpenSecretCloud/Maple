import type { MouseEvent } from "react";
import { Link } from "@tanstack/react-router";
import { Home } from "lucide-react";
import { MapleWordmark } from "@/components/MapleWordmark";
import { marketingUrl } from "@/config/domains";
import { openExternalUrl } from "@/utils/openUrl";
import { isTauri } from "@/utils/platform";

const marketingHomeUrl = marketingUrl("/");

function openExternalLink(event: MouseEvent<HTMLAnchorElement>, url: string) {
  if (!isTauri()) {
    return;
  }

  event.preventDefault();
  openExternalUrl(url);
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
    <header className="mx-auto grid w-full max-w-6xl grid-cols-[1fr_auto] items-center gap-3 px-4 py-5 sm:px-6 lg:px-8">
      <Link
        to="/"
        aria-label="Maple app home"
        className="col-start-1 row-start-1 flex items-center justify-self-start"
      >
        <MapleWordmark className="h-5 w-auto text-[#221a18] dark:text-foreground" />
      </Link>

      <MarketingHomeLink className="col-start-2 row-start-1 inline-flex items-center gap-2 justify-self-end rounded-md px-3 py-2 text-sm font-semibold text-[#747474] transition hover:bg-black/5 hover:text-[#221a18] dark:text-muted-foreground dark:hover:bg-white/5 dark:hover:text-foreground" />
    </header>
  );
}
