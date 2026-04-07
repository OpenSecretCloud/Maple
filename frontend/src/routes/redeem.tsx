import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { useState, useEffect } from "react";
import { useOpenSecret } from "@opensecret/react";
import { TopNav } from "@/components/TopNav";
import { FullPageMain } from "@/components/FullPageMain";
import { getBillingService } from "@/billing/billingService";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { MarketingHeader } from "@/components/MarketingHeader";
import { Loader2, Check, AlertTriangle, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { VerificationModal } from "@/components/VerificationModal";

type RedeemSearchParams = {
  code?: string;
};

const UUID_REGEX = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

function isValidUUID(code: string): boolean {
  return UUID_REGEX.test(code);
}

function formatPassCode(code: string): string {
  const cleaned = code.replace(/[^0-9a-f]/gi, "").toLowerCase();
  if (cleaned.length <= 8) return cleaned;
  if (cleaned.length <= 12) return `${cleaned.slice(0, 8)}-${cleaned.slice(8)}`;
  if (cleaned.length <= 16) {
    return `${cleaned.slice(0, 8)}-${cleaned.slice(8, 12)}-${cleaned.slice(12)}`;
  }
  if (cleaned.length <= 20) {
    return `${cleaned.slice(0, 8)}-${cleaned.slice(8, 12)}-${cleaned.slice(12, 16)}-${cleaned.slice(16)}`;
  }
  return `${cleaned.slice(0, 8)}-${cleaned.slice(8, 12)}-${cleaned.slice(12, 16)}-${cleaned.slice(16, 20)}-${cleaned.slice(20, 32)}`;
}

function RedeemPage() {
  const navigate = useNavigate();
  const os = useOpenSecret();
  const queryClient = useQueryClient();
  const { code: urlCode } = Route.useSearch();
  const [passCode, setPassCode] = useState(urlCode || "");
  const [checkTrigger, setCheckTrigger] = useState(0);
  const [redeemError, setRedeemError] = useState<string>("");
  const [redeemSuccess, setRedeemSuccess] = useState(false);

  const isLoggedIn = !!os.auth.user;
  const isGuestUser = os.auth.user?.user.login_method?.toLowerCase() === "guest";

  const { data: billingStatus } = useQuery({
    queryKey: ["billingStatus"],
    queryFn: async () => {
      const billingService = getBillingService();
      return await billingService.getBillingStatus();
    },
    enabled: isLoggedIn
  });

  const trimmedCode = passCode.trim().toLowerCase();
  const isValidCode = isValidUUID(trimmedCode);

  const {
    data: passCheckData,
    isLoading: isChecking,
    error: checkError
  } = useQuery({
    queryKey: ["passCheck", trimmedCode, checkTrigger],
    queryFn: async () => {
      const billingService = getBillingService();
      return await billingService.checkPassCode(trimmedCode);
    },
    enabled: isValidCode && trimmedCode.length === 36,
    retry: false
  });

  const redeemMutation = useMutation({
    mutationFn: async (code: string) => {
      const billingService = getBillingService();
      return await billingService.redeemPassCode({ pass_code: code });
    },
    onSuccess: () => {
      setRedeemSuccess(true);
      setRedeemError("");
      queryClient.invalidateQueries({ queryKey: ["billingStatus"] });
      setTimeout(() => {
        navigate({ to: "/" });
      }, 3000);
    },
    onError: (error: Error) => {
      setRedeemError(error.message);
      setRedeemSuccess(false);
    }
  });

  useEffect(() => {
    if (urlCode) {
      setPassCode(urlCode);
    }
  }, [urlCode]);

  const handlePassCodeChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const value = e.target.value;
    const formatted = formatPassCode(value);
    setPassCode(formatted);
    setRedeemError("");
    setCheckTrigger((prev) => prev + 1);
  };

  const handlePaste = async () => {
    try {
      const text = await navigator.clipboard.readText();
      const formatted = formatPassCode(text);
      setPassCode(formatted);
      setRedeemError("");
      setCheckTrigger((prev) => prev + 1);
    } catch (err) {
      console.error("Failed to read clipboard:", err);
    }
  };

  const handleRedeem = () => {
    if (!isLoggedIn) {
      navigate({
        to: "/signup",
        search: {
          next: "/redeem",
          code: trimmedCode
        }
      });
      return;
    }

    if (!import.meta.env.DEV && !isGuestUser && !os.auth.user?.user.email_verified) {
      console.log("Email verification required before redemption");
      return;
    }

    if (isValidCode && passCheckData?.valid && passCheckData?.status === "active") {
      redeemMutation.mutate(trimmedCode);
    }
  };

  const getStatusMessage = (): {
    type: "success" | "error" | "warning" | "info";
    message: string;
  } | null => {
    if (redeemSuccess) {
      return {
        type: "success",
        message: "Pass code redeemed successfully! Redirecting to dashboard..."
      };
    }

    if (redeemError) {
      return {
        type: "error",
        message: redeemError
      };
    }

    if (!isValidCode && trimmedCode.length > 0) {
      return {
        type: "error",
        message: "Invalid pass code format. Please enter a valid UUID."
      };
    }

    if (checkError) {
      return {
        type: "error",
        message: "Failed to check pass code. Please try again."
      };
    }

    if (passCheckData && !passCheckData.valid) {
      return {
        type: "error",
        message: passCheckData.message || "Invalid pass code"
      };
    }

    if (passCheckData?.valid) {
      const status = passCheckData.status;

      if (status === "pending") {
        return {
          type: "warning",
          message: "This pass code is pending payment confirmation and cannot be redeemed yet."
        };
      }

      if (status === "redeemed") {
        return {
          type: "error",
          message: "This pass code has already been redeemed and cannot be used again."
        };
      }

      if (status === "expired") {
        const expiryDate = passCheckData.expires_at
          ? new Date(passCheckData.expires_at).toLocaleDateString()
          : "an unknown date";
        return {
          type: "error",
          message: `This pass code expired on ${expiryDate}. Contact support if you believe this is an error.`
        };
      }

      if (status === "revoked") {
        return {
          type: "error",
          message: "This pass code has been revoked and cannot be used."
        };
      }

      if (status === "active") {
        return {
          type: "success",
          message: `Valid pass code for ${passCheckData.plan_name} (${passCheckData.duration_months} months)`
        };
      }
    }

    return null;
  };

  const statusMessage = getStatusMessage();
  const canRedeem =
    isValidCode &&
    passCheckData?.valid &&
    passCheckData?.status === "active" &&
    !redeemMutation.isPending &&
    !redeemSuccess;

  const hasActiveSubscription =
    billingStatus?.is_subscribed &&
    billingStatus?.payment_provider !== null &&
    billingStatus?.subscription_status === "active";

  const currentPlanName = billingStatus?.product_name?.toLowerCase();
  const isOnFreePlan = currentPlanName?.includes("free") || !billingStatus?.is_subscribed;
  const isOnNonFreePlan = hasActiveSubscription && !isOnFreePlan;

  return (
    <>
      <TopNav />
      <FullPageMain>
        <MarketingHeader
          title={
            <h2 className="text-6xl font-light mb-0">
              Redeem <span className="text-[hsl(var(--maple-primary))]">Subscription Pass</span>
            </h2>
          }
          subtitle={
            <div className="space-y-2">
              <p>Enter your subscription pass code to activate your plan.</p>
              <p>Pass codes are one-time use and valid for 12 months after creation.</p>
            </div>
          }
        />

        <div className="pt-8 w-full max-w-2xl mx-auto px-4 sm:px-6 lg:px-8">
          {isLoggedIn && isOnNonFreePlan && !redeemSuccess && (
            <div className="mb-6 flex items-start gap-3 rounded-lg border border-maple-warning/30 bg-maple-warning/10 p-6 text-maple-warning dark:border-maple-warning/40 dark:bg-maple-warning/15 dark:text-maple-warning">
              <div className="rounded-full bg-maple-warning/20 p-1 dark:bg-maple-warning/25">
                <AlertTriangle className="h-6 w-6 text-maple-warning" />
              </div>
              <div className="flex-1">
                <p className="font-semibold text-lg">
                  Active {billingStatus?.product_name} Subscription Detected
                </p>
                <p className="text-sm mt-2">
                  You're currently subscribed to the {billingStatus?.product_name} plan. Pass codes
                  can only be redeemed on free accounts.
                </p>
                <p className="text-sm mt-2">
                  To redeem this pass code, you'll need to cancel your current subscription and wait
                  for it to expire, or let it naturally expire at the end of your billing period.
                </p>
                <p className="text-sm mt-2">
                  You can create a new free account to redeem this code.
                </p>
                <Button
                  onClick={() => navigate({ to: "/pricing" })}
                  variant="outline"
                  size="sm"
                  className="mt-4"
                >
                  Manage Subscription
                </Button>
              </div>
            </div>
          )}
          <div className="flex flex-col border-[hsl(var(--marketing-card-border))] bg-[hsl(var(--marketing-card))]/75 text-foreground p-8 border rounded-lg">
            <div className="space-y-6">
              <div className="space-y-2">
                <label htmlFor="pass-code" className="text-lg font-medium">
                  Pass Code
                </label>
                <div className="flex gap-2">
                  <Input
                    id="pass-code"
                    type="text"
                    placeholder="xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx"
                    value={passCode}
                    onChange={handlePassCodeChange}
                    className="font-mono text-sm"
                    maxLength={36}
                  />
                  <Button
                    onClick={handlePaste}
                    variant="outline"
                    className="whitespace-nowrap"
                    type="button"
                  >
                    Paste
                  </Button>
                </div>
                <p className="text-sm text-[hsl(var(--marketing-text-muted))]">
                  Enter the subscription pass code you received
                </p>
              </div>

              {isChecking && (
                <div className="flex items-center gap-2 text-[hsl(var(--marketing-text-muted))]">
                  <Loader2 className="w-4 h-4 animate-spin" />
                  <span>Checking pass code...</span>
                </div>
              )}

              {statusMessage && (
                <div
                  className={`flex items-start gap-3 rounded-lg border p-4 ${
                    statusMessage.type === "success"
                      ? "border-maple-success/30 bg-maple-success/10 text-maple-success dark:border-maple-success/40 dark:bg-maple-success/15"
                      : statusMessage.type === "error"
                        ? "border-maple-error/30 bg-maple-error/10 text-maple-error dark:border-maple-error/40 dark:bg-maple-error/15"
                        : statusMessage.type === "warning"
                          ? "border-maple-warning/30 bg-maple-warning/10 text-maple-warning dark:border-maple-warning/40 dark:bg-maple-warning/15"
                          : "border-maple-info/30 bg-maple-info/10 text-maple-info dark:border-maple-info/40 dark:bg-maple-info/15"
                  }`}
                >
                  <div
                    className={`rounded-full p-1 ${
                      statusMessage.type === "success"
                        ? "bg-maple-success/20 dark:bg-maple-success/25"
                        : statusMessage.type === "error"
                          ? "bg-maple-error/20 dark:bg-maple-error/25"
                          : statusMessage.type === "warning"
                            ? "bg-maple-warning/20 dark:bg-maple-warning/25"
                            : "bg-maple-info/20 dark:bg-maple-info/25"
                    }`}
                  >
                    {statusMessage.type === "success" ? (
                      <Check className="h-5 w-5 text-maple-success" />
                    ) : statusMessage.type === "error" ? (
                      <X className="h-5 w-5 text-maple-error" />
                    ) : (
                      <AlertTriangle
                        className={`h-5 w-5 ${
                          statusMessage.type === "warning"
                            ? "text-maple-warning"
                            : "text-maple-info"
                        }`}
                      />
                    )}
                  </div>
                  <div className="flex-1">
                    <p>{statusMessage.message}</p>
                  </div>
                </div>
              )}

              {passCheckData?.valid && passCheckData?.status === "active" && (
                <div className="space-y-4 p-6 bg-gradient-to-b from-[hsl(var(--maple-primary))]/5 to-transparent rounded-lg border border-[hsl(var(--maple-primary))]/20">
                  <h3 className="text-xl font-medium">Plan Details</h3>
                  <div className="space-y-2 text-[hsl(var(--marketing-text-muted))]">
                    <div className="flex justify-between">
                      <span>Plan:</span>
                      <span className="font-medium text-foreground">{passCheckData.plan_name}</span>
                    </div>
                    <div className="flex justify-between">
                      <span>Duration:</span>
                      <span className="font-medium text-foreground">
                        {passCheckData.duration_months} months
                      </span>
                    </div>
                    {passCheckData.expires_at && (
                      <div className="flex justify-between">
                        <span>Code Expires:</span>
                        <span className="font-medium text-foreground">
                          {new Date(passCheckData.expires_at).toLocaleDateString()}
                        </span>
                      </div>
                    )}
                  </div>
                </div>
              )}

              <Button
                onClick={handleRedeem}
                disabled={!canRedeem || isOnNonFreePlan || redeemMutation.isPending}
                className="w-full 
                  dark:bg-[hsl(var(--marketing-cta-invert-bg)/0.9)] dark:text-[hsl(var(--marketing-cta-invert-fg))] dark:hover:bg-[hsl(var(--maple-primary))]/80 dark:hover:text-[hsl(var(--foreground))] dark:active:bg-[hsl(var(--marketing-cta-invert-bg)/0.8)]
                  bg-background text-foreground hover:bg-[hsl(var(--maple-primary))] hover:text-[hsl(var(--foreground))] active:bg-background/80 
                  border border-[hsl(var(--maple-primary))]/30 hover:border-[hsl(var(--maple-primary))]
                  px-8 py-4 rounded-lg text-xl font-light 
                  transition-all duration-300 shadow-[0_0_15px_rgba(var(--maple-primary-rgb),0.2)] 
                  hover:shadow-[0_0_25px_rgba(var(--maple-primary-rgb),0.3)] disabled:opacity-50 
                  disabled:cursor-not-allowed flex items-center justify-center gap-2"
              >
                {redeemMutation.isPending ? (
                  <>
                    <Loader2 className="w-5 h-5 animate-spin" />
                    Redeeming...
                  </>
                ) : isLoggedIn ? (
                  "Redeem Pass Code"
                ) : (
                  "Create Account to Redeem"
                )}
              </Button>

              {!isLoggedIn && (
                <p className="text-center text-sm text-[hsl(var(--marketing-text-muted))]">
                  Already have a free account,{" "}
                  <button
                    onClick={() =>
                      navigate({
                        to: "/login",
                        search: {
                          next: "/redeem",
                          code: trimmedCode
                        }
                      })
                    }
                    className="text-[hsl(var(--maple-primary))] hover:underline"
                  >
                    log in
                  </button>
                </p>
              )}
            </div>
          </div>

          <div className="flex flex-col gap-4 border-[hsl(var(--marketing-card-border))] bg-[hsl(var(--marketing-card))]/75 text-foreground p-6 border rounded-lg mt-8">
            <h3 className="text-xl font-medium">About Subscription Passes</h3>
            <div className="space-y-3 text-[hsl(var(--marketing-text-muted))]">
              <p>
                Subscription passes are prepaid codes that grant you access to Maple's premium
                features for a specified duration.
              </p>
              <ul className="list-disc list-inside space-y-2 ml-4">
                <li>Each pass code can only be used once</li>
                <li>Passes must be redeemed within 12 months of creation</li>
                <li>Only free accounts can redeem passes</li>
                <li>Pass subscriptions expire automatically at the end of the duration</li>
                <li>No automatic renewal or payment required</li>
              </ul>
            </div>
          </div>
        </div>
      </FullPageMain>
      <VerificationModal />
    </>
  );
}

export const Route = createFileRoute("/redeem")({
  component: RedeemPage,
  validateSearch: (search: Record<string, unknown>): RedeemSearchParams => ({
    code: typeof search.code === "string" ? search.code : undefined
  })
});
