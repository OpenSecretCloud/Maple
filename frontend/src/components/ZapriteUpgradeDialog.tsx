import { useCallback, useEffect, useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Loader2 } from "lucide-react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { getBillingService } from "@/billing/billingService";
import { openValidatedCheckoutUrl } from "@/billing/billingApi";
import {
  formatUsdFromCents,
  isZapriteUpgradeTerminal,
  type ZapriteUpgradeQuote,
  type ZapriteUpgradeStatusResponse
} from "@/billing/zapriteUpgrade";
import { isIOS, isMobile, isTauri } from "@/utils/platform";

type ZapriteUpgradeDialogProps = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  userId: string | null;
  targetProductId: string | null;
  resumeUpgradeId?: string | null;
};

function statusCopy(status: string): string {
  switch (status) {
    case "PENDING":
    case "PROCESSING":
    case "PAID":
    case "OVERPAID":
      return "Waiting for Bitcoin payment to confirm. Your current plan stays active until payment completes.";
    case "UNDERPAID":
      return "The payment is underpaid. Send the remaining amount in the checkout window, or contact support.";
    case "COMPLETED":
      return "Upgrade complete. Your Max entitlement is now active for the rest of this billing period.";
    case "EXPIRED":
      return "This quote or checkout expired. Your current plan is unchanged.";
    case "CANCELED":
      return "This upgrade was canceled. Your current plan is unchanged.";
    case "FAILED":
      return "This upgrade could not be completed. Your current plan is unchanged.";
    default:
      return "Preparing Bitcoin checkout.";
  }
}

export function ZapriteUpgradeDialog({
  open,
  onOpenChange,
  userId,
  targetProductId,
  resumeUpgradeId
}: ZapriteUpgradeDialogProps) {
  const queryClient = useQueryClient();
  const [quote, setQuote] = useState<ZapriteUpgradeQuote | null>(null);
  const [upgradeId, setUpgradeId] = useState<string | null>(resumeUpgradeId ?? null);
  const [idempotencyKey] = useState(() => crypto.randomUUID());
  const [error, setError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const mountedUserId = useRef(userId);

  useEffect(() => {
    mountedUserId.current = userId;
  }, [userId]);

  useEffect(() => {
    if (!open) {
      return;
    }
    setError(null);
    if (resumeUpgradeId) {
      setUpgradeId(resumeUpgradeId);
      return;
    }
    if (!targetProductId) {
      return;
    }
    let cancelled = false;
    setQuote(null);
    getBillingService()
      .createZapriteUpgradeQuote(targetProductId)
      .then((nextQuote) => {
        if (!cancelled && mountedUserId.current === userId) {
          setQuote(nextQuote);
        }
      })
      .catch((err: unknown) => {
        if (!cancelled) {
          setError(err instanceof Error ? err.message : "Failed to load upgrade quote");
        }
      });
    return () => {
      cancelled = true;
    };
  }, [open, resumeUpgradeId, targetProductId, userId]);

  const { data: upgradeStatus } = useQuery<ZapriteUpgradeStatusResponse>({
    queryKey: ["zapriteUpgrade", userId, upgradeId],
    queryFn: async () => {
      if (!upgradeId) {
        throw new Error("Missing upgrade id");
      }
      return getBillingService().getZapriteUpgradeStatus(upgradeId);
    },
    enabled: open && !!userId && !!upgradeId,
    refetchInterval: (query) => {
      const status = query.state.data?.status;
      if (!status || isZapriteUpgradeTerminal(status)) {
        return false;
      }
      return 2000;
    }
  });

  useEffect(() => {
    if (upgradeStatus?.status === "COMPLETED" && mountedUserId.current === userId) {
      void queryClient.invalidateQueries({ queryKey: ["billingStatus"] });
    }
  }, [upgradeStatus?.status, queryClient, userId]);

  const displayedQuote = upgradeStatus?.quote ?? quote;

  const handleConfirm = useCallback(async () => {
    if (!displayedQuote || isIOS()) {
      return;
    }
    setSubmitting(true);
    setError(null);
    try {
      const successUrl =
        isTauri() || isMobile()
          ? "https://trymaple.ai/pricing"
          : `${window.location.origin}/pricing`;
      const created = await getBillingService().createZapriteUpgrade(
        displayedQuote.quote_id,
        idempotencyKey,
        successUrl
      );
      if (mountedUserId.current !== userId) {
        return;
      }
      setUpgradeId(created.upgrade_id);
      await openValidatedCheckoutUrl(created.checkout_url);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to start Bitcoin checkout");
    } finally {
      setSubmitting(false);
    }
  }, [displayedQuote, idempotencyKey, userId]);

  const currentStatus = upgradeStatus?.status;
  const canConfirm = !!displayedQuote && !upgradeId && !isIOS() && !submitting;
  const checkoutUrl = upgradeStatus?.checkout_url;
  const canReopenCheckout =
    !!checkoutUrl && !isIOS() && !!currentStatus && !isZapriteUpgradeTerminal(currentStatus);

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-lg">
        <DialogHeader>
          <DialogTitle>Upgrade with Bitcoin</DialogTitle>
          <DialogDescription>
            The billing server computed this exact amount. Maple does not recalculate the price.
          </DialogDescription>
        </DialogHeader>

        {error && <p className="text-sm text-maple-error">{error}</p>}

        {!displayedQuote && !error && (
          <div className="flex items-center gap-2 text-sm text-muted-foreground">
            <Loader2 className="h-4 w-4 animate-spin" />
            Loading quote...
          </div>
        )}

        {displayedQuote && (
          <div className="space-y-3 text-sm">
            <p>
              {displayedQuote.source.plan_name} → {displayedQuote.target.plan_name}
            </p>
            <p>
              Unused {displayedQuote.source.plan_name} value is credited against the remaining{" "}
              {displayedQuote.target.plan_name} term. Your expiration date stays{" "}
              {new Date(displayedQuote.subscription_end).toLocaleDateString()}.
            </p>
            <p>
              Amount due: <strong>{formatUsdFromCents(displayedQuote.amount_due_cents)}</strong>
              {displayedQuote.target.discount_basis_points
                ? ` including a ${displayedQuote.target.discount_basis_points / 100}% annual Bitcoin discount.`
                : "."}
            </p>
            <p>Quote expires {new Date(displayedQuote.expires_at).toLocaleString()}.</p>
            {currentStatus && <p>{statusCopy(currentStatus)}</p>}
          </div>
        )}

        {isIOS() && (
          <p className="text-sm text-muted-foreground">
            Paid Bitcoin upgrades are not available in the iOS app.
          </p>
        )}

        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            Close
          </Button>
          {canConfirm && (
            <Button variant="primary" onClick={handleConfirm} disabled={submitting}>
              {submitting ? <Loader2 className="h-4 w-4 animate-spin" /> : "Confirm and pay"}
            </Button>
          )}
          {canReopenCheckout && checkoutUrl && (
            <Button
              variant="primary"
              onClick={() => {
                void openValidatedCheckoutUrl(checkoutUrl).catch((err: unknown) => {
                  setError(err instanceof Error ? err.message : "Failed to open checkout");
                });
              }}
            >
              Open checkout
            </Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
