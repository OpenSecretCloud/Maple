import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { useState, useEffect, useCallback } from "react";
import { useOpenSecret } from "@opensecret/react";
import { TopNav } from "@/components/TopNav";
import { FullPageMain } from "@/components/FullPageMain";
import { getBillingService } from "@/billing/billingService";
import { useQuery } from "@tanstack/react-query";
import { MarketingHeader } from "@/components/MarketingHeader";
import { Loader2, Check, AlertTriangle, Bitcoin } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { useLocalState } from "@/state/useLocalState";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { PRICING_PLANS } from "@/config/pricingConfig";
import { VerificationModal } from "@/components/VerificationModal";
import { TeamSeatDialog } from "@/components/TeamSeatDialog";
import { isIOS, isAndroid, isMobile } from "@/utils/platform";
import packageJson from "../../package.json";

// File type constants for upload features
const SUPPORTED_IMAGE_FORMATS = [".jpg", ".png", ".webp"];
const SUPPORTED_DOCUMENT_FORMATS = [".pdf", ".txt", ".md"];

type PricingSearchParams = {
  selected_plan?: string;
  success?: boolean;
  canceled?: boolean;
};

interface Product {
  id: string;
  name: string;
  default_price: {
    unit_amount: number;
  };
  description?: string;
  active?: boolean;
  is_available?: boolean;
}

function PricingSkeletonCard() {
  return (
    <div className="flex flex-col border-[hsl(var(--marketing-card-border))] bg-[hsl(var(--marketing-card))]/75 text-foreground p-4 sm:p-6 md:p-8 border rounded-lg relative overflow-hidden">
      <div className="absolute inset-0 -translate-x-full animate-[shimmer_2s_infinite] bg-gradient-to-r from-transparent via-foreground/5 to-transparent"></div>
      <div className="grid grid-rows-[auto_1fr_auto_auto] h-full gap-4 sm:gap-6 md:gap-8">
        <div className="h-6 sm:h-8 bg-foreground/10 rounded-md w-1/2"></div>
        <div className="space-y-2 sm:space-y-3">
          <div className="h-4 bg-foreground/10 rounded-md w-3/4"></div>
          <div className="h-4 bg-foreground/10 rounded-md w-5/6"></div>
          <div className="h-4 bg-foreground/10 rounded-md w-2/3"></div>
        </div>
        <div className="h-6 sm:h-8 bg-foreground/10 rounded-md w-1/3"></div>
        <div className="h-12 sm:h-14 bg-foreground/10 rounded-lg w-full"></div>
      </div>
    </div>
  );
}

