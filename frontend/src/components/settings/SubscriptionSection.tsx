import { useState } from "react";
import { Link } from "@tanstack/react-router";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { ArrowUpCircle, Loader2, ExternalLink } from "lucide-react";
import { useLocalState } from "@/state/useLocalState";
import { getBillingService } from "@/billing/billingService";
import { isMobile } from "@/utils/platform";
import { formatResetDate } from "@/utils/dateFormat";

export function SubscriptionSection() {
  const { billingStatus } = useLocalState();
  const [isPortalLoading, setIsPortalLoading] = useState(false);

  const productName = billingStatus?.product_name || "";
  const hasStripeAccount = billingStatus?.stripe_customer_id !== null;
  const isPro = productName.toLowerCase().includes("pro");
  const isMax = productName.toLowerCase().includes("max");
  const isStarter = productName.toLowerCase().includes("starter");
  const isTeamPlan = productName.toLowerCase().includes("team");
  const showUpgrade = !isMax && !isTeamPlan;
  const showManage = (isPro || isMax || isStarter || isTeamPlan) && hasStripeAccount;

  const handleManageSubscription = async () => {
    if (!hasStripeAccount) return;

    try {
      setIsPortalLoading(true);
      const billingService = getBillingService();
      const url = await billingService.getPortalUrl();

      if (isMobile()) {
        const { invoke } = await import("@tauri-apps/api/core");
        await invoke("plugin:opener|open_url", { url })
          .then(() => console.log("[Billing] Opened portal URL"))
          .catch((err: Error) => {
            console.error("[Billing] Failed to open browser:", err);
            alert("Failed to open browser. Please try again.");
          });
        await new Promise((resolve) => setTimeout(resolve, 300));
        return;
      }

      window.open(url, "_blank");
    } catch (error) {
      console.error("Error fetching portal URL:", error);
    } finally {
      setIsPortalLoading(false);
    }
  };

  // Credit usage calculation
  const hasCredits = billingStatus?.total_tokens != null && billingStatus?.used_tokens != null;
  const percentUsed = hasCredits
    ? Math.min(100, Math.max(0, (billingStatus.used_tokens! / billingStatus.total_tokens!) * 100))
    : 0;
  const roundedPercent = Math.round(percentUsed);
  const hasApiCredits =
    billingStatus?.api_credit_balance !== undefined && billingStatus.api_credit_balance > 0;

  const getBarColor = () => {
    if (percentUsed >= 90) return "rgb(239, 68, 68)";
    if (percentUsed >= 75) return "rgb(245, 158, 11)";
    return "rgb(16, 185, 129)";
  };

  const formatCredits = (credits: number) => {
    return new Intl.NumberFormat("en-US").format(credits);
  };

  return (
    <div className="space-y-8">
      <div>
        <h2 className="text-2xl font-semibold tracking-tight">Subscription</h2>
        <p className="text-muted-foreground mt-1">Manage your plan and billing details.</p>
      </div>

      {/* Current Plan */}
      <div className="space-y-4">
        <h3 className="text-lg font-medium">Current Plan</h3>
        <div className="flex items-center gap-3">
          <Badge
            variant="secondary"
            className="bg-[hsl(var(--primary))] text-[hsl(var(--primary-foreground))] dark:bg-white dark:text-[hsl(var(--background))] uppercase px-3 py-1 text-sm"
          >
            {billingStatus ? `${billingStatus.product_name} Plan` : "Loading..."}
          </Badge>
        </div>
        {billingStatus?.current_period_end && (
          <p className="text-sm text-muted-foreground">
            {billingStatus.payment_provider === "subscription_pass" ||
            billingStatus.payment_provider === "zaprite"
              ? "Expires on "
              : "Renews on "}
            {new Date(Number(billingStatus.current_period_end) * 1000).toLocaleDateString(
              undefined,
              {
                year: "numeric",
                month: "long",
                day: "numeric"
              }
            )}
          </p>
        )}
      </div>

      {/* Credit Usage */}
      {hasCredits && (
        <div className="space-y-4">
          <h3 className="text-lg font-medium">Credit Usage</h3>
          <div className="bg-muted/50 rounded-lg p-4 space-y-3">
            <div className="flex justify-between text-sm">
              <span>Plan Credits</span>
              <span className="font-medium">{roundedPercent}% used</span>
            </div>
            <div className="h-3 w-full overflow-hidden rounded-full bg-muted">
              <div
                className="h-full transition-all"
                style={{
                  width: `${percentUsed}%`,
                  backgroundColor: getBarColor()
                }}
              />
            </div>
            <div className="flex justify-between text-xs text-muted-foreground">
              {hasApiCredits && (
                <span>+ {formatCredits(billingStatus.api_credit_balance ?? 0)} extra credits</span>
              )}
              <span className={hasApiCredits ? "" : "ml-auto"}>
                {formatResetDate(billingStatus?.usage_reset_date)}
              </span>
            </div>
          </div>
        </div>
      )}

      {/* Actions */}
      <div className="space-y-3">
        {showUpgrade && (
          <Link to="/pricing">
            <Button variant="default" className="gap-2 w-full sm:w-auto">
              <ArrowUpCircle className="h-4 w-4" />
              Upgrade Your Plan
            </Button>
          </Link>
        )}
        {showManage && (
          <Button
            variant="outline"
            className="gap-2 w-full sm:w-auto"
            onClick={handleManageSubscription}
            disabled={isPortalLoading}
          >
            {isPortalLoading ? (
              <Loader2 className="h-4 w-4 animate-spin" />
            ) : (
              <ExternalLink className="h-4 w-4" />
            )}
            {isPortalLoading ? "Loading..." : "Manage Subscription"}
          </Button>
        )}
      </div>
    </div>
  );
}
