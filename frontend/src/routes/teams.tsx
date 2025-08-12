import { createFileRoute } from "@tanstack/react-router";
import { TopNav } from "@/components/TopNav";
import { FullPageMain } from "@/components/FullPageMain";
import { MarketingHeader } from "@/components/MarketingHeader";
import { Shield, Lock, Users, Sparkles, Building2, ArrowRight, FileText } from "lucide-react";
import { Link } from "@tanstack/react-router";
import { PricingTier } from "@/components/Marketing";
import { PRICING_PLANS } from "@/config/pricingConfig";

export const Route = createFileRoute("/teams")({
  component: TeamsPage
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
        <Icon className="w-6 h-6 text-[hsl(var(--purple))]" />
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
            <span>
              Secure AI for <span className="text-[hsl(var(--purple))]">Teams</span>
            </span>
          }
          subtitle={
            <span>
              Protect your trade secrets while using AI. <br />
              No data is shared with advertisers, competitors, or third parties. Period.
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
            <div className="text-center mb-8">
              <h2 className="text-4xl font-light mb-4">
                Why Choose{" "}
                <span className="text-[hsl(var(--purple))] font-medium">Maple Teams?</span>
              </h2>
              <p className="text-xl text-[hsl(var(--marketing-text-muted))] max-w-2xl mx-auto">
                Experience the productivity gains of AI without the data tracking.
                <br /> Maple Teams scales AI across your entire organization to meet your needs.
              </p>
            </div>

            {/* Centered Image */}
            <div className="flex justify-center mb-8">
              <img
                src="/maple-teams-startup-office.jpg"
                alt="A company office with different departments of Accounting, Finance, Marketing, Engineering, and Sales"
                className="max-w-xl w-full h-auto rounded-lg shadow-lg"
              />
            </div>

            <div className="grid grid-cols-1 md:grid-cols-3 gap-8">
              <FeatureCard
                icon={Users}
                title="Pooled Usage"
                description="Share AI credits across your team and control access from a central dashboard. Add or remove users anytime."
              />
              <FeatureCard
                icon={FileText}
                title="Advanced Model Access"
                description="Teams get priority access to the latest AI models, including advanced language models for complex analysis, coding assistance, and creative tasks."
              />
              <FeatureCard
                icon={Building2}
                title="Sanitize Client Data"
                description="Use Maple to redact or anonymize sensitive client information before sharing with other cloud tools or AI. Protect privacy and stay compliant."
              />
            </div>
          </div>
        </section>

        {/* AI Models Section */}
        <section className="w-full py-16">
          <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8">
            <div className="text-center mb-12">
              <h2 className="text-4xl font-light mb-4">
                Powerful AI models.{" "}
                <span className="text-[hsl(var(--purple))] font-medium">No data sharing.</span>
              </h2>
              <p className="text-xl text-[hsl(var(--marketing-text-muted))] max-w-2xl mx-auto">
                We use open-source models from the biggest providers.
              </p>
            </div>
            {(() => {
              const models = [
                { src: "/badge-openai-logo.png", alt: "OpenAI", label: "OpenAI GPT-OSS" },
                { src: "/badge-google-logo.png", alt: "Google", label: "Google Gemma" },
                { src: "/badge-deepseek-logo.png", alt: "DeepSeek", label: "DeepSeek R1" },
                { src: "/badge-meta-logo.png", alt: "Meta", label: "Meta Llama" }
              ];
              return (
                <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-8">
                  {models.map((model) => (
                    <div key={model.label} className="flex flex-col items-center">
                      <img
                        src={model.src}
                        alt={model.alt}
                        loading="lazy"
                        decoding="async"
                        className="w-72 h-24 object-contain mb-4"
                      />
                      <span className="text-lg font-medium text-foreground">{model.label}</span>
                    </div>
                  ))}
                </div>
              );
            })()}
            <div className="text-center">
              <p className="text-xl text-[hsl(var(--marketing-text-muted))] max-w-2xl mx-auto">
                <br />
                None of your data is transmitted to these companies.
                <br />
                Get the best without the mess.
              </p>
            </div>
          </div>
        </section>

        {/* Security Section */}
        <section className="w-full py-16">
          <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8">
            <div className="text-center mb-8">
              <h2 className="text-4xl font-light mb-4">
                Security by <span className="text-[hsl(var(--purple))] font-medium">Design</span>
              </h2>
              <p className="text-xl text-[hsl(var(--marketing-text-muted))] max-w-2xl mx-auto">
                You don't share company secrets with competitors, so why should your AI? Whether
                you're designing products in a highly competitive market or dealing with sensitive
                client information at a firm or non-profit, Teams protects it all.
              </p>
            </div>

            {/* Centered Image */}
            <div className="flex justify-center mb-8">
              <img
                src="/audio-hardware-engineers.jpg"
                alt="Two audio hardware engineers collaborating at work using secure Maple AI"
                className="max-w-2xl w-full h-auto rounded-lg shadow-lg"
              />
            </div>

            <div className="grid grid-cols-1 md:grid-cols-3 gap-8">
              <FeatureCard
                icon={Shield}
                title="End-to-End Encryption"
                description="All conversations and files are encrypted on your device and only decrypted inside secure enclaves. No one—not even Maple—can access your plaintext data."
              />
              <FeatureCard
                icon={Lock}
                title="Confidential Computing"
                description="Maple runs inside hardware-secured enclaves (AWS Nitro, Nvidia TEE) with cryptographic proof. Your organization’s data is protected at every step."
              />
              <FeatureCard
                icon={Sparkles}
                title="Open Source Models"
                description="Teams users get access to the best models Maple offers, including advanced open source LLMs. Your data is never sent to model creators—it always stays within secure enclaves."
              />
            </div>
          </div>
        </section>

        {/* How It Works Section */}
        <section className="w-full py-16">
          <div className="max-w-5xl mx-auto px-4 sm:px-6 lg:px-8">
            <div className="text-center mb-12">
              <h2 className="text-3xl font-light mb-4">How Maple Teams Works</h2>
              <div className="flex justify-center">
                <ol className="text-lg text-[hsl(var(--marketing-text-muted))] max-w-xl text-left list-decimal list-inside space-y-2">
                  <li>Purchase seats for your team</li>
                  <li>Invite members by email</li>
                  <li>Use AI securely with your data, encrypted</li>
                </ol>
              </div>
              <div className="flex justify-center mt-4 mb-8">
                <a
                  href="https://blog.trymaple.ai/manage-your-team-on-maple/"
                  target="_blank"
                  rel="noopener noreferrer"
                  className="text-[hsl(var(--purple))] underline text-lg font-medium hover:text-[hsl(var(--purple))]/80 transition-colors"
                >
                  Learn More
                </a>
              </div>
            </div>
            <div className="flex flex-col md:flex-row gap-8 items-center justify-center">
              <div className="flex-1 flex flex-col gap-4">
                <div className="flex items-center gap-3">
                  <Building2 className="w-8 h-8 text-[hsl(var(--purple))]" />
                  <span className="text-xl font-medium">For Businesses</span>
                </div>
                <p className="text-[hsl(var(--marketing-text-muted))]">
                  Maple Teams is trusted by organizations in big tech, legal, higher education,
                  healthcare, finance, consulting, and more. Draft contracts, analyze client data,
                  and collaborate on sensitive projects with full privacy and compliance.
                </p>
                <div className="flex justify-center mt-4">
                  <img
                    src="/legal-office.jpg"
                    alt="Legal professionals working in a modern law office with secure AI assistance"
                    className="max-w-md w-full h-auto rounded-lg shadow-md"
                  />
                </div>
              </div>
              <div className="flex-1 flex flex-col gap-4">
                <div className="flex items-center gap-3">
                  <Users className="w-8 h-8 text-[hsl(var(--purple))]" />
                  <span className="text-xl font-medium">For Non-Profits</span>
                </div>
                <p className="text-[hsl(var(--marketing-text-muted))]">
                  Non-profits and advocacy groups use Maple to securely manage sensitive
                  information, collaborate on grant writing, and protect the privacy of those they
                  serve—no IT headaches, just secure, private AI for your mission.
                </p>
                <div className="flex justify-center mt-4">
                  <img
                    src="/social-worker-office.jpg"
                    alt="Social workers collaborating in a non-profit office using secure AI tools"
                    className="max-w-md w-full h-auto rounded-lg shadow-md"
                  />
                </div>
              </div>
            </div>
          </div>
        </section>

        {/* Pricing/CTA Section */}
        <section className="w-full py-16 dark:bg-[hsl(var(--section-alt))] bg-[hsl(var(--section-alt))]">
          <div className="max-w-4xl mx-auto px-4 sm:px-6 lg:px-8 text-center">
            <h2 className="text-3xl font-light mb-4">Simple, Transparent Pricing</h2>
            <p className="text-lg text-[hsl(var(--marketing-text-muted))] mb-8">
              $30/month per seat. All features included. No hidden fees. Cancel anytime.
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
              for enterprise options.
            </div>
          </div>
        </section>
      </FullPageMain>
    </>
  );
}
