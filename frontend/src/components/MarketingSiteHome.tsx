import { Link } from "@tanstack/react-router";
import {
  ArrowRight,
  Download,
  FlaskConical,
  MessageSquareMore,
  Microscope,
  Sparkles
} from "lucide-react";
import { ComparisonChart } from "@/components/ComparisonChart";
import { Footer } from "@/components/Footer";
import { isIOS } from "@/utils/platform";

const AI_MODELS = [
  { src: "/badge-openai-logo.png", alt: "OpenAI", labels: ["OpenAI GPT-OSS"] },
  { src: "/badge-google-logo.png", alt: "Google", labels: ["Google Gemma"] },
  { src: "/badge-deepseek-logo.png", alt: "DeepSeek", labels: ["DeepSeek R1"] },
  { src: "/badge-kimi-logo.png", alt: "Moonshot", labels: ["Kimi K2.5"] },
  { src: "/badge-qwen-logo.png", alt: "Qwen", labels: ["Qwen3-VL"] },
  { src: "/badge-meta-logo.png", alt: "Meta", labels: ["Meta Llama"] }
];

function CTAButton({
  children,
  to,
  primary = false
}: {
  children: React.ReactNode;
  to: string;
  primary?: boolean;
}) {
  return (
    <Link to={to} className={primary ? "cta-button-primary" : "cta-button-secondary"}>
      {children}
    </Link>
  );
}

function TestimonialCard({
  quote,
  author,
  role,
  avatar
}: {
  quote: string;
  author: string;
  role: string;
  avatar: string;
}) {
  return (
    <div className="p-6 rounded-xl bg-[hsl(var(--marketing-card))]/80 border border-[hsl(var(--marketing-card-border))] flex flex-col gap-4">
      <p className="text-lg italic text-foreground/90 font-light">&ldquo;{quote}&rdquo;</p>
      <div className="flex items-center gap-3 mt-2">
        <img src={avatar} alt={author} className="w-12 h-12 rounded-full" />
        <div>
          <p className="font-medium text-foreground">{author}</p>
          <p className="text-sm text-[hsl(var(--marketing-text-muted))]">{role}</p>
        </div>
      </div>
    </div>
  );
}

function ProductSpotlightCard({
  title,
  description,
  icon: Icon,
  to,
  accentClass
}: {
  title: string;
  description: string;
  icon: React.ElementType;
  to: string;
  accentClass: string;
}) {
  return (
    <Link
      to={to}
      className={`group relative flex flex-col gap-4 rounded-2xl border border-[hsl(var(--marketing-card-border))] bg-[hsl(var(--marketing-card))]/80 p-8 transition-all hover:border-[hsl(var(--maple-primary))]/40 hover:shadow-[0_0_40px_rgba(var(--maple-primary-rgb),0.12)] ${accentClass}`}
    >
      <div className="flex h-12 w-12 items-center justify-center rounded-xl bg-[hsl(var(--maple-primary))]/15 text-[hsl(var(--maple-primary))]">
        <Icon className="h-6 w-6" />
      </div>
      <div>
        <h2 className="text-2xl font-medium text-foreground">{title}</h2>
        <p className="mt-2 text-lg font-light text-[hsl(var(--marketing-text-muted))]">
          {description}
        </p>
      </div>
      <span className="mt-auto inline-flex items-center gap-2 text-sm font-medium text-[hsl(var(--maple-primary))]">
        Learn more
        <ArrowRight className="h-4 w-4 transition-transform group-hover:translate-x-0.5" />
      </span>
    </Link>
  );
}

