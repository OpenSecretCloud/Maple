import { Link } from "@tanstack/react-router";
import { VerificationStatus } from "./VerificationStatus";
import {
  ArrowRight,
  Check,
  Lock,
  MessageSquareMore,
  Shield,
  Sparkles,
  Laptop,
  Tag
} from "lucide-react";
import { Footer } from "./Footer";
import { useQuery } from "@tanstack/react-query";
import { getBillingService } from "@/billing/billingService";
import { PRICING_PLANS, type PlanFeature } from "@/config/pricingConfig";
import { isIOS } from "@/utils/platform";
import { ComparisonChart } from "./ComparisonChart";
import type { DiscountResponse } from "@/billing/billingApi";
import { Badge } from "@/components/ui/badge";

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
      {primary && (
        <div className="absolute inset-0 rounded-lg overflow-hidden">
          <div
            className="absolute inset-0 bg-gradient-to-r from-[hsl(var(--purple))]/0 via-[hsl(var(--primary-foreground))]/20 to-[hsl(var(--purple))]/0 opacity-50 animate-shimmer"
            style={{ transform: "translateX(-100%)" }}
          ></div>
        </div>
      )}
    </Link>
  );
}

function FeatureCard({
  icon: Icon,
  title,
  description,
  gradient = "from-[hsl(var(--purple))]/10 to-[hsl(var(--blue))]/10"
}: {
  icon: React.ElementType;
  title: string;
  description: string;
  gradient?: string;
}) {
  return (
    <div className={`feature-card ${gradient}`}>
      <div className="p-3 rounded-full bg-[hsl(var(--marketing-card))]/50 border border-[hsl(var(--purple))]/30 w-fit">
        <Icon className="w-6 h-6 text-[hsl(var(--purple))]" />
      </div>
      <h3 className="text-2xl font-medium text-foreground">{title}</h3>
      <p className="text-lg font-light text-[hsl(var(--marketing-text-muted))]">{description}</p>
    </div>
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

function PricingTier({
  name,
  price,
  description,
  features,
  ctaText,
  popular = false,
  productId = "",
  isIOS = false,
  discount
}: {
  name: string;
  price: string;
  description: string;
  features: PlanFeature[];
  ctaText: string;
  popular?: boolean;
  productId?: string;
  isIOS?: boolean;
  discount?: DiscountResponse;
}) {
  const isFreeplan = name.toLowerCase().includes("free");
  const discountPercent = discount?.active ? discount.percent_off : 0;
  const originalPrice = price.replace("$", "");
  const discountedPrice =
    !isFreeplan && discountPercent > 0
      ? `$${(Number(originalPrice) * ((100 - discountPercent) / 100)).toFixed(0)}`
      : price;
  const showDiscount = !isFreeplan && discountPercent > 0;

  return (
    <div
      className={`flex flex-col p-8 rounded-xl relative ${popular ? "border-2 border-[hsl(var(--purple))] bg-gradient-to-b from-[hsl(var(--marketing-card))] to-[hsl(var(--marketing-card))]/80 shadow-[0_0_30px_rgba(148,105,248,0.2)]" : "border border-[hsl(var(--marketing-card-border))] bg-[hsl(var(--marketing-card))]/50"}`}
    >
      {popular && (
        <div className="absolute -top-4 left-1/2 transform -translate-x-1/2 bg-[hsl(var(--purple))] text-[hsl(var(--marketing-card))] px-4 py-1 rounded-full text-sm font-medium text-center min-w-[110px] whitespace-normal">
          Most Popular
        </div>
      )}
      {showDiscount && (
        <Badge className="absolute -top-3 right-4 bg-gradient-to-r from-pink-500 to-orange-500 text-white">
          {discountPercent}% OFF
        </Badge>
      )}
      <h3 className="text-xl font-medium text-foreground">{name}</h3>
      <div className="mt-4 mb-2">
        <span className="text-4xl font-bold text-foreground">{discountedPrice}</span>
        {showDiscount && (
          <span className="text-xl line-through text-foreground/50 ml-2">{price}</span>
        )}
        {name === "Team" ? (
          <span className="text-[hsl(var(--marketing-text-muted))] ml-2">/user /mo</span>
        ) : (
          price !== "Free" && (
            <span className="text-[hsl(var(--marketing-text-muted))] ml-2">/mo</span>
          )
        )}
      </div>
      {showDiscount && discount?.active && discount.duration_months && (
        <p className="text-xs text-foreground/50 -mt-1 mb-2">
          {discountPercent}% off for first {discount.duration_months} month
          {discount.duration_months > 1 ? "s" : ""}
        </p>
      )}
      <p className="text-[hsl(var(--marketing-text-muted))] mb-6">{description}</p>
      <div className="flex flex-col gap-3 mb-8">
        {features.map((feature, index) => (
          <div key={index} className="flex items-start gap-2">
            {feature.text !== "" &&
              (feature.icon ||
                (feature.included ? (
                  <Check className="w-5 h-5 text-green-500 mt-0.5 flex-shrink-0" />
                ) : null))}
            <span className={`${feature.included ? "text-foreground/80" : "text-foreground/50"}`}>
              {feature.text}
            </span>
          </div>
        ))}
      </div>
      {/* For iOS devices, disable paid plans with "Coming Soon" text */}
      {isIOS && !isFreeplan ? (
        <button
          disabled={true}
          className="mt-auto py-3 px-6 rounded-lg text-center font-medium transition-all duration-300 
            dark:bg-white/90 dark:text-black dark:hover:bg-[hsl(var(--purple))]/80 dark:hover:text-[hsl(var(--foreground))] dark:active:bg-white/80
            bg-background text-foreground hover:bg-[hsl(var(--purple))] hover:text-[hsl(var(--foreground))] active:bg-background/80 
            border border-[hsl(var(--purple))]/30 hover:border-[hsl(var(--purple))]
            shadow-[0_0_15px_rgba(var(--purple-rgb),0.2)] hover:shadow-[0_0_25px_rgba(var(--purple-rgb),0.3)]
            opacity-50 cursor-not-allowed"
        >
          Coming Soon
        </button>
      ) : productId ? (
        // When we have a product ID, create a button that handles the navigation
        <button
          onClick={() => {
            // Use window.location to navigate with search params
            window.location.href = `/signup?next=/pricing&selected_plan=${productId}`;
          }}
          className="mt-auto py-3 px-6 rounded-lg text-center font-medium transition-all duration-300 
              dark:bg-white/90 dark:text-black dark:hover:bg-[hsl(var(--purple))]/80 dark:hover:text-[hsl(var(--foreground))] dark:active:bg-white/80
              bg-background text-foreground hover:bg-[hsl(var(--purple))] hover:text-[hsl(var(--foreground))] active:bg-background/80 
              border border-[hsl(var(--purple))]/30 hover:border-[hsl(var(--purple))]
              shadow-[0_0_15px_rgba(var(--purple-rgb),0.2)] hover:shadow-[0_0_25px_rgba(var(--purple-rgb),0.3)]"
        >
          {ctaText}
        </button>
      ) : (
        // Default link when no product ID is available
        <Link
          to="/signup"
          className="mt-auto py-3 px-6 rounded-lg text-center font-medium transition-all duration-300 
              dark:bg-white/90 dark:text-black dark:hover:bg-[hsl(var(--purple))]/80 dark:hover:text-[hsl(var(--foreground))] dark:active:bg-white/80
              bg-background text-foreground hover:bg-[hsl(var(--purple))] hover:text-[hsl(var(--foreground))] active:bg-background/80 
              border border-[hsl(var(--purple))]/30 hover:border-[hsl(var(--purple))]
              shadow-[0_0_15px_rgba(var(--purple-rgb),0.2)] hover:shadow-[0_0_25px_rgba(var(--purple-rgb),0.3)]"
        >
          {ctaText}
        </Link>
      )}
    </div>
  );
}

export { PricingTier };

export function Marketing() {
  // Use the platform detection function for iOS
  // Android doesn't have App Store restrictions, so we only need to check for iOS
  const isIOSPlatform = isIOS();

  // Fetch products to get product IDs for pricing tiers
  const { data: products } = useQuery({
    queryKey: ["marketing-products"],
    queryFn: async () => {
      try {
        const billingService = getBillingService();
        return await billingService.getProducts();
      } catch (error) {
        console.error("Error fetching products:", error);
        return [];
      }
    },
    retry: 1
  });

  // Fetch active discount/promotion
  const { data: discount } = useQuery<DiscountResponse>({
    queryKey: ["discount"],
    queryFn: async () => {
      const billingService = getBillingService();
      return await billingService.getDiscount();
    },
    staleTime: 5 * 60 * 1000
  });

  // Find product IDs for each tier
  const getProductId = (name: string) => {
    if (!products || products.length === 0) return "";

    const product = products.find((p) => p.name.toLowerCase() === name.toLowerCase() && p.active);
    return product ? product.id : "";
  };

  return (
    <div className="flex flex-col items-center w-full">
      {/* Hero Section */}
      <section className="w-full max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 pt-32 pb-24">
        <div className="grid grid-cols-1 lg:grid-cols-2 gap-16 items-center">
          <div className="flex flex-col gap-8">
            <div className="flex flex-col gap-2">
              <h1 className="text-6xl font-light tracking-tight mb-4">
                <span
                  className="dark:bg-gradient-to-r dark:from-foreground dark:to-[hsl(var(--blue))]
                             bg-gradient-to-r from-foreground from-5% via-[hsl(var(--purple))]/90 via-50% to-[hsl(var(--purple))]
                             text-transparent bg-clip-text"
                >
                  Private AI Chat
                </span>{" "}
                <br /> <span className="text-foreground">that's truly secure</span>
              </h1>
              <p className="text-xl text-[hsl(var(--marketing-text-muted))] font-light max-w-xl">
                End-to-end encryption means your conversations are confidential and protected at
                every step. Only you can access your data — not even we can read your chats.
              </p>
            </div>
            <div className="flex gap-4 flex-col sm:flex-row">
              <CTAButton to="/signup" primary>
                <Sparkles className="h-5 w-5" />
                Start Secure Chat
              </CTAButton>
              <CTAButton to="/login">Log In</CTAButton>
            </div>
            <div className="flex items-center gap-4 text-sm text-[hsl(var(--marketing-text-muted))] mt-2">
              <a
                href="#testimonials"
                className="flex items-center gap-4 hover:text-foreground/80 transition-colors duration-300"
                onClick={(e) => {
                  e.preventDefault();
                  document.getElementById("testimonials")?.scrollIntoView({ behavior: "smooth" });
                }}
              >
                <div className="flex -space-x-2">
                  <img
                    src="/ryan-g.jpg"
                    alt="User avatar"
                    className="w-8 h-8 rounded-full border border-[hsl(var(--marketing-card))]"
                  />
                  <img
                    src="/lauren-t.jpg"
                    alt="User avatar"
                    className="w-8 h-8 rounded-full border border-[hsl(var(--marketing-card))]"
                  />
                  <img
                    src="/javier-r.jpg"
                    alt="User avatar"
                    className="w-8 h-8 rounded-full border border-[hsl(var(--marketing-card))]"
                  />
                </div>
                <p className="ml-10 md:ml-0">
                  Trusted by professionals who handle sensitive client information
                </p>
              </a>
            </div>
          </div>
          <div
            className="relative dark:bg-gradient-to-br dark:from-[hsl(var(--purple))]/10 dark:to-[hsl(var(--blue))]/10 
               bg-gradient-to-br from-[hsl(var(--purple))]/5 to-[hsl(var(--purple))]/20 rounded-2xl p-1"
          >
            <div className="bg-[hsl(var(--marketing-card))]/80 backdrop-blur-sm rounded-xl overflow-hidden border border-[hsl(var(--marketing-card-border))]">
              <div className="bg-[hsl(var(--marketing-card))] p-3 border-b border-[hsl(var(--marketing-card-border))] flex items-center">
                <div className="flex gap-1.5">
                  <div className="w-3 h-3 rounded-full bg-foreground/20"></div>
                  <div className="w-3 h-3 rounded-full bg-foreground/20"></div>
                  <div className="w-3 h-3 rounded-full bg-foreground/20"></div>
                </div>
                <div className="mx-auto text-sm text-[hsl(var(--marketing-text-muted))]">
                  Encrypted Chat
                </div>
              </div>
              <div className="p-6 space-y-4">
                <div className="flex gap-3">
                  <div className="w-8 h-8 rounded-full bg-foreground/30 flex items-center justify-center text-background flex-shrink-0">
                    AI
                  </div>
                  <div className="bg-[hsl(var(--marketing-card-highlight))] p-3 rounded-xl rounded-tl-none text-foreground/90 text-sm max-w-xs">
                    How can I help you today? Your conversation is fully encrypted.
                  </div>
                </div>
                <div className="flex gap-3 justify-end">
                  <div className="bg-[hsl(var(--purple))]/20 p-3 rounded-xl rounded-tr-none text-foreground text-sm max-w-xs">
                    I need to discuss some sensitive information. Is this really secure?
                  </div>
                  <div className="w-8 h-8 rounded-full bg-foreground/10 flex items-center justify-center text-background flex-shrink-0">
                    U
                  </div>
                </div>
                <div className="flex gap-3">
                  <div className="w-8 h-8 rounded-full bg-foreground/30 flex items-center justify-center text-background flex-shrink-0">
                    AI
                  </div>
                  <div className="bg-[hsl(var(--marketing-card-highlight))] p-3 rounded-xl rounded-tl-none text-foreground/90 text-sm max-w-xs">
                    Yes, absolutely. Your messages are end-to-end encrypted. Even the server admins
                    can't read your conversations. We use secure enclaves and open-source code to
                    verify this.
                  </div>
                </div>
              </div>
            </div>
            <div className="absolute -bottom-3 -right-3 bg-[hsl(var(--purple))] text-[hsl(var(--marketing-card))] p-2 rounded-lg font-medium text-sm flex items-center gap-1.5">
              <Lock className="w-4 h-4" /> End-to-End Encrypted
            </div>
          </div>
        </div>

        <div className="flex flex-col sm:flex-row justify-center items-center gap-4 mt-8">
          <Link
            to="/downloads"
            className="inline-flex items-center gap-2 h-10 px-6 rounded-lg text-center font-medium transition-all duration-300
              dark:bg-white/90 dark:text-black dark:hover:bg-white dark:active:bg-white/80
              bg-black text-white hover:bg-black/90 active:bg-black/80
              border border-[hsl(var(--marketing-card-border))]"
          >
            <Laptop className="h-5 w-5" />
            <span>Desktop</span>
          </Link>
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
          {!isIOSPlatform && (
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
          )}
        </div>
      </section>

      {/* Features Section */}
      <section className="w-full py-20 dark:bg-[hsl(var(--section-alt))] bg-[hsl(var(--section-alt))]">
        <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8">
          <div className="text-center mb-16">
            <h2 className="text-4xl font-light mb-4">
              Privacy at <span className="text-[hsl(var(--purple))] font-medium">Every Layer</span>
            </h2>
            <p className="text-xl text-[hsl(var(--marketing-text-muted))] max-w-2xl mx-auto">
              Your security is our primary concern. We've engineered Maple to ensure your data
              remains yours alone, protected by cutting-edge encryption.
            </p>
          </div>

          <div className="grid grid-cols-1 md:grid-cols-3 gap-8">
            <FeatureCard
              icon={Shield}
              title="Your Device"
              description="All communications are encrypted locally on your device before being transmitted, ensuring your data is secured from the start."
              gradient="from-[hsl(var(--purple))]/5 to-[hsl(var(--purple))]/10"
            />
            <FeatureCard
              icon={Lock}
              title="Secure Server"
              description="Our servers can't read your data. We use secure enclaves in confidential computing environments to verify our infrastructure integrity."
              gradient="dark:from-[hsl(var(--blue))]/10 dark:to-[hsl(var(--blue))]/5 from-[hsl(var(--purple))]/10 to-[hsl(var(--purple))]/20"
            />
            <FeatureCard
              icon={Sparkles}
              title="AI Processing"
              description="Even during AI processing, your data remains encrypted. The entire pipeline through to the GPU is designed with privacy as the priority."
              gradient="dark:from-foreground/10 dark:to-foreground/5 from-[hsl(var(--purple))]/20 to-[hsl(var(--purple))]/5"
            />
          </div>
        </div>
      </section>

      {/* Proof/Verification Section */}
      <section className="w-full py-20">
        <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8">
          <div className="grid grid-cols-1 lg:grid-cols-2 gap-16 items-center">
            <div>
              <h2 className="text-4xl font-light mb-6">
                We{" "}
                <span className="dark:text-[hsl(var(--blue))] text-[hsl(var(--purple))] font-medium">
                  Prove
                </span>{" "}
                Our Security
              </h2>
              <p className="text-xl text-[hsl(var(--marketing-text-muted))] mb-8">
                Unlike other services that merely claim to be secure, we provide cryptographic
                proof. Our commitment to transparency means you don't have to trust us - you can
                verify it yourself.
              </p>
              <div className="text-l text-[hsl(var(--marketing-text-muted))]">
                Live Secure Enclave Verification
              </div>
              <div className="bg-[hsl(var(--marketing-card))]/80 border border-[hsl(var(--marketing-card-border))] rounded-xl p-6 mb-8">
                <VerificationStatus />
              </div>
              <Link
                to="/proof"
                className="dark:text-[hsl(var(--blue))] dark:hover:text-[hsl(var(--blue))]/80 
                           text-[hsl(var(--purple))] hover:text-[hsl(var(--purple))]/80 
                           flex items-center gap-2 font-medium"
              >
                Learn more about our verification system <ArrowRight className="h-4 w-4" />
              </Link>
            </div>
            <div
              className="relative dark:bg-gradient-to-br dark:from-[hsl(var(--purple))]/10 dark:to-[hsl(var(--blue))]/10 
                 bg-gradient-to-br from-[hsl(var(--purple))]/5 to-[hsl(var(--purple))]/20 rounded-2xl p-1"
            >
              <div className="bg-[hsl(var(--marketing-card))]/80 backdrop-blur-sm rounded-xl overflow-hidden border border-[hsl(var(--marketing-card-border))]">
                {/* Header */}
                <div className="bg-[hsl(var(--marketing-card))] p-3 border-b border-[hsl(var(--marketing-card-border))] flex items-center">
                  <div className="mx-auto text-sm text-[hsl(var(--marketing-text-muted))]">
                    Secure Server
                  </div>
                </div>

                {/* Main Content */}
                <div className="p-8 relative">
                  <div className="w-full mx-auto relative flex flex-col md:flex-row items-center justify-between px-2">
                    {/* Container for mobile view to ensure proper spacing */}
                    <div className="flex flex-col md:hidden items-center gap-8 w-full">
                      {/* Phone - Mobile */}
                      <div className="w-24 h-48 relative">
                        <div className="absolute inset-0 bg-[hsl(var(--marketing-card-highlight))] border border-[hsl(var(--purple))]/30 rounded-2xl">
                          <div className="m-2 bg-[hsl(var(--purple))]/10 rounded-lg border border-[hsl(var(--purple))]/30 h-[calc(100%-16px)]">
                            <div className="h-2 w-8 bg-[hsl(var(--purple))]/20 rounded mx-auto mt-2"></div>
                            <div className="h-2 w-12 bg-[hsl(var(--purple))]/20 rounded mx-auto mt-2"></div>
                          </div>
                        </div>
                        <div className="absolute -top-2 -right-2 dark:bg-[hsl(var(--blue))]/20 dark:border-[hsl(var(--blue))]/30 bg-[hsl(var(--purple))]/20 border-[hsl(var(--purple))]/30 p-2 rounded-lg border">
                          <div className="w-4 h-3 border-2 dark:border-[hsl(var(--blue))] border-[hsl(var(--purple))] rounded-t-lg mx-auto"></div>
                          <div className="w-6 h-5 dark:bg-[hsl(var(--blue))]/30 dark:border-[hsl(var(--blue))] bg-[hsl(var(--purple))]/30 border-2 border-[hsl(var(--purple))] rounded-lg -mt-0.5"></div>
                        </div>
                      </div>

                      {/* Vertical Connection Line - Mobile */}
                      <div className="h-16 w-px bg-[hsl(var(--purple))]/50 relative">
                        <div className="absolute -top-1 left-1/2 -translate-x-1/2 w-2 h-2 border-t-2 border-l-2 border-[hsl(var(--purple))] transform rotate-45"></div>
                        <div className="absolute -bottom-1 left-1/2 -translate-x-1/2 w-2 h-2 border-b-2 border-r-2 border-[hsl(var(--purple))] transform rotate-45"></div>
                        <div className="absolute left-1/2 top-1/2 -translate-x-1/2 -translate-y-1/2 whitespace-nowrap text-xs text-foreground/40 ml-1">
                          Encrypted Connection
                        </div>
                      </div>

                      {/* Server - Mobile */}
                      <div className="w-32 h-48 relative">
                        <div className="absolute inset-0 bg-[hsl(var(--marketing-card-highlight))] border border-[hsl(var(--purple))]/30 rounded-lg">
                          <div className="h-8 border-b border-[hsl(var(--purple))]/30 flex items-center justify-between px-3">
                            <div className="flex gap-1">
                              <div className="w-2 h-2 rounded-full bg-[hsl(var(--purple))]/40"></div>
                              <div className="w-2 h-2 rounded-full bg-[hsl(var(--purple))]/40"></div>
                            </div>
                          </div>
                          <div className="p-3 space-y-2">
                            <div className="h-7 bg-[hsl(var(--purple))]/20 rounded border border-[hsl(var(--purple))]/30 flex items-center px-2">
                              <div className="w-2 h-2 rounded-full bg-[hsl(var(--purple))]/40"></div>
                            </div>
                            <div className="h-7 bg-[hsl(var(--purple))]/20 rounded border border-[hsl(var(--purple))]/30 flex items-center px-2">
                              <div className="w-2 h-2 rounded-full bg-[hsl(var(--purple))]/40"></div>
                            </div>
                            <div className="h-7 bg-[hsl(var(--purple))]/20 rounded border border-[hsl(var(--purple))]/30 flex items-center px-2">
                              <div className="w-2 h-2 rounded-full bg-[hsl(var(--purple))]/40"></div>
                            </div>
                          </div>
                        </div>
                        <div className="absolute -top-2 -right-2 dark:bg-[hsl(var(--blue))]/20 dark:border-[hsl(var(--blue))]/30 bg-[hsl(var(--purple))]/20 border-[hsl(var(--purple))]/30 p-2 rounded-lg border">
                          <div className="w-4 h-3 border-2 dark:border-[hsl(var(--blue))] border-[hsl(var(--purple))] rounded-t-lg mx-auto"></div>
                          <div className="w-6 h-5 dark:bg-[hsl(var(--blue))]/30 dark:border-[hsl(var(--blue))] bg-[hsl(var(--purple))]/30 border-2 border-[hsl(var(--purple))] rounded-lg -mt-0.5"></div>
                        </div>
                      </div>
                    </div>

                    {/* Desktop layout - hidden on mobile */}
                    <div className="hidden md:flex items-center justify-between w-full">
                      {/* Phone - Desktop */}
                      <div className="w-24 h-48 relative">
                        {/* Same phone content as mobile */}
                        <div className="absolute inset-0 bg-[hsl(var(--marketing-card-highlight))] border border-[hsl(var(--purple))]/30 rounded-2xl">
                          <div className="m-2 bg-[hsl(var(--purple))]/10 rounded-lg border border-[hsl(var(--purple))]/30 h-[calc(100%-16px)]">
                            <div className="h-2 w-8 bg-[hsl(var(--purple))]/20 rounded mx-auto mt-2"></div>
                            <div className="h-2 w-12 bg-[hsl(var(--purple))]/20 rounded mx-auto mt-2"></div>
                          </div>
                        </div>
                        <div className="absolute -top-2 -right-2 dark:bg-[hsl(var(--blue))]/20 dark:border-[hsl(var(--blue))]/30 bg-[hsl(var(--purple))]/20 border-[hsl(var(--purple))]/30 p-2 rounded-lg border">
                          <div className="w-4 h-3 border-2 dark:border-[hsl(var(--blue))] border-[hsl(var(--purple))] rounded-t-lg mx-auto"></div>
                          <div className="w-6 h-5 dark:bg-[hsl(var(--blue))]/30 dark:border-[hsl(var(--blue))] bg-[hsl(var(--purple))]/30 border-2 border-[hsl(var(--purple))] rounded-lg -mt-0.5"></div>
                        </div>
                      </div>

                      {/* Horizontal Connection Line - Desktop */}
                      <div className="flex-1 mx-2 flex items-center justify-center">
                        <div className="h-px w-4/5 bg-[hsl(var(--purple))]/50 relative">
                          <div className="absolute left-0 top-1/2 -translate-y-1/2 w-2 h-2 border-t-2 border-l-2 border-[hsl(var(--purple))] transform -rotate-45"></div>
                          <div className="absolute right-0 top-1/2 -translate-y-1/2 w-2 h-2 border-t-2 border-r-2 border-[hsl(var(--purple))] transform rotate-45"></div>
                          <div className="absolute -top-4 left-1/2 -translate-x-1/2 whitespace-nowrap text-xs text-foreground/40">
                            Encrypted Connection
                          </div>
                        </div>
                      </div>

                      {/* Server - Desktop */}
                      <div className="w-32 h-48 relative">
                        {/* Same server content as mobile */}
                        <div className="absolute inset-0 bg-[hsl(var(--marketing-card-highlight))] border border-[hsl(var(--purple))]/30 rounded-lg">
                          <div className="h-8 border-b border-[hsl(var(--purple))]/30 flex items-center justify-between px-3">
                            <div className="flex gap-1">
                              <div className="w-2 h-2 rounded-full bg-[hsl(var(--purple))]/40"></div>
                              <div className="w-2 h-2 rounded-full bg-[hsl(var(--purple))]/40"></div>
                            </div>
                          </div>
                          <div className="p-3 space-y-2">
                            <div className="h-7 bg-[hsl(var(--purple))]/20 rounded border border-[hsl(var(--purple))]/30 flex items-center px-2">
                              <div className="w-2 h-2 rounded-full bg-[hsl(var(--purple))]/40"></div>
                            </div>
                            <div className="h-7 bg-[hsl(var(--purple))]/20 rounded border border-[hsl(var(--purple))]/30 flex items-center px-2">
                              <div className="w-2 h-2 rounded-full bg-[hsl(var(--purple))]/40"></div>
                            </div>
                            <div className="h-7 bg-[hsl(var(--purple))]/20 rounded border border-[hsl(var(--purple))]/30 flex items-center px-2">
                              <div className="w-2 h-2 rounded-full bg-[hsl(var(--purple))]/40"></div>
                            </div>
                          </div>
                        </div>
                        <div className="absolute -top-2 -right-2 dark:bg-[hsl(var(--blue))]/20 dark:border-[hsl(var(--blue))]/30 bg-[hsl(var(--purple))]/20 border-[hsl(var(--purple))]/30 p-2 rounded-lg border">
                          <div className="w-4 h-3 border-2 dark:border-[hsl(var(--blue))] border-[hsl(var(--purple))] rounded-t-lg mx-auto"></div>
                          <div className="w-6 h-5 dark:bg-[hsl(var(--blue))]/30 dark:border-[hsl(var(--blue))] bg-[hsl(var(--purple))]/30 border-2 border-[hsl(var(--purple))] rounded-lg -mt-0.5"></div>
                        </div>
                      </div>
                    </div>
                  </div>

                  {/* Crypto Text */}
                  <div className="mt-8 flex gap-4 justify-center overflow-hidden text-xs font-mono">
                    <div className="text-foreground/60">0x7B4...</div>
                    <div className="text-foreground/60">ECDSA P-384</div>
                    <div className="text-foreground/60">SHA-384</div>
                  </div>
                </div>
              </div>

              {/* Status Indicator */}
              <div className="absolute -bottom-3 -right-3 bg-[hsl(var(--purple))] text-[hsl(var(--marketing-card))] p-2 rounded-lg font-medium text-sm flex items-center gap-1.5">
                <Shield className="w-4 h-4" /> Secure Enclave Verified
              </div>
            </div>
          </div>
        </div>
      </section>

      {/* Comparison Chart */}
      <ComparisonChart />

      {/* Testimonials */}
      <section
        id="testimonials"
        className="w-full py-20 dark:bg-[hsl(var(--section-alt))] bg-[hsl(var(--section-alt))]"
      >
        <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8">
          <div className="text-center mb-16">
            <h2 className="text-4xl font-light mb-4">
              What Our <span className="text-foreground font-medium">Users Say</span>
            </h2>
            <p className="text-xl text-[hsl(var(--marketing-text-muted))] max-w-2xl mx-auto">
              Hear from those who've made privacy a priority with Maple AI.
              <br />
              (Quotes are real, names are changed for privacy)
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
              quote="I've been using Maple AI to help my clients with their tax and financial needs. The encryption means that their sensitive financial data and situations stay private. It really gives me peace of mind."
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

      {/* Pricing Section */}
      <section className="w-full py-20">
        <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8">
          <div className="text-center mb-16">
            <h2 className="text-4xl font-light mb-4">
              Simple, <span className="text-[hsl(var(--purple))] font-medium">Transparent</span>{" "}
              Pricing
            </h2>
            <p className="text-xl text-[hsl(var(--marketing-text-muted))] max-w-2xl mx-auto">
              No hidden fees. Choose the plan that works for your needs.
            </p>
          </div>

          {/* Promotion Banner */}
          {discount?.active && (
            <div className="mb-8 bg-gradient-to-r from-pink-500/10 to-orange-500/10 border border-pink-500/30 rounded-lg p-4 flex items-center gap-3">
              <div className="rounded-full bg-gradient-to-r from-pink-500 to-orange-500 p-2">
                <Tag className="w-5 h-5 text-white" />
              </div>
              <div className="flex-1">
                <p className="font-medium text-foreground">{discount.name}</p>
                <p className="text-sm text-[hsl(var(--marketing-text-muted))]">
                  {discount.description}
                </p>
              </div>
              <Badge className="bg-gradient-to-r from-pink-500 to-orange-500 text-white text-lg px-3 py-1">
                {discount.percent_off}% OFF
              </Badge>
            </div>
          )}

          <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-8">
            {PRICING_PLANS.filter((plan) => {
              // Always hide Starter plan on marketing page
              return plan.name.toLowerCase() !== "starter";
            }).map((plan) => (
              <PricingTier
                key={plan.name}
                name={plan.name}
                price={plan.price}
                description={plan.description}
                features={plan.features}
                ctaText={plan.ctaText}
                popular={plan.popular}
                productId={getProductId(plan.name)}
                isIOS={isIOSPlatform}
                discount={discount}
              />
            ))}
          </div>
        </div>
      </section>

      {/* CTA Section */}
      <section className="w-full py-20 dark:bg-gradient-to-r dark:from-[hsl(var(--background))] dark:via-[hsl(var(--section-alt))] dark:to-[hsl(var(--background))] bg-gradient-to-r from-[hsl(var(--section-alt))] via-[hsl(var(--marketing-card))] to-[hsl(var(--section-alt))]">
        <div className="max-w-5xl mx-auto px-4 sm:px-6 lg:px-8">
          <div className="bg-gradient-to-br from-[hsl(var(--purple))]/20 to-foreground/20 p-1 rounded-2xl">
            <div className="dark:bg-[hsl(var(--background))]/95 bg-[hsl(var(--marketing-card))]/95 rounded-2xl p-12 text-center">
              <h2 className="text-4xl font-light mb-4">
                Ready to Chat <span className="text-foreground font-medium">Securely?</span>
              </h2>
              <p className="text-xl text-[hsl(var(--marketing-text-muted))] max-w-2xl mx-auto mb-8">
                Join privacy-conscious users who've made the switch to truly secure AI chat.
              </p>
              <div className="flex justify-center gap-4 flex-col sm:flex-row">
                <CTAButton to="/signup" primary>
                  <MessageSquareMore className="h-5 w-5" />
                  Start Chatting Securely
                </CTAButton>
                <CTAButton to="/login">Log In</CTAButton>
              </div>
            </div>
          </div>
        </div>
      </section>

      <Footer />
    </div>
  );
}
