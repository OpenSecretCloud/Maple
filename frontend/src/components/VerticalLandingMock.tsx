import { Link } from "@tanstack/react-router";
import { TopNav } from "@/components/TopNav";
import { FullPageMain } from "@/components/FullPageMain";
import { MarketingHeader } from "@/components/MarketingHeader";
import { ArrowRight, Download } from "lucide-react";

export function VerticalLandingMock({
  title,
  subtitle,
  bullets
}: {
  title: string;
  subtitle: string;
  bullets: string[];
}) {
  return (
    <>
      <TopNav />
      <FullPageMain>
        <MarketingHeader title={title} subtitle={subtitle} />
        <div className="w-full max-w-3xl mx-auto rounded-2xl border border-dashed border-[hsl(var(--marketing-card-border))] bg-[hsl(var(--marketing-card))]/40 p-8 text-left">
          <p className="text-sm font-medium uppercase tracking-wide text-[hsl(var(--marketing-text-muted))] mb-4">
            Sitemap mock
          </p>
          <ul className="list-disc pl-5 space-y-2 text-[hsl(var(--marketing-text-muted))]">
            {bullets.map((b) => (
              <li key={b}>{b}</li>
            ))}
          </ul>
          <div className="mt-8 flex flex-wrap gap-4">
            <Link to="/downloads" className="cta-button-primary inline-flex items-center gap-2">
              <Download className="h-5 w-5" />
              Download
            </Link>
            <Link to="/solutions" className="cta-button-secondary inline-flex items-center gap-2">
              All solutions
              <ArrowRight className="h-4 w-4" />
            </Link>
          </div>
        </div>
      </FullPageMain>
    </>
  );
}
