import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { useState, useEffect } from "react";
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

function PricingSkeletonCard() {
  return (
    <div className="flex flex-col border-white/10 bg-black/75 text-white p-4 sm:p-6 md:p-8 border rounded-lg relative overflow-hidden">
      <div className="absolute inset-0 -translate-x-full animate-[shimmer_2s_infinite] bg-gradient-to-r from-transparent via-white/5 to-transparent"></div>
      <div className="grid grid-rows-[auto_1fr_auto_auto] h-full gap-4 sm:gap-6 md:gap-8">
        <div className="h-6 sm:h-8 bg-white/10 rounded-md w-1/2"></div>
        <div className="space-y-2 sm:space-y-3">
          <div className="h-4 bg-white/10 rounded-md w-3/4"></div>
          <div className="h-4 bg-white/10 rounded-md w-5/6"></div>
          <div className="h-4 bg-white/10 rounded-md w-2/3"></div>
        </div>
        <div className="h-6 sm:h-8 bg-white/10 rounded-md w-1/3"></div>
        <div className="h-12 sm:h-14 bg-white/10 rounded-lg w-full"></div>
      </div>
    </div>
  );
}

function PricingFAQ() {
  return (
    <div className="flex flex-col gap-8 border-white/10 bg-black/75 text-white p-6 sm:p-8 border rounded-lg mt-8 max-w-7xl mx-auto w-full px-4 sm:px-6 lg:px-8">
      <h3 className="text-2xl font-medium">FAQ</h3>

      <div className="flex flex-col gap-4">
        <details className="group">
          <summary className="cursor-pointer text-lg font-medium hover:text-white/80">
            What is the difference between the plans?
          </summary>
          <div className="mt-4 text-white/70 space-y-2">
            <p>The plans are sized to grow with your needs.</p>
            <ul className="list-disc list-inside space-y-1 ml-4">
              <li>
                Free: 10 chats per week, resets Sunday 00:00 UTC. Max length on individual chats.
              </li>
              <li>Starter: Enough chats per month for a casual user</li>
              <li>Pro: Great for heavier workloads with a high monthly cap</li>
              <li>Enterprise: Message us at team@opensecret.cloud</li>
            </ul>
          </div>
        </details>

        <details className="group">
          <summary className="cursor-pointer text-lg font-medium hover:text-white/80">
            How private is Maple?
          </summary>
          <p className="mt-4 text-white/70">
            Encrypted end to end. Maple uses confidential computing to secure the code that access
            user data and LLM data. Your account has its own private key that encrypts your chats
            and the responses from the AI model. Every user has their own personal data vault that
            can't be read by anyone else, not even us.
          </p>
        </details>

        <details className="group">
          <summary className="cursor-pointer text-lg font-medium hover:text-white/80">
            How do you synchronize my chat history across devices?
          </summary>
          <p className="mt-4 text-white/70">
            We use a secure synchronization protocol that ensures your encrypted chat history is
            synced across all your devices. This means that you can start a conversation on one
            device and pick it up where you left off on another device, without compromising your
            security or privacy.
          </p>
        </details>

        <details className="group">
          <summary className="cursor-pointer text-lg font-medium hover:text-white/80">
            Is this safe to use with my company's confidential information?
          </summary>
          <p className="mt-4 text-white/70">
            The service is encrypted end-to-end, so your confidential information is private between
            you and the AI. Consult your company's security policy.
          </p>
        </details>

        <details className="group">
          <summary className="cursor-pointer text-lg font-medium hover:text-white/80">
            Can companies use my data to train their AI models?
          </summary>
          <p className="mt-4 text-white/70">
            No. When you chat with AI in Maple, nobody knows what is being said back and forth. Thus
            data is not able to be used for training new AI models by any company.
          </p>
        </details>

        <details className="group">
          <summary className="cursor-pointer text-lg font-medium hover:text-white/80">
            How did you build this?
          </summary>
          <p className="mt-4 text-white/70">
            Maple is made using{" "}
            <a href="https://opensecret.cloud" className="text-white hover:text-white/80 underline">
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
  const navigate = useNavigate();
  const os = useOpenSecret();
  const { setBillingStatus } = useLocalState();
  const isLoggedIn = !!os.auth.user;

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

  // Auto-enable Bitcoin toggle for Zaprite users
  useEffect(() => {
    if (freshBillingStatus?.payment_provider === "zaprite") {
      setUseBitcoin(true);
    }
  }, [freshBillingStatus?.payment_provider]);

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

  // Check team plan availability if user is logged in
  const { data: isTeamPlanAvailable } = useQuery({
    queryKey: ["teamPlanAvailable"],
    queryFn: async () => {
      const billingService = getBillingService();
      try {
        return await billingService.getTeamPlanAvailable();
      } catch (error) {
        console.error("Error checking team plan availability:", error);
        return false;
      }
    },
    enabled: isLoggedIn
  });

  const {
    data: products,
    error: productsError,
    isLoading: productsLoading
  } = useQuery({
    queryKey: ["products"],
    queryFn: async () => {
      try {
        const billingService = getBillingService();
        return await billingService.getProducts();
      } catch (error) {
        console.error("Error fetching products:", error);
        throw error;
      }
    },
    retry: 1 // Only retry once for faster error feedback
  });

  const getButtonText = (product: any) => {
    if (loadingProductId === product.id) {
      return (
        <>
          <Loader2 className="w-5 h-5 animate-spin" />
          Processing...
        </>
      );
    }

    if (!isLoggedIn) {
      return "Start Chatting";
    }

    const currentPlanName = freshBillingStatus?.product_name?.toLowerCase();
    const targetPlanName = product.name.toLowerCase();
    const isCurrentPlan = currentPlanName === targetPlanName;
    const isTeamPlan = targetPlanName.includes("team");

    // If user is on Zaprite plan, show Contact Us
    if (freshBillingStatus?.payment_provider === "zaprite") {
      return "Contact Us";
    }

    // For team plan, show Contact Us if not available
    if (isTeamPlan && !isTeamPlanAvailable) {
      return "Contact Us";
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

  const handleButtonClick = (product: any) => {
    if (!isLoggedIn) {
      navigate({ to: "/signup" });
      return;
    }

    const targetPlanName = product.name.toLowerCase();
    const isTeamPlan = targetPlanName.includes("team");

    // For team plan, redirect to email if not available
    if (isTeamPlan && !isTeamPlanAvailable) {
      window.location.href = "mailto:support@opensecret.cloud";
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
      newHandleSubscribe(product.id);
      return;
    }

    // For all other cases (upgrades/downgrades between paid plans, or downgrades to free),
    // use portal URL if it exists
    if (portalUrl) {
      window.open(portalUrl, "_blank");
      return;
    }

    // If no portal URL exists and it's not a free plan user upgrading,
    // create checkout session
    newHandleSubscribe(product.id);
  };

  const newHandleSubscribe = async (productId: string) => {
    if (!isLoggedIn) {
      navigate({ to: "/signup" });
      return;
    }

    setLoadingProductId(productId);
    try {
      const billingService = getBillingService();
      const email = os.auth.user?.user.email;
      if (!email) {
        throw new Error("User email not found");
      }

      if (useBitcoin) {
        await billingService.createZapriteCheckoutSession(
          email,
          productId,
          `${window.location.origin}/pricing?success=true`
        );
      } else {
        await billingService.createCheckoutSession(
          email,
          productId,
          `${window.location.origin}/pricing?success=true`,
          `${window.location.origin}/pricing?canceled=true`
        );
      }
    } catch (err) {
      console.error("Subscribe error:", err);
      setCheckoutError(err instanceof Error ? err.message : "Failed to start checkout");
    } finally {
      setLoadingProductId(null);
    }
  };

  // Show loading state if we're fetching initial data
  if (productsLoading || (isLoggedIn && isBillingStatusLoading)) {
    return (
      <>
        <TopNav />
        <FullPageMain>
          <MarketingHeader
            title="Simple, transparent pricing"
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

          <div className="pt-8 w-full grid grid-cols-1 md:grid-cols-4 gap-4 md:gap-4 lg:gap-6 px-4 sm:px-6 lg:px-8">
            <div className="flex flex-col border-white/10 bg-black/75 text-white p-8 border rounded-lg col-span-full">
              <div className="flex flex-col items-center text-center gap-4">
                <div className="rounded-full bg-red-500/10 p-3">
                  <AlertTriangle className="w-6 h-6 text-red-500" />
                </div>
                <div className="space-y-2">
                  <h3 className="text-xl font-medium">Unable to Load Pricing</h3>
                  <p className="text-white/70">{errorMessage}</p>
                </div>
                <Button
                  onClick={() => window.location.reload()}
                  className="mt-4 bg-white/90 backdrop-blur-sm text-black hover:bg-white/70 active:bg-white/80 px-8 py-4 rounded-lg text-xl font-light transition-all duration-200 shadow-[0_0_25px_rgba(255,255,255,0.25)] hover:shadow-[0_0_35px_rgba(255,255,255,0.35)]"
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
          title="Simple, transparent pricing"
          subtitle={
            <div className="space-y-2">
              <p>Start with our free tier and upgrade as you grow.</p>
              <p>All plans include end-to-end encrypted AI chat.</p>
            </div>
          }
        />

        <div className="w-full max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 flex justify-center">
          <div className="inline-flex items-center gap-4 px-6 py-2.5 rounded-full bg-black/50 backdrop-blur-sm border border-white/10">
            <div className="flex items-center gap-2 text-[#F7931A] text-base font-light">
              <Bitcoin className="w-4.5 h-4.5" />
              <span>Pay with Bitcoin</span>
            </div>
            <Switch
              id="bitcoin-toggle"
              checked={useBitcoin}
              onCheckedChange={setUseBitcoin}
              className="data-[state=checked]:bg-[#F7931A] scale-100"
            />
          </div>
        </div>

        <div className="pt-8 w-full max-w-7xl mx-auto grid grid-cols-1 md:grid-cols-4 gap-4 md:gap-4 lg:gap-6 px-4 sm:px-6 lg:px-8">
          {products &&
            [...products]
              .sort((a, b) => a.default_price.unit_amount - b.default_price.unit_amount)
              .filter((product) => product.active)
              .map((product) => {
                const isCurrentPlan =
                  isLoggedIn &&
                  freshBillingStatus?.product_name?.toLowerCase() === product.name.toLowerCase();
                const isTeamPlan = product.name.toLowerCase().includes("team");

                // Calculate prices
                const monthlyOriginalPrice = (product.default_price.unit_amount / 100).toFixed(2);
                const monthlyDiscountedPrice = (
                  Math.floor(product.default_price.unit_amount / 2) / 100
                ).toFixed(2);

                // Calculate yearly prices for Bitcoin (10% off)
                const yearlyDiscountedPrice = (
                  Math.floor(product.default_price.unit_amount * 12 * 0.9) / 100
                ).toFixed(2);

                // Calculate monthly equivalent of yearly Bitcoin price
                const monthlyEquivalentPrice = (Number(yearlyDiscountedPrice) / 12).toFixed(2);

                const displayOriginalPrice = monthlyOriginalPrice;
                const displayDiscountedPrice = useBitcoin
                  ? monthlyEquivalentPrice
                  : monthlyDiscountedPrice;

                return (
                  <div
                    key={product.id}
                    className={`flex flex-col border-white/10 bg-black/75 text-white p-4 sm:p-6 md:p-8 border rounded-lg relative group transition-all duration-300 hover:border-white/30 ${
                      isCurrentPlan ? "ring-2 ring-white" : ""
                    } ${useBitcoin && product.name === "Team" ? "opacity-50 cursor-not-allowed" : ""}`}
                  >
                    {isCurrentPlan && (
                      <Badge className="absolute -top-3 left-1/2 -translate-x-1/2 bg-white text-black font-medium">
                        Current Plan
                      </Badge>
                    )}
                    {product.name !== "Free" && (
                      <Badge className="absolute -top-3 right-4 bg-gradient-to-r from-pink-500 to-orange-500 text-white">
                        {useBitcoin && product.name !== "Team" ? "10% OFF" : "50% OFF"}
                      </Badge>
                    )}
                    <div className="grid grid-rows-[auto_1fr_auto_auto] h-full gap-4 sm:gap-6 md:gap-8">
                      <h3 className="text-xl sm:text-2xl font-medium flex items-center gap-2">
                        {product.name}
                        {useBitcoin && product.name !== "Free" && product.name !== "Team"
                          ? " (Yearly)"
                          : ""}
                        {isCurrentPlan && <Check className="w-5 h-5 text-green-500" />}
                      </h3>

                      <p className="text-base sm:text-lg font-light text-white/70 break-words">
                        {product.name === "Team" && useBitcoin
                          ? "Team plan is not available with Bitcoin payment."
                          : product.description}
                      </p>

                      <div className="flex flex-col">
                        {product.name !== "Free" ? (
                          <>
                            <div className="flex flex-wrap items-center gap-2">
                              <span className="text-2xl sm:text-3xl font-bold">
                                ${displayDiscountedPrice}
                              </span>
                              <span className="text-lg sm:text-xl line-through text-white/50">
                                ${displayOriginalPrice}
                              </span>
                              <div className="flex flex-col text-white/70">
                                <span className="text-base sm:text-lg font-light">
                                  {product.name === "Team" ? "per user" : ""}
                                </span>
                                <span className="text-base sm:text-lg font-light -mt-1">
                                  per month
                                </span>
                              </div>
                            </div>
                            <div className="space-y-0.5 sm:space-y-1 mt-1">
                              {useBitcoin && product.name !== "Team" ? (
                                <>
                                  <p className="text-sm sm:text-base text-white/90 font-medium">
                                    {product.name === "Team"
                                      ? `Billed yearly at $${yearlyDiscountedPrice} per user`
                                      : `Billed yearly at $${yearlyDiscountedPrice}`}
                                  </p>
                                  <p className="text-xs sm:text-sm text-white/50">
                                    Save 10% with annual billing
                                  </p>
                                </>
                              ) : (
                                <>
                                  <p className="text-xs sm:text-sm text-white/50">
                                    First 3 months only
                                  </p>
                                  <p className="text-xs sm:text-sm text-white/50">
                                    Offer ends January 31st
                                  </p>
                                </>
                              )}
                            </div>
                          </>
                        ) : (
                          <div className="flex flex-wrap items-center gap-2">
                            <span className="text-2xl sm:text-3xl font-bold">
                              ${displayOriginalPrice}
                            </span>
                            <span className="text-base sm:text-lg font-light text-white/70">
                              per month
                            </span>
                          </div>
                        )}
                      </div>

                      <button
                        onClick={() => handleButtonClick(product)}
                        disabled={
                          loadingProductId === product.id || (useBitcoin && product.name === "Team")
                        }
                        className={`w-full bg-white/90 backdrop-blur-sm text-black hover:bg-white/70 active:bg-white/80 px-4 sm:px-8 py-3 sm:py-4 rounded-lg text-lg sm:text-xl font-light transition-all duration-200 shadow-[0_0_25px_rgba(255,255,255,0.25)] hover:shadow-[0_0_35px_rgba(255,255,255,0.35)] disabled:opacity-50 disabled:cursor-not-allowed flex items-center justify-center gap-2 group-hover:bg-white ${
                          isTeamPlan && !isTeamPlanAvailable
                            ? "!opacity-100 !cursor-pointer hover:!bg-white/70"
                            : ""
                        }`}
                      >
                        {useBitcoin && product.name === "Team"
                          ? "Not Available"
                          : getButtonText(product)}
                      </button>
                    </div>
                  </div>
                );
              })}
        </div>

        <PricingFAQ />
      </FullPageMain>
    </>
  );
}

export const Route = createFileRoute("/pricing")({
  component: PricingPage
});
