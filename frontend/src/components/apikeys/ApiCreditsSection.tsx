import { useState, useEffect } from "react";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Loader2, CreditCard, Bitcoin, Coins, CheckCircle } from "lucide-react";
import { useQuery } from "@tanstack/react-query";
import { getBillingService } from "@/billing/billingService";
import { useOpenSecret } from "@opensecret/react";

interface CreditPackage {
  credits: number;
  price: number;
  label: string;
}

const CREDIT_PACKAGES: CreditPackage[] = [
  { credits: 10000, price: 10, label: "10,000 credits" },
  { credits: 50000, price: 50, label: "50,000 credits" },
  { credits: 100000, price: 100, label: "100,000 credits" },
  { credits: 500000, price: 500, label: "500,000 credits" }
];

interface ApiCreditsSectionProps {
  showSuccessMessage?: boolean;
}

export function ApiCreditsSection({ showSuccessMessage = false }: ApiCreditsSectionProps) {
  const [selectedPackage, setSelectedPackage] = useState<CreditPackage>(CREDIT_PACKAGES[0]);
  const [isPurchasing, setIsPurchasing] = useState(false);
  const [paymentMethod, setPaymentMethod] = useState<"stripe" | "zaprite" | null>(null);
  const [showSuccess, setShowSuccess] = useState(showSuccessMessage);
  const { auth } = useOpenSecret();

  useEffect(() => {
    if (showSuccessMessage) {
      setShowSuccess(true);
      // Hide success message after 5 seconds
      const timer = setTimeout(() => setShowSuccess(false), 5000);
      return () => clearTimeout(timer);
    }
  }, [showSuccessMessage]);

  // Fetch credit balance
  const {
    data: creditBalance,
    isLoading: isLoadingBalance,
    error: balanceError
  } = useQuery({
    queryKey: ["apiCreditBalance"],
    queryFn: async () => {
      const billingService = getBillingService();
      return await billingService.getApiCreditBalance();
    },
    enabled: !!auth.user && !auth.loading
  });

  const formatCredits = (credits: number) => {
    return new Intl.NumberFormat("en-US").format(credits);
  };

  const handlePurchase = async (method: "stripe" | "zaprite") => {
    setIsPurchasing(true);
    setPaymentMethod(method);

    try {
      const billingService = getBillingService();

      // Determine success/cancel URLs based on platform
      let successUrl: string;
      let cancelUrl: string | undefined;

      try {
        // Check if we're in Tauri environment
        const isTauri = await import("@tauri-apps/api/core")
          .then((m) => m.isTauri())
          .catch(() => false);

        const isTauriIOS =
          isTauri &&
          (await import("@tauri-apps/plugin-os")
            .then((m) => m.type())
            .then((type) => type === "ios")
            .catch(() => false));

        // For iOS, use Universal Links that match the AASA configuration
        if (isTauriIOS) {
          successUrl = `https://trymaple.ai/payment-success-credits?source=${method}`;
          cancelUrl =
            method === "stripe" ? `https://trymaple.ai/payment-canceled?source=stripe` : undefined;
        } else {
          // For web or desktop, use regular URLs with query params
          const baseUrl = window.location.origin;
          successUrl = `${baseUrl}/?credits_success=true`;
          cancelUrl = method === "stripe" ? `${baseUrl}/` : undefined;
        }
      } catch (error) {
        console.error("Error determining platform:", error);
        // Fall back to regular URLs if platform detection fails
        const baseUrl = window.location.origin;
        successUrl = `${baseUrl}/?credits_success=true`;
        cancelUrl = method === "stripe" ? `${baseUrl}/` : undefined;
      }

      if (method === "stripe") {
        const response = await billingService.purchaseApiCredits({
          credits: selectedPackage.credits,
          success_url: successUrl,
          cancel_url: cancelUrl || successUrl
        });

        // Redirect to Stripe checkout
        window.location.href = response.checkout_url;
      } else {
        // For Zaprite, we need the user's email
        const email = auth.user?.user.email || "";
        const response = await billingService.purchaseApiCreditsZaprite({
          credits: selectedPackage.credits,
          email,
          success_url: successUrl
        });

        // Redirect to Zaprite checkout
        window.location.href = response.checkout_url;
      }
    } catch (error) {
      console.error("Failed to create checkout session:", error);
    } finally {
      setIsPurchasing(false);
      setPaymentMethod(null);
    }
  };

  if (isLoadingBalance) {
    return (
      <Card className="p-4">
        <div className="flex items-center justify-center py-4">
          <Loader2 className="h-6 w-6 animate-spin" />
        </div>
      </Card>
    );
  }

  if (balanceError) {
    return (
      <Card className="p-4">
        <p className="text-sm text-destructive">Failed to load credit balance</p>
      </Card>
    );
  }

  return (
    <div className="space-y-4">
      {/* Success Message */}
      {showSuccess && (
        <Alert className="border-green-500/50 bg-green-500/10">
          <CheckCircle className="h-4 w-4 text-green-500" />
          <AlertDescription className="text-sm">
            Payment successful! Your credits have been added to your account.
          </AlertDescription>
        </Alert>
      )}

      {/* Current Balance */}
      <Card className="p-4">
        <div className="flex items-center justify-between">
          <div>
            <p className="text-sm text-muted-foreground">API Credit Balance</p>
            <p className="text-2xl font-bold flex items-center gap-2">
              <Coins className="h-5 w-5" />
              {formatCredits(creditBalance?.balance || 0)}
            </p>
            <p className="text-xs text-muted-foreground mt-1">
              $1 per 1,000 credits • Use for API requests
            </p>
          </div>
        </div>
      </Card>

      {/* Purchase Credits */}
      <Card className="p-4">
        <h3 className="font-medium mb-3">Purchase Credits</h3>

        {/* Credit Package Selection */}
        <div className="grid grid-cols-2 gap-2 mb-4">
          {CREDIT_PACKAGES.map((pkg) => (
            <button
              key={pkg.credits}
              onClick={() => setSelectedPackage(pkg)}
              className={`p-3 rounded-lg border text-left transition-colors ${
                selectedPackage.credits === pkg.credits
                  ? "border-primary bg-primary/10"
                  : "border-border hover:border-muted-foreground"
              }`}
            >
              <p className="font-medium text-sm">{pkg.label}</p>
              <p className="text-xs text-muted-foreground">${pkg.price}</p>
            </button>
          ))}
        </div>

        {/* Payment Methods */}
        <div className="flex gap-2">
          <Button
            onClick={() => handlePurchase("stripe")}
            disabled={isPurchasing}
            className="flex-1"
            size="sm"
          >
            {isPurchasing && paymentMethod === "stripe" ? (
              <Loader2 className="mr-2 h-4 w-4 animate-spin" />
            ) : (
              <CreditCard className="mr-2 h-4 w-4" />
            )}
            Pay with Card
          </Button>

          <Button
            onClick={() => handlePurchase("zaprite")}
            disabled={isPurchasing}
            variant="outline"
            className="flex-1"
            size="sm"
          >
            {isPurchasing && paymentMethod === "zaprite" ? (
              <Loader2 className="mr-2 h-4 w-4 animate-spin" />
            ) : (
              <Bitcoin className="mr-2 h-4 w-4" />
            )}
            Pay with Bitcoin
          </Button>
        </div>

        <p className="text-xs text-muted-foreground mt-3">Minimum purchase: 10,000 credits ($10)</p>
      </Card>
    </div>
  );
}
