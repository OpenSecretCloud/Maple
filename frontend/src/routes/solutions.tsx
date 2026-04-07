import { createFileRoute, Link, Outlet, useRouterState } from "@tanstack/react-router";
import { TopNav } from "@/components/TopNav";
import { FullPageMain } from "@/components/FullPageMain";
import { MarketingHeader } from "@/components/MarketingHeader";
import {
  ArrowRight,
  Briefcase,
  Code2,
  HeartHandshake,
  Landmark,
  PiggyBank,
  Users
} from "lucide-react";

const TILES = [
  {
    to: "/solutions/lawyers",
    title: "AI for Lawyers",
    description: "Matter work, drafts, and research with client confidentiality first.",
    icon: Landmark
  },
  {
    to: "/solutions/accountants",
    title: "AI for Accountants",
    description: "Tax and advisory workflows without exposing client financials.",
    icon: PiggyBank
  },
  {
    to: "/solutions/finance",
    title: "AI for Finance",
    description: "Analysis and reporting with controls fit for regulated data.",
    icon: Briefcase
  },
  {
    to: "/solutions/therapy",
    title: "AI for Therapy",
    description: "Support clinical notes and prep with PHI-grade privacy posture.",
    icon: HeartHandshake
  },
  {
    to: "/solutions/teams",
    title: "AI for Teams",
    description: "The default for cross-functional teams and general enterprise use.",
    icon: Users
  },
  {
    to: "/solutions/developers",
    title: "Secure API for Developers",
    description: "Maple Proxy and APIs for builders who need attestable infrastructure.",
    icon: Code2
  }
] as const;

export const Route = createFileRoute("/solutions")({
  component: SolutionsBranch
});

function SolutionsHub() {
  return (
    <>
      <TopNav />
      <FullPageMain>
        <MarketingHeader
          title="Solutions"
          subtitle="Pick a vertical to see how Maple maps to your workflows. These pages are structure-only mocks for now."
        />
        <div className="w-full max-w-5xl mx-auto grid grid-cols-1 sm:grid-cols-2 gap-6">
          {TILES.map(({ to, title, description, icon: Icon }) => (
            <Link
              key={to}
              to={to}
              className="group flex flex-col gap-3 rounded-2xl border border-[hsl(var(--marketing-card-border))] bg-[hsl(var(--marketing-card))]/80 p-6 transition-all hover:border-[hsl(var(--maple-primary))]/35"
            >
              <div className="flex h-11 w-11 items-center justify-center rounded-lg bg-[hsl(var(--maple-primary))]/12 text-[hsl(var(--maple-primary))]">
                <Icon className="h-5 w-5" />
              </div>
              <h2 className="text-xl font-medium text-foreground">{title}</h2>
              <p className="text-[hsl(var(--marketing-text-muted))] font-light flex-1">
                {description}
              </p>
              <span className="inline-flex items-center gap-1 text-sm font-medium text-[hsl(var(--maple-primary))]">
                Open
                <ArrowRight className="h-4 w-4 group-hover:translate-x-0.5 transition-transform" />
              </span>
            </Link>
          ))}
        </div>
      </FullPageMain>
    </>
  );
}

function SolutionsBranch() {
  const pathname = useRouterState({ select: (s) => s.location.pathname });
  const isHub = pathname === "/solutions" || pathname === "/solutions/";
  if (isHub) {
    return <SolutionsHub />;
  }
  return <Outlet />;
}