function PricingFAQ() {
  return (
    <div className="flex flex-col gap-8 border-[hsl(var(--marketing-card-border))] bg-[hsl(var(--marketing-card))]/75 text-foreground p-6 sm:p-8 border rounded-lg mt-8 max-w-7xl mx-auto w-full px-4 sm:px-6 lg:px-8">
      <h3 className="text-2xl font-medium">FAQ</h3>

      <div className="flex flex-col gap-4">
        <details className="group">
          <summary className="cursor-pointer text-lg font-medium hover:text-foreground/80">
            What is the difference between the plans?
          </summary>
          <div className="mt-4 text-[hsl(var(--marketing-text-muted))] space-y-2">
            <p>The plans are sized to grow with your needs.</p>
            <ul className="list-disc list-inside space-y-1 ml-4">
              <li>
                Free: 25 messages per week, resets Sunday 00:00 UTC. Max length on individual
                messages.
              </li>
              <li>Pro: Generous usage for power users with a high monthly cap</li>
              <li>Max: 20x more usage than Pro for maximum power users</li>
              <li>Team: Even more usage per team member with unified billing</li>
              <li>Enterprise: Message us at team@opensecret.cloud</li>
            </ul>
          </div>
        </details>

        <details className="group">
          <summary className="cursor-pointer text-lg font-medium hover:text-foreground/80">
            How private is Maple?
          </summary>
          <p className="mt-4 text-[hsl(var(--marketing-text-muted))]">
            Encrypted end to end. Maple uses confidential computing to secure the code that access
            user data and LLM data. Your account has its own private key that encrypts your chats
            and the responses from the AI model. Every user has their own personal data vault that
            can't be read by anyone else, not even us.
          </p>
        </details>

        <details className="group">
          <summary className="cursor-pointer text-lg font-medium hover:text-foreground/80">
            How do you synchronize my chat history across devices?
          </summary>
          <p className="mt-4 text-[hsl(var(--marketing-text-muted))]">
            We use a secure synchronization protocol that ensures your encrypted chat history is
            synced across all your devices. This means that you can start a conversation on one
            device and pick it up where you left off on another device, without compromising your
            security or privacy.
          </p>
        </details>

        <details className="group">
          <summary className="cursor-pointer text-lg font-medium hover:text-foreground/80">
            Is this safe to use with my company's confidential information?
          </summary>
          <p className="mt-4 text-[hsl(var(--marketing-text-muted))]">
            The service is encrypted end-to-end, so your confidential information is private between
            you and the AI. Consult your company's security policy.
          </p>
        </details>

        <details className="group">
          <summary className="cursor-pointer text-lg font-medium hover:text-foreground/80">
            Can companies use my data to train their AI models?
          </summary>
          <p className="mt-4 text-[hsl(var(--marketing-text-muted))]">
            No. When you chat with AI in Maple, nobody knows what is being said back and forth. Thus
            data is not able to be used for training new AI models by any company.
          </p>
        </details>

        <details className="group">
          <summary className="cursor-pointer text-lg font-medium hover:text-foreground/80">
            Which file types are supported for document and image upload?
          </summary>
          <div className="mt-4 text-[hsl(var(--marketing-text-muted))] space-y-4">
            <p>We support a range of file types for both images and documents.</p>
            <div>
              <strong>Images:</strong>
              <ul className="list-disc list-inside ml-4 mt-2 space-y-1">
                {SUPPORTED_IMAGE_FORMATS.map((format) => (
                  <li key={format}>
                    <code className="bg-foreground/10 px-1 py-0.5 rounded text-sm">{format}</code>
                  </li>
                ))}
              </ul>
            </div>
            <div>
              <strong>Documents (Desktop/Mobile apps only):</strong>
              <ul className="list-disc list-inside ml-4 mt-2 space-y-1">
                {SUPPORTED_DOCUMENT_FORMATS.map((format) => (
                  <li key={format}>
                    <code className="bg-foreground/10 px-1 py-0.5 rounded text-sm">{format}</code>
                  </li>
                ))}
              </ul>
            </div>
            <p>There is a 1 file limit per chat prompt with a 10MB file size limit per file.</p>
          </div>
        </details>

        <details className="group">
          <summary className="cursor-pointer text-lg font-medium hover:text-foreground/80">
            How did you build this?
          </summary>
          <p className="mt-4 text-[hsl(var(--marketing-text-muted))]">
            Maple is made using{" "}
            <a
              href="https://opensecret.cloud"
              className="text-foreground hover:text-foreground/80 underline"
            >
              OpenSecret
            </a>
            , an encrypted backend for developers. It handles private keys, encrypted data sync,
            private AI, and more automatically. We're building OpenSecret as a way to bring better
            privacy to users by turning on encryption by default. If you're a developer, go see how
            easy it is to build with.
          </p>
        </details>
      </div>
    </div>
  );
}

