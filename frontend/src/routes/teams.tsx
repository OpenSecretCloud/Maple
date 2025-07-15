import { createFileRoute } from "@tanstack/react-router";
import { TopNav } from "@/components/TopNav";
import { FullPageMain } from "@/components/FullPageMain";
import { MarketingHeader } from "@/components/MarketingHeader";
import { Shield, Lock, Users, Sparkles, Building2, ArrowRight, FileText } from "lucide-react";
import { Link } from "@tanstack/react-router";
import { PricingTier } from "@/components/Marketing";
import { PRICING_PLANS } from "@/config/pricingConfig";

export const Route = createFileRoute("/teams")({
  component: TeamsPage,
  head: () => ({
    meta: [
      {
        title: "AI for Teams - Secure AI Workflow & Private AI for Teams | Maple AI"
      },
      {
        name: "description",
        content:
          "Secure AI for teams with end-to-end encryption and confidential computing. Perfect for legal AI, financial AI, and accounting AI workflows. Private AI collaboration platform."
      },
      {
        name: "keywords",
        content:
          "AI for Teams, Secure AI Workflow, Private AI for Teams, Legal AI, Financial AI, Accounting AI, Confidential Computing, Team AI Collaboration, Private AI Platform, Secure AI Chat"
      },
      {
        property: "og:title",
        content: "AI for Teams - Secure AI Workflow & Private AI for Teams | Maple AI"
      },
      {
        property: "og:description",
        content:
          "Secure AI for teams with end-to-end encryption and confidential computing. Perfect for legal AI, financial AI, and accounting AI workflows. Private AI collaboration platform."
      },
      {
        property: "og:type",
        content: "website"
      },
      {
        property: "og:url",
        content: "https://trymaple.ai/teams"
      },
      {
        property: "og:image",
        content: "https://trymaple.ai/twitter-card.jpg"
      },
      {
        name: "twitter:card",
        content: "summary_large_image"
      },
      {
        name: "twitter:title",
        content: "AI for Teams - Secure AI Workflow & Private AI for Teams | Maple AI"
      },
      {
        name: "twitter:description",
        content:
          "Secure AI for teams with end-to-end encryption and confidential computing. Perfect for legal AI, financial AI, and accounting AI workflows."
      },
      {
        name: "twitter:image",
        content: "https://trymaple.ai/twitter-card.jpg"
      }
    ]
  })
});

function FeatureCard({
  icon: Icon,
  title,
  description
}: {
  icon: React.ElementType;
  title: string;
  description: string;
}) {
  return (
    <div className="feature-card from-[hsl(var(--purple))]/10 to-[hsl(var(--blue))]/10">
      <div className="p-3 rounded-full bg-[hsl(var(--marketing-card))]/50 border border-[hsl(var(--purple))]/30 w-fit">
        <Icon className="w-6 h-6 text-[hsl(var(--purple))]" aria-hidden="true" />
      </div>
      <h3 className="text-2xl font-medium text-foreground">{title}</h3>
      <p className="text-lg font-light text-[hsl(var(--marketing-text-muted))]">{description}</p>
    </div>
  );
}