export function MarketingSiteHome() {
  const isIOSPlatform = isIOS();

  return (
    <div className="flex flex-col items-center w-full">
      <section className="w-full max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 pt-32 pb-16">
        <div className="mx-auto max-w-3xl text-center mb-14">
          <h1 className="text-5xl sm:text-6xl font-light tracking-tight mb-4">
            <span className="dark:bg-gradient-to-r dark:from-foreground dark:to-[hsl(var(--blue))] bg-gradient-to-r from-foreground from-5% via-[hsl(var(--maple-primary))]/90 via-50% to-[hsl(var(--maple-primary))] text-transparent bg-clip-text">
              The AI platform
            </span>{" "}
            <span className="text-foreground">for privileged information</span>
          </h1>
          <p className="text-xl text-[hsl(var(--marketing-text-muted))] font-light">
            Your data stays yours with end-to-end encryption. Explore Agent for everyday work and
            Research for deep analysis—then take Maple anywhere.
          </p>
        </div>

        <div className="grid grid-cols-1 lg:grid-cols-2 gap-6 lg:gap-8">
          <ProductSpotlightCard
            title="Maple Agent"
            description="Fast, encrypted chat with frontier models—built for workflows that touch sensitive client and firm data."
            icon={MessageSquareMore}
            to="/agent"
            accentClass=""
          />
          <ProductSpotlightCard
            title="Maple Research"
            description="Long-context research and document-heavy tasks in a confidential environment you can verify."
            icon={Microscope}
            to="/research"
            accentClass=""
          />
        </div>

        <div className="mt-10 flex flex-col sm:flex-row justify-center items-center gap-4">
          <CTAButton to="/downloads" primary>
            <Download className="h-5 w-5" />
            Download
          </CTAButton>
          <CTAButton to="/login">
            <Sparkles className="h-5 w-5" />
            Log in
          </CTAButton>
        </div>
        {!isIOSPlatform && (
          <div className="mt-6 flex flex-wrap justify-center gap-4">
            <a
              href="https://apps.apple.com/us/app/id6743764835"
              target="_blank"
              rel="noopener noreferrer"
              className="inline-block"
            >
              <img
                src="/app-store-badge.svg"
                alt="Download on the App Store"
                className="h-10 w-auto"
              />
            </a>
            <a
              href="https://play.google.com/store/apps/details?id=cloud.opensecret.maple"
              target="_blank"
              rel="noopener noreferrer"
              className="inline-block"
            >
              <img
                src="/google-play-badge.png"
                alt="Get it on Google Play"
                className="h-10 w-auto"
              />
            </a>
          </div>
        )}
      </section>

      <section
        id="platform"
        className="w-full py-16 dark:bg-[hsl(var(--section-alt))] bg-[hsl(var(--section-alt))]"
      >
        <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8">
          <div className="flex flex-col sm:flex-row sm:items-end sm:justify-between gap-4 mb-12">
            <div>
              <div className="inline-flex items-center gap-2 rounded-full border border-[hsl(var(--marketing-card-border))] bg-[hsl(var(--marketing-card))]/60 px-3 py-1 text-sm text-[hsl(var(--marketing-text-muted))] mb-4">
                <FlaskConical className="h-4 w-4 text-[hsl(var(--maple-primary))]" />
                Platform overview
              </div>
              <h2 className="text-4xl font-light">
                Models &{" "}
                <span className="text-[hsl(var(--maple-primary))] font-medium">comparison</span>
              </h2>
              <p className="mt-2 text-xl text-[hsl(var(--marketing-text-muted))] max-w-2xl">
                Full-size open models without shipping your prompts to model vendors.
              </p>
            </div>
            <Link
              to="/proof"
              className="text-[hsl(var(--maple-primary))] hover:text-[hsl(var(--maple-primary))]/80 flex items-center gap-2 font-medium shrink-0"
            >
              Security &amp; attestation <ArrowRight className="h-4 w-4" />
            </Link>
          </div>

          <div className="grid grid-cols-2 md:grid-cols-3 gap-8 mb-16">
            {AI_MODELS.map((model) => (
              <div key={model.alt} className="flex flex-col items-center">
                <img
                  src={model.src}
                  alt={model.alt}
                  loading="lazy"
                  decoding="async"
                  className="max-w-full h-24 object-contain mb-4"
                />
                <div className="flex flex-col items-center">
                  {model.labels.map((label, index) => (
                    <span key={index} className="text-lg font-medium text-foreground">
                      {label}
                    </span>
                  ))}
                </div>
              </div>
            ))}
          </div>
          <p className="text-center text-lg text-[hsl(var(--marketing-text-muted))] max-w-2xl mx-auto mb-16">
            None of your data is transmitted to these companies. Get the best models without the
            usual data tradeoffs.
          </p>
        </div>
      </section>

      <ComparisonChart />

      <section
        id="testimonials"
        className="w-full py-20 dark:bg-[hsl(var(--section-alt))] bg-[hsl(var(--section-alt))]"
      >
        <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8">
          <div className="text-center mb-16">
            <h2 className="text-4xl font-light mb-4">
              Voices from <span className="text-foreground font-medium">the community</span>
            </h2>
            <p className="text-xl text-[hsl(var(--marketing-text-muted))] max-w-2xl mx-auto">
              Real feedback from people who rely on Maple for sensitive work—presented as cards
              (we&apos;ll swap in live Twitter embeds when you&apos;re ready).
            </p>
          </div>

          <div className="grid grid-cols-1 md:grid-cols-3 gap-8">
            <TestimonialCard
              quote="For me, knowing it's end-to-end encrypted reassures me that proprietary information, client information, or any IP won't be exposed."
              author="Ryan G."
              role="VP Data & Analytics"
              avatar="/ryan-g.jpg"
            />
            <TestimonialCard
              quote="I've been using Maple AI to help my clients with their tax and financial needs. The encryption means that their sensitive financial data and situations stay private."
              author="Lauren T."
              role="Certified Public Accountant"
              avatar="/lauren-t.jpg"
            />
            <TestimonialCard
              quote="I am fascinated by the possibilities of AI in the legal field. Finding Maple and knowing confidential client information is encrypted has been a game changer for our team."
              author="Javier R."
              role="Attorney"
              avatar="/javier-r.jpg"
            />
          </div>
        </div>
      </section>

      <section className="w-full py-20 dark:bg-gradient-to-r dark:from-[hsl(var(--background))] dark:via-[hsl(var(--section-alt))] dark:to-[hsl(var(--background))] bg-gradient-to-r from-[hsl(var(--section-alt))] via-[hsl(var(--marketing-card))] to-[hsl(var(--section-alt))]">
        <div className="max-w-5xl mx-auto px-4 sm:px-6 lg:px-8">
          <div className="bg-gradient-to-br from-[hsl(var(--maple-primary))]/20 to-foreground/20 p-1 rounded-2xl">
            <div className="dark:bg-[hsl(var(--background))]/95 bg-[hsl(var(--marketing-card))]/95 rounded-2xl p-12 text-center">
              <h2 className="text-4xl font-light mb-4">
                Ready for <span className="text-foreground font-medium">encrypted AI?</span>
              </h2>
              <p className="text-xl text-[hsl(var(--marketing-text-muted))] max-w-2xl mx-auto mb-8">
                Download the app, or start in the browser—then invite your team when you&apos;re
                ready.
              </p>
              <div className="flex justify-center gap-4 flex-col sm:flex-row">
                <CTAButton to="/downloads" primary>
                  <Download className="h-5 w-5" />
                  Download Maple
                </CTAButton>
                <CTAButton to="/pricing">View pricing</CTAButton>
              </div>
            </div>
          </div>
        </div>
      </section>

      <Footer />
    </div>
  );
}
