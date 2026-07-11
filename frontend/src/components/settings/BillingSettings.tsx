import { useState } from "react";
import { Link } from "@tanstack/react-router";
import { CreditCard, KeyRound, Loader2, Sparkles } from "lucide-react";
import { openBillingPortal } from "@/billing/billingPortal";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { useLocalState } from "@/state/useLocalState";
import { SettingsPage, SettingsSection } from "./SettingsPage";

export function BillingSettings() {
  const { billingStatus } = useLocalState();
  const [isPortalLoading, setIsPortalLoading] = useState(false);
  const [portalError, setPortalError] = useState<string | null>(null);

  const productName = billingStatus?.product_name ?? "";
  const normalizedProductName = productName.toLowerCase();
  const hasStripeAccount = !!billingStatus?.stripe_customer_id;
  const isPaidPlan = ["starter", "pro", "max", "team"].some((plan) =>
    normalizedProductName.includes(plan)
  );
  const showManage = isPaidPlan && hasStripeAccount;
  const showUpgrade =
    !normalizedProductName.includes("max") && !normalizedProductName.includes("team");

  const handleManageSubscription = async () => {
    if (!showManage) return;
    setIsPortalLoading(true);
    setPortalError(null);
    try {
      await openBillingPortal();
    } catch (error) {
      console.error("Error opening billing portal:", error);
      setPortalError(
        "Unable to open subscription management. Please try again or contact support@trymaple.ai."
      );
    } finally {
      setIsPortalLoading(false);
    }
  };

  const periodLabel =
    billingStatus?.payment_provider === "subscription_pass" ||
    billingStatus?.payment_provider === "zaprite"
      ? "Expires"
      : "Renews";

  return (
    <SettingsPage title="Billing" description="Review your plan and manage subscription access.">
      <SettingsSection title="Current plan">
        <div className="flex flex-col gap-5 sm:flex-row sm:items-center sm:justify-between">
          <div>
            <div className="flex items-center gap-2">
              <CreditCard className="h-5 w-5 text-muted-foreground" />
              <p className="text-lg font-semibold">
                {billingStatus ? `${billingStatus.product_name} Plan` : "Loading plan..."}
              </p>
            </div>
            {billingStatus?.current_period_end && (
              <p className="mt-1.5 text-sm text-muted-foreground">
                {periodLabel} on{" "}
                {new Date(Number(billingStatus.current_period_end) * 1000).toLocaleDateString(
                  undefined,
                  { year: "numeric", month: "long", day: "numeric" }
                )}
              </p>
            )}
          </div>
          <div className="flex flex-col gap-2 sm:flex-row">
            {showUpgrade && (
              <Button asChild variant="primary">
                <Link to="/pricing">
                  <Sparkles className="mr-2 h-4 w-4" />
                  Upgrade plan
                </Link>
              </Button>
            )}
            {showManage && (
              <Button
                type="button"
                variant="outline"
                onClick={handleManageSubscription}
                disabled={isPortalLoading}
              >
                {isPortalLoading && <Loader2 className="mr-2 h-4 w-4 animate-spin" />}
                {isPortalLoading ? "Opening..." : "Manage subscription"}
              </Button>
            )}
          </div>
        </div>
        {portalError && (
          <Alert variant="destructive" className="mt-4">
            <AlertDescription>{portalError}</AlertDescription>
          </Alert>
        )}
      </SettingsSection>

      <SettingsSection
        title="API credits"
        description="View your extra credit balance or purchase credits for API and extended plan usage."
      >
        <Button asChild variant="outline">
          <Link to="/settings/api" replace>
            <KeyRound className="mr-2 h-4 w-4" />
            Manage API and credits
          </Link>
        </Button>
      </SettingsSection>
    </SettingsPage>
  );
}