function TeamsPage() {
  return (
    <>
      <TopNav />
      <FullPageMain>
        <MarketingHeader
          title={
            <h1>
              <span>
                Secure AI for <span className="text-[hsl(var(--purple))]">Teams</span>
              </span>
            </h1>
          }
          subtitle={
            <span>
              Private AI workflow platform for organizations that value trust. Collaborate with
              confidence using our secure AI workspace—perfect for legal AI, financial AI, and
              accounting AI workflows.
            </span>
          }
        />

        {/* Hero CTA */}
        <div className="flex flex-col items-center gap-4 mt-8 mb-12">
          <Link
            to="/signup"
            search={{ selected_plan: "team" }}
            className="cta-button-primary flex items-center gap-2 px-8 py-4 text-xl font-light rounded-lg shadow-lg transition-all duration-300"
          >
            Get Started with Teams <ArrowRight className="w-5 h-5" />
          </Link>
          <a
            href="mailto:team@opensecret.cloud"
            className="cta-button-secondary flex items-center gap-2 px-8 py-4 text-xl font-light rounded-lg border border-[hsl(var(--purple))]/30 hover:border-[hsl(var(--purple))] mt-2"
          >
            Request a Demo
          </a>
        </div>

        {/* Features Section */}
        <section className="w-full py-16 dark:bg-[hsl(var(--section-alt))] bg-[hsl(var(--section-alt))]">
          <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8">
            <div className="text-center mb-16">
              <h2 className="text-4xl font-light mb-4">
                Why Choose{" "}
                <span className="text-[hsl(var(--purple))] font-medium">
                  Maple Teams for Your AI Workflow?
                </span>
              </h2>
              <p className="text-xl text-[hsl(var(--marketing-text-muted))] max-w-2xl mx-auto">
                Maple Teams is the secure AI workflow platform built for organizations that handle
                sensitive data—law firms, therapy practices, non-profits, and businesses that demand
                privacy. Share AI access, pool usage, and collaborate securely with end-to-end
                encryption and confidential computing for your legal AI, financial AI, and
                accounting AI needs.
              </p>
            </div>
            <div className="grid grid-cols-1 md:grid-cols-3 gap-8">
              <FeatureCard
                icon={Users}
                title="Secure AI Team Collaboration"
                description="Share AI credits across your team and control access from a central dashboard. Perfect for legal AI teams, financial AI departments, and accounting AI workflows."
              />
              <FeatureCard
                icon={FileText}
                title="Private AI Document Analysis"
                description="Upload documents and images for secure AI analysis—summarize, extract insights, or translate securely as a team. Supported formats include PDF, DOCX, Excel, JPG, PNG, and more."
              />
              <FeatureCard
                icon={Building2}
                title="Confidential Data Processing"
                description="Use Maple's private AI to redact or anonymize sensitive client information before sharing with other cloud tools or AI. Protect privacy and stay compliant."
              />
            </div>
          </div>
        </section>

        {/* Security Section */}
        <section className="w-full py-16">
          <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8">
            <div className="text-center mb-16">
              <h2 className="text-4xl font-light mb-4">
                Secure AI Workflow{" "}
                <span className="text-[hsl(var(--purple))] font-medium">Security by Design</span>
              </h2>
              <p className="text-xl text-[hsl(var(--marketing-text-muted))] max-w-2xl mx-auto">
                Maple Teams is built from the ground up for privacy and compliance. Your secure AI
                workflow is protected at every step—by cryptography, hardware, and open-source
                transparency, making it ideal for legal AI, financial AI, and accounting AI
                applications.
              </p>
            </div>
            <div className="grid grid-cols-1 md:grid-cols-3 gap-8">
              <FeatureCard
                icon={Shield}
                title="End-to-End Encryption for AI"
                description="All conversations and files are encrypted on your device and only decrypted inside secure enclaves. No one—not even Maple—can access your plaintext data in our private AI platform."
              />
              <FeatureCard
                icon={Lock}
                title="Confidential Computing Platform"
                description="Maple runs inside hardware-secured enclaves (AWS Nitro, Nvidia TEE) with cryptographic proof. Your organization's AI workflow data is protected at every step."
              />
              <FeatureCard
                icon={Sparkles}
                title="Private AI Models"
                description="Teams users get access to the best models Maple offers, including advanced open source LLMs. Your data is never sent to model creators—it always stays within secure enclaves."
              />
            </div>
          </div>
        </section>

        {/* How It Works Section */}
        <section className="w-full py-16">
          <div className="max-w-5xl mx-auto px-4 sm:px-6 lg:px-8">
            <div className="text-center mb-12">
              <h2 className="text-3xl font-light mb-4">How Our Secure AI Workflow Works</h2>
              <div className="flex justify-center">
                <ol className="text-lg text-[hsl(var(--marketing-text-muted))] max-w-xl text-left list-decimal list-inside space-y-2">
                  <li>Purchase seats for your secure AI team</li>
                  <li>Invite members by email to your private AI workspace</li>
                  <li>Use AI securely with your data, encrypted and protected</li>
                </ol>
              </div>
              <div className="flex justify-center mt-4 mb-8">
                <a
                  href="https://blog.trymaple.ai/manage-your-team-on-maple/"
                  target="_blank"
                  rel="noopener noreferrer"
                  className="text-[hsl(var(--purple))] underline text-lg font-medium hover:text-[hsl(var(--purple))]/80 transition-colors"
                >
                  Learn More About Team Management
                </a>
              </div>
            </div>
            <div className="flex flex-col md:flex-row gap-8 items-center justify-center">
              <div className="flex-1 flex flex-col gap-4">
                <div className="flex items-center gap-3">
                  <Building2 className="w-8 h-8 text-[hsl(var(--purple))]" />
                  <span className="text-xl font-medium">
                    Legal AI & Financial AI for Businesses
                  </span>
                </div>
                <p className="text-[hsl(var(--marketing-text-muted))]">
                  Maple Teams is trusted by organizations in law, healthcare, finance, and
                  consulting. Draft contracts with legal AI, analyze client data with financial AI,
                  and collaborate on sensitive projects with full privacy and compliance using our
                  secure AI workflow.
                </p>
              </div>
              <div className="flex-1 flex flex-col gap-4">
                <div className="flex items-center gap-3">
                  <Users className="w-8 h-8 text-[hsl(var(--purple))]" />
                  <span className="text-xl font-medium">Private AI for Non-Profits</span>
                </div>
                <p className="text-[hsl(var(--marketing-text-muted))]">
                  Non-profits and advocacy groups use Maple's private AI platform to securely manage
                  sensitive information, collaborate on grant writing, and protect the privacy of
                  those they serve—no IT headaches, just secure, private AI for your mission.
                </p>
              </div>
            </div>
          </div>
        </section>

        {/* Pricing/CTA Section */}
        <section className="w-full py-16 dark:bg-[hsl(var(--section-alt))] bg-[hsl(var(--section-alt))]">
          <div className="max-w-4xl mx-auto px-4 sm:px-6 lg:px-8 text-center">
            <h2 className="text-3xl font-light mb-4">Simple, Transparent Pricing for Teams</h2>
            <p className="text-lg text-[hsl(var(--marketing-text-muted))] mb-8">
              $30/month per seat for secure AI workflow access. All features included. Priority
              support for teams. No hidden fees.
            </p>
            <div className="flex justify-center">
              <div className="max-w-md w-full">
                <PricingTier
                  name={PRICING_PLANS.find((plan) => plan.name === "Team")!.name}
                  price={PRICING_PLANS.find((plan) => plan.name === "Team")!.price}
                  description={PRICING_PLANS.find((plan) => plan.name === "Team")!.description}
                  features={PRICING_PLANS.find((plan) => plan.name === "Team")!.features}
                  ctaText={PRICING_PLANS.find((plan) => plan.name === "Team")!.ctaText}
                  popular={PRICING_PLANS.find((plan) => plan.name === "Team")!.popular}
                />
              </div>
            </div>
            <div className="mt-6 text-[hsl(var(--marketing-text-muted))] text-sm">
              Need a larger deployment or have compliance questions?{" "}
              <a href="mailto:team@opensecret.cloud" className="underline hover:text-foreground">
                Contact us
              </a>{" "}
              for enterprise options. View our full{" "}
              <Link to="/pricing" className="underline hover:text-foreground">
                pricing plans
              </Link>{" "}
              or{" "}
              <Link to="/signup" className="underline hover:text-foreground">
                get started
              </Link>{" "}
              with our secure AI workflow today.
            </div>
          </div>
        </section>
      </FullPageMain>
    </>
  );
}