function PricingPage() {
  const [checkoutError, setCheckoutError] = useState<string>("");
  const [loadingProductId, setLoadingProductId] = useState<string | null>(null);
  const [useBitcoin, setUseBitcoin] = useState(false);
  const [showTeamSeatDialog, setShowTeamSeatDialog] = useState(false);
  const [pendingTeamProductId, setPendingTeamProductId] = useState<string | null>(null);
  const navigate = useNavigate();
  const os = useOpenSecret();
  const { setBillingStatus } = useLocalState();
  const isLoggedIn = !!os.auth.user;
  const { selected_plan } = Route.useSearch();

  // Use platform detection functions
  const isIOSPlatform = isIOS();
  const isAndroidPlatform = isAndroid();
  const isMobilePlatform = isMobile();

  // Fetch billing status if user is logged in
  const { data: freshBillingStatus, isLoading: isBillingStatusLoading } = useQuery({
    queryKey: ["billingStatus"],
    queryFn: async () => {
      const billingService = getBillingService();
      const status = await billingService.getBillingStatus();
      setBillingStatus(status);
      return status;
    },
    enabled: isLoggedIn
  });

  // Auto-enable Bitcoin toggle for Zaprite users (except on mobile platforms)
  useEffect(() => {
    if (freshBillingStatus?.payment_provider === "zaprite" && !isMobilePlatform) {
      setUseBitcoin(true);
    }
  }, [freshBillingStatus?.payment_provider, isMobilePlatform]);

  // Always try to fetch portal URL if logged in
  const { data: portalUrl } = useQuery({
    queryKey: ["portalUrl"],
    queryFn: async () => {
      if (!isLoggedIn) return null;
      const billingService = getBillingService();
      try {
        return await billingService.getPortalUrl();
      } catch (error) {
        console.error("Error fetching portal URL:", error);
        return null;
      }
    },
    enabled: isLoggedIn
  });

  const {
    data: products,
    error: productsError,
    isLoading: productsLoading
  } = useQuery({
    queryKey: ["products", isIOSPlatform, isAndroidPlatform],
    queryFn: async () => {
      try {
        const billingService = getBillingService();
        // Send version for mobile builds (iOS needs it for App Store restrictions)
        if (isIOSPlatform || isAndroidPlatform) {
          // Get version from package.json
          const version = `v${packageJson.version}`;
          console.log("[Billing] Fetching products with version:", version);
          return await billingService.getProducts(version);
        }
        return await billingService.getProducts();
      } catch (error) {
        console.error("Error fetching products:", error);
        throw error;
      }
    },
    retry: 1 // Only retry once for faster error feedback
  });

  // Handle payment callback status from URL params
  const { success, canceled } = Route.useSearch();

  // Check if team plan purchase and redirect after success
  useEffect(() => {
    if (success && freshBillingStatus?.product_name?.toLowerCase().includes("team")) {
      // Redirect to home with team_setup param
      navigate({ to: "/", search: { team_setup: true }, replace: true });
    }
  }, [success, freshBillingStatus, navigate]);

  const getButtonText = (product: Product) => {
    if (loadingProductId === product.id) {
      return (
        <>
          <Loader2 className="w-5 h-5 animate-spin" />
          Processing...
        </>
      );
    }

    const targetPlanName = product.name.toLowerCase();
    const isTeamPlan = targetPlanName.includes("team");
    const isFreeplan = targetPlanName.includes("free");

    // Show "Not available in app" for iOS paid plans if server says not available
    // Android can support paid plans (no App Store restrictions)
    if (isIOSPlatform && !isFreeplan && product.is_available === false) {
      return "Not available in app";
    }

    // Show Start Chatting for all plans when not logged in
    if (!isLoggedIn) {
      return "Start Chatting";
    }

    const currentPlanName = freshBillingStatus?.product_name?.toLowerCase();
    const isCurrentPlan = currentPlanName === targetPlanName;

    // If user is on Zaprite plan, show Contact Us
    if (freshBillingStatus?.payment_provider === "zaprite") {
      return "Contact Us";
    }

    // For team plan
    if (isTeamPlan) {
      if (isCurrentPlan) {
        return "Manage Plan";
      }
      return "Upgrade";
    }

    // For free plan
    if (targetPlanName.includes("free")) {
      if (isCurrentPlan) {
        return "Start Chatting";
      }
      return "Downgrade";
    }

    // For paid plans
    if (isCurrentPlan) {
      return "Manage Plan";
    }

    // Only compare plan levels if we have both products and current plan name
    if (currentPlanName && products) {
      const sortedProducts = [...products].sort(
        (a, b) => a.default_price.unit_amount - b.default_price.unit_amount
      );
      const currentPlanIndex = sortedProducts.findIndex(
        (p) => p.name.toLowerCase() === currentPlanName
      );
      const targetPlanIndex = sortedProducts.findIndex(
        (p) => p.name.toLowerCase() === targetPlanName
      );

      if (currentPlanIndex !== -1 && targetPlanIndex !== -1 && targetPlanIndex > currentPlanIndex) {
        return "Upgrade";
      }
    }

    return "Downgrade";
  };

  const newHandleSubscribe = useCallback(
    async (productId: string, quantity?: number) => {
      if (!isLoggedIn) {
        navigate({ to: "/signup" });
        return;
      }

      // Check if email is verified before proceeding to checkout (skip in dev mode)
      if (!import.meta.env.DEV && !os.auth.user?.user.email_verified) {
        console.log("Email verification required before checkout");
        return;
      }

      setLoadingProductId(productId);
      try {
        const billingService = getBillingService();
        const email = os.auth.user?.user.email;
        if (!email) {
          throw new Error("User email not found");
        }

        // For mobile platforms (iOS and Android), use Universal Links / App Links
        if (isMobilePlatform) {
          if (useBitcoin) {
            await billingService.createZapriteCheckoutSession(
              email,
              productId,
              `https://trymaple.ai/payment-success?source=zaprite`,
              quantity
            );
          } else {
            await billingService.createCheckoutSession(
              email,
              productId,
              `https://trymaple.ai/payment-success?source=stripe`,
              `https://trymaple.ai/payment-canceled?source=stripe`,
              quantity
            );
          }
        } else {
          // For web or desktop, use regular URLs
          if (useBitcoin) {
            await billingService.createZapriteCheckoutSession(
              email,
              productId,
              `${window.location.origin}/pricing?success=true`,
              quantity
            );
          } else {
            await billingService.createCheckoutSession(
              email,
              productId,
              `${window.location.origin}/pricing?success=true`,
              `${window.location.origin}/pricing?canceled=true`,
              quantity
            );
          }
        }
      } catch (err) {
        console.error("Subscribe error:", err);
        setCheckoutError(err instanceof Error ? err.message : "Failed to start checkout");
      } finally {
        setLoadingProductId(null);
      }
    },
    [
      isLoggedIn,
      navigate,
      os.auth.user?.user.email,
      os.auth.user?.user.email_verified,
      useBitcoin,
      isMobilePlatform
    ]
  );

  const handleButtonClick = useCallback(
    (product: Product) => {
      const targetPlanName = product.name.toLowerCase();
      const isFreeplan = targetPlanName.includes("free");
      const isTeamPlan = targetPlanName.includes("team");

      // Disable clicks for iOS paid plans if server says not available
      // Android can support paid plans
      if (isIOSPlatform && !isFreeplan && product.is_available === false) {
        return;
      }

      if (!isLoggedIn) {
        if (!targetPlanName.includes("free")) {
          // For paid plans, redirect to signup with the plan selection
          navigate({
            to: "/signup",
            search: {
              next: "/pricing",
              selected_plan: product.id
            }
          });
          return;
        }
        navigate({ to: "/signup" });
        return;
      }

      // If user is on Zaprite plan, redirect to email
      if (freshBillingStatus?.payment_provider === "zaprite") {
        window.location.href = "mailto:support@opensecret.cloud";
        return;
      }

      const currentPlanName = freshBillingStatus?.product_name?.toLowerCase();
      const isCurrentlyOnFreePlan = currentPlanName?.includes("free");
      const isTargetFreePlan = targetPlanName.includes("free");

      // If on free plan and clicking free plan, go home
      if (isCurrentlyOnFreePlan && isTargetFreePlan) {
        navigate({ to: "/" });
        return;
      }

      // If user is on free plan and clicking a paid plan, use checkout URL
      if (isCurrentlyOnFreePlan && !isTargetFreePlan) {
        // For team plans, show seat selection dialog first
        if (isTeamPlan) {
          setPendingTeamProductId(product.id);
          setShowTeamSeatDialog(true);
          return;
        }
        newHandleSubscribe(product.id);
        return;
      }

      // For all other cases (upgrades/downgrades between paid plans, or downgrades to free),
      // use portal URL if it exists
      if (portalUrl) {
        // Open in external browser for mobile platforms (iOS and Android)
        if (isMobilePlatform) {
          console.log(
            "[Billing] Mobile platform detected, using opener plugin to launch external browser for portal"
          );

          // Use the Tauri opener plugin for mobile platforms
          import("@tauri-apps/api/core")
            .then((coreModule) => {
              return coreModule.invoke("plugin:opener|open_url", { url: portalUrl });
            })
            .then(() => {
              console.log("[Billing] Successfully opened portal URL in external browser");
            })
            .catch((err) => {
              console.error("[Billing] Failed to open external browser:", err);
              alert("Failed to open browser. Please try again.");
            });
        } else {
          // Default browser opening for desktop and web platforms
          window.open(portalUrl, "_blank");
        }
        return;
      }

      // If no portal URL exists and it's not a free plan user upgrading,
      // create checkout session
      // For team plans, show seat selection dialog first
      if (isTeamPlan) {
        setPendingTeamProductId(product.id);
        setShowTeamSeatDialog(true);
        return;
      }
      newHandleSubscribe(product.id);
    },
    [
      isLoggedIn,
      freshBillingStatus,
      navigate,
      portalUrl,
      newHandleSubscribe,
      isIOSPlatform,
      isMobilePlatform
    ]
  );

  const handleTeamSeatConfirm = useCallback(
    (seats: number) => {
      if (pendingTeamProductId) {
        newHandleSubscribe(pendingTeamProductId, seats);
        setPendingTeamProductId(null);
      }
    },
    [pendingTeamProductId, newHandleSubscribe]
  );

  useEffect(() => {
    let isSubscribed = true;

    // If user is logged in and there's a selected plan, trigger checkout (except on iOS for paid plans)
    if (isLoggedIn && selected_plan && !isBillingStatusLoading) {
      if (loadingProductId) return; // Prevent multiple triggers
      const product = products?.find((p) => p.id === selected_plan);
      if (product && isSubscribed) {
        handleButtonClick(product);
      }
    }

    return () => {
      isSubscribed = false;
    };
  }, [
    isLoggedIn,
    selected_plan,
    isBillingStatusLoading,
    products,
    loadingProductId,
    handleButtonClick,
    isIOSPlatform
  ]);

  // Show loading state if we're fetching initial data
  if (productsLoading || (isLoggedIn && isBillingStatusLoading)) {
    return (
      <>
        <TopNav />
        <FullPageMain>
          <MarketingHeader
            title={
              <h2 className="text-6xl font-light mb-0">
                Simple, <span className="text-[hsl(var(--purple))]">Transparent</span> Pricing
              </h2>
            }
            subtitle={
              <div className="space-y-2">
                <p>Start with our free tier and upgrade as you grow.</p>
                <p>All plans include end-to-end encrypted AI chat.</p>
              </div>
            }
          />

          <div className="pt-8 w-full max-w-7xl mx-auto grid grid-cols-1 md:grid-cols-4 gap-4 md:gap-4 lg:gap-6 px-4 sm:px-6 lg:px-8">
            <PricingSkeletonCard />
            <PricingSkeletonCard />
            <PricingSkeletonCard />
            <PricingSkeletonCard />
          </div>

          <PricingFAQ />
        </FullPageMain>
      </>
    );
  }

  // Show error state for any errors (products error or checkout error)
  if (productsError || checkoutError) {
    const errorMessage =
      checkoutError ||
      (productsError instanceof Error ? productsError.message : "An unexpected error occurred");

    return (
      <>
        <TopNav />
        <FullPageMain>
          <MarketingHeader title="Pricing" subtitle="Choose the plan that's right for you." />

          <div className="pt-8 w-full grid grid-cols-1 md:grid-cols-3 gap-4 md:gap-4 lg:gap-6 px-4 sm:px-6 lg:px-8">
            <div className="flex flex-col border-[hsl(var(--marketing-card-border))] bg-[hsl(var(--marketing-card))]/75 text-foreground p-8 border rounded-lg col-span-full">
              <div className="flex flex-col items-center text-center gap-4">
                <div className="rounded-full bg-red-500/10 p-3">
                  <AlertTriangle className="w-6 h-6 text-red-500" />
                </div>
                <div className="space-y-2">
                  <h3 className="text-xl font-medium">Unable to Load Pricing</h3>
                  <p className="text-[hsl(var(--marketing-text-muted))]">{errorMessage}</p>
                </div>
                <Button
                  onClick={() => window.location.reload()}
                  className="mt-4 
                    dark:bg-white/90 dark:text-black dark:hover:bg-[hsl(var(--purple))]/80 dark:hover:text-[hsl(var(--foreground))] dark:active:bg-white/80
                    bg-background text-foreground hover:bg-[hsl(var(--purple))] hover:text-[hsl(var(--foreground))] active:bg-background/80 
                    border border-[hsl(var(--purple))]/30 hover:border-[hsl(var(--purple))]
                    px-8 py-4 rounded-lg text-xl font-light transition-all duration-300 
                    shadow-[0_0_15px_rgba(var(--purple-rgb),0.2)] hover:shadow-[0_0_25px_rgba(var(--purple-rgb),0.3)]"
                >
                  Retry
                </Button>
              </div>
            </div>
          </div>

          <PricingFAQ />
        </FullPageMain>
      </>
    );
  }

  return (
    <>
      <TopNav />
      <FullPageMain>
        <MarketingHeader
          title={
            <h2 className="text-6xl font-light mb-0">
              Simple, <span className="text-[hsl(var(--purple))]">Transparent</span> Pricing
            </h2>
          }
          subtitle={
            <div className="space-y-2">
              <p>Start with our free tier and upgrade as you grow.</p>
              <p>All plans include end-to-end encrypted AI chat.</p>
            </div>
          }
        />

        {/* Payment Callback Status Messages */}
        {success && (
          <div className="w-full max-w-7xl mx-auto mt-4 px-4 sm:px-6 lg:px-8">
            <div className="bg-green-50 dark:bg-green-900/20 border border-green-200 dark:border-green-900/50 text-green-800 dark:text-green-100 rounded-lg p-4 flex items-center gap-3">
              <div className="rounded-full bg-green-100 dark:bg-green-800 p-1">
                <Check className="w-5 h-5 text-green-600 dark:text-green-200" />
              </div>
              <p>Payment successful! Your subscription has been updated.</p>
            </div>
          </div>
        )}

        {canceled && (
          <div className="w-full max-w-7xl mx-auto mt-4 px-4 sm:px-6 lg:px-8">
            <div className="bg-amber-50 dark:bg-amber-900/20 border border-amber-200 dark:border-amber-900/50 text-amber-800 dark:text-amber-100 rounded-lg p-4 flex items-center gap-3">
              <div className="rounded-full bg-amber-100 dark:bg-amber-800 p-1">
                <AlertTriangle className="w-5 h-5 text-amber-600 dark:text-amber-200" />
              </div>
              <p>Payment canceled. Your subscription remains unchanged.</p>
            </div>
          </div>
        )}

        {(() => {
          const filteredPlans = PRICING_PLANS.filter((plan) => {
            // Always hide Starter plan unless user is currently on Starter
            const isStarterPlan = plan.name.toLowerCase() === "starter";
            const isUserOnStarter = freshBillingStatus?.product_name?.toLowerCase() === "starter";

            if (isStarterPlan && !isUserOnStarter) {
              return false;
            }

            return true;
          });

          const gridColumns = filteredPlans.length === 4 ? "lg:grid-cols-4" : "lg:grid-cols-3";

          return (
            <div
              className={`pt-8 w-full max-w-7xl mx-auto grid grid-cols-1 ${gridColumns} gap-4 md:gap-4 lg:gap-6 px-4 sm:px-6 lg:px-8`}
            >
              {filteredPlans.map((plan) => {
                // Find the matching product from server data
                const product = products?.find(
                  (p) => p.name.toLowerCase() === plan.name.toLowerCase() && p.active
                );

                // If no product found from server, use plan data
                if (!product && plan.name !== "Free") return null;

                const isCurrentPlan =
                  isLoggedIn &&
                  freshBillingStatus?.product_name?.toLowerCase() === plan.name.toLowerCase();
                const isTeamPlan = plan.name.toLowerCase().includes("team");
                const isFreeplan = plan.name.toLowerCase().includes("free");

                // Calculate prices
                let monthlyOriginalPrice = plan.price.replace("$", "");
                let monthlyPrice = monthlyOriginalPrice;

                if (product) {
                  monthlyOriginalPrice = (product.default_price.unit_amount / 100).toFixed(0);
                  monthlyPrice = monthlyOriginalPrice;
                }

                // Calculate yearly prices for Bitcoin (10% off)
                const yearlyDiscountedPrice = product
                  ? (Math.floor(product.default_price.unit_amount * 12 * 0.9) / 100).toFixed(2)
                  : (Number(monthlyOriginalPrice) * 12 * 0.9).toFixed(2);

                // Calculate monthly equivalent of yearly Bitcoin price
                const monthlyEquivalentPrice = (Number(yearlyDiscountedPrice) / 12).toFixed(2);

                const displayPrice =
                  useBitcoin && !isFreeplan ? monthlyEquivalentPrice : monthlyPrice;

                return (
                  <div
                    key={plan.name}
                    className={`flex flex-col ${
                      plan.popular && !isCurrentPlan
                        ? "border-2 border-[hsl(var(--purple))] bg-gradient-to-b from-[hsl(var(--marketing-card))] to-[hsl(var(--marketing-card))]/80 relative shadow-[0_0_30px_rgba(var(--purple-rgb),0.2)]"
                        : "border border-[hsl(var(--marketing-card-border))] bg-[hsl(var(--marketing-card))]/75"
                    } text-foreground p-4 sm:p-6 md:p-8 rounded-lg relative group transition-all duration-300 hover:border-foreground/30 ${
                      isCurrentPlan ? "ring-2 ring-foreground" : ""
                    } ${useBitcoin && isTeamPlan ? "opacity-50 cursor-not-allowed" : ""}`}
                  >
                    {plan.popular && !isCurrentPlan && (
                      <div className="absolute -top-4 left-1/2 transform -translate-x-1/2 bg-[hsl(var(--purple))] text-[hsl(var(--marketing-card))] px-4 py-1 rounded-full text-sm font-medium text-center min-w-[110px] whitespace-normal">
                        Most Popular
                      </div>
                    )}
                    {isCurrentPlan && (
                      <Badge className="absolute -top-3 left-1/2 -translate-x-1/2 bg-foreground text-background font-medium">
                        Current Plan
                      </Badge>
                    )}
                    {!isFreeplan && useBitcoin && !isTeamPlan && (
                      <Badge className="absolute -top-3 right-4 bg-gradient-to-r from-pink-500 to-orange-500 text-white">
                        10% OFF
                      </Badge>
                    )}
                    <div className="grid grid-rows-[auto_auto_1fr_auto_auto] h-full gap-4 sm:gap-6 md:gap-8">
                      <h3 className="text-xl sm:text-2xl font-medium flex items-center gap-2">
                        {plan.name}
                        {useBitcoin && !isFreeplan && !isTeamPlan ? " (Yearly)" : ""}
                        {isCurrentPlan && <Check className="w-5 h-5 text-green-500" />}
                      </h3>

                      <p className="text-base sm:text-lg font-light text-[hsl(var(--marketing-text-muted))] break-words">
                        {isTeamPlan && useBitcoin
                          ? "Team plan is not available with Bitcoin payment."
                          : plan.description}
                      </p>

                      {/* Features List */}
                      <div className="flex flex-col gap-2">
                        {plan.features.map((feature, index) => (
                          <div key={index} className="flex items-start gap-2 text-sm">
                            {feature.text !== "" &&
                              (feature.icon ||
                                (feature.included ? (
                                  <Check className="w-4 h-4 text-green-500 mt-0.5 flex-shrink-0" />
                                ) : null))}
                            <span
                              className={`${feature.included ? "text-foreground/80" : "text-foreground/50"}`}
                            >
                              {feature.text}
                            </span>
                          </div>
                        ))}
                      </div>

                      <div className="flex flex-col">
                        {!isFreeplan ? (
                          <>
                            <div className="flex flex-wrap items-center gap-2">
                              <span className="text-2xl sm:text-3xl font-bold">
                                ${displayPrice}
                              </span>
                              {useBitcoin && !isTeamPlan && (
                                <span className="text-lg sm:text-xl line-through text-foreground/50">
                                  ${monthlyOriginalPrice}
                                </span>
                              )}
                              <div className="flex flex-col text-[hsl(var(--marketing-text-muted))]">
                                <span className="text-base sm:text-lg font-light">
                                  {isTeamPlan ? "per user" : ""}
                                </span>
                                <span className="text-base sm:text-lg font-light -mt-1">
                                  per month
                                </span>
                              </div>
                            </div>
                            <div className="space-y-0.5 sm:space-y-1 mt-1">
                              {useBitcoin && !isTeamPlan && (
                                <>
                                  <p className="text-sm sm:text-base text-foreground/90 font-medium">
                                    {isTeamPlan
                                      ? `Billed yearly at $${yearlyDiscountedPrice} per user`
                                      : `Billed yearly at $${yearlyDiscountedPrice}`}
                                  </p>
                                  <p className="text-xs sm:text-sm text-foreground/50">
                                    Save 10% with annual billing
                                  </p>
                                </>
                              )}
                            </div>
                          </>
                        ) : (
                          <div className="flex flex-wrap items-center gap-2">
                            <span className="text-2xl sm:text-3xl font-bold">${monthlyPrice}</span>
                            <span className="text-base sm:text-lg font-light text-[hsl(var(--marketing-text-muted))]">
                              per month
                            </span>
                          </div>
                        )}
                      </div>

                      <button
                        onClick={() =>
                          handleButtonClick(
                            product || {
                              id: plan.name.toLowerCase(),
                              name: plan.name,
                              description: plan.description,
                              active: true,
                              default_price: {
                                id: "",
                                currency: "usd",
                                unit_amount: 0,
                                recurring: {
                                  interval: "month",
                                  interval_count: 1
                                }
                              }
                            }
                          )
                        }
                        disabled={
                          loadingProductId === (product?.id || plan.name.toLowerCase()) ||
                          (useBitcoin && isTeamPlan) ||
                          (isIOSPlatform && !isFreeplan && product?.is_available === false)
                        }
                        className={`w-full 
                      dark:bg-white/90 dark:text-black dark:hover:bg-[hsl(var(--purple))]/80 dark:hover:text-[hsl(var(--foreground))] dark:active:bg-white/80
                      bg-background text-foreground hover:bg-[hsl(var(--purple))] hover:text-[hsl(var(--foreground))] active:bg-background/80 
                      border border-[hsl(var(--purple))]/30 hover:border-[hsl(var(--purple))]
                      px-4 sm:px-8 py-3 sm:py-4 rounded-lg text-lg sm:text-xl font-light 
                      transition-all duration-300 shadow-[0_0_15px_rgba(var(--purple-rgb),0.2)] 
                      hover:shadow-[0_0_25px_rgba(var(--purple-rgb),0.3)] disabled:opacity-50 
                      disabled:cursor-not-allowed flex items-center justify-center gap-2 
                      group-hover:bg-[hsl(var(--purple))] group-hover:text-[hsl(var(--foreground))] dark:group-hover:text-[hsl(var(--foreground))] dark:group-hover:bg-[hsl(var(--purple))]/80`}
                      >
                        {useBitcoin && isTeamPlan
                          ? "Not Available"
                          : getButtonText(
                              product || {
                                id: plan.name.toLowerCase(),
                                name: plan.name,
                                description: plan.description || "",
                                active: true,
                                default_price: {
                                  id: "",
                                  currency: "usd",
                                  unit_amount: 0,
                                  recurring: {
                                    interval: "month",
                                    interval_count: 1
                                  }
                                }
                              }
                            )}
                      </button>
                    </div>
                  </div>
                );
              })}
            </div>
          );
        })()}

        <div className="w-full max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 flex justify-center mt-8">
          <div className="inline-flex items-center gap-4 px-6 py-2.5 rounded-full bg-[hsl(var(--marketing-card))]/50 backdrop-blur-sm border border-[hsl(var(--marketing-card-border))]">
            <div className="flex items-center gap-2 text-[hsl(var(--bitcoin))] text-base font-light">
              <Bitcoin className="w-4.5 h-4.5" />
              <span>Pay with Bitcoin</span>
            </div>
            <Switch
              id="bitcoin-toggle"
              checked={useBitcoin}
              onCheckedChange={setUseBitcoin}
              className="data-[state=checked]:bg-[hsl(var(--bitcoin))] data-[state=unchecked]:border-foreground/30 scale-100"
            />
          </div>
        </div>

        <PricingFAQ />
      </FullPageMain>
      <TeamSeatDialog
        open={showTeamSeatDialog}
        onOpenChange={setShowTeamSeatDialog}
        onConfirm={handleTeamSeatConfirm}
      />
      <VerificationModal />
    </>
  );
}

export const Route = createFileRoute("/pricing")({
  component: PricingPage,
  validateSearch: (search: Record<string, unknown>): PricingSearchParams => ({
    selected_plan: typeof search.selected_plan === "string" ? search.selected_plan : undefined,
    success:
      search.success === true || search.success === "true"
        ? true
        : search.success === false || search.success === "false"
          ? false
          : undefined,
    canceled:
      search.canceled === true || search.canceled === "true"
        ? true
        : search.canceled === false || search.canceled === "false"
          ? false
          : undefined
  })
});
