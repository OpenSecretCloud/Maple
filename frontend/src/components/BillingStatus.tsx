import { useNavigate } from "@tanstack/react-router";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Button } from "@/components/ui/button";
import { BillingDebugger } from "./BillingDebugger";
import { useLocalState } from "@/state/useLocalState";
import { getBillingService } from "@/billing/billingService";

export function BillingStatus() {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const { setBillingStatus } = useLocalState();

  const { data: billingStatus, isLoading } = useQuery({
    queryKey: ["billingStatus"],
    queryFn: async () => {
      const billingService = getBillingService();
      const status = await billingService.getBillingStatus();
      setBillingStatus(status);
      return status;
    }
  });

  if (isLoading || !billingStatus) {
    return (
      <Button variant="default" disabled className="opacity-50 h-auto whitespace-normal py-2">
        Loading...
      </Button>
    );
  }

  const isFree = billingStatus.product_name.toLowerCase().includes("free");
  const isPro = billingStatus.product_name.toLowerCase().includes("pro");

  const getChatsText = () => {
    if (isFree) {
      if (billingStatus.chats_remaining === null || billingStatus.chats_remaining <= 0) {
        return "You've run out of messages, upgrade to keep chatting!";
      }
      return `Free Plan — ${billingStatus.chats_remaining} Message${billingStatus.chats_remaining === 1 ? "" : "s"} Left This Week`;
    }
    if (!billingStatus.can_chat) {
      if (isPro) {
        return "Contact us to increase your limits";
      }
      return "You've run out of messages, upgrade to keep chatting!";
    }
    return `${billingStatus.product_name} Plan`;
  };

  return (
    <div className="flex flex-col items-center gap-2 w-full">
      <Button
        variant="default"
        onClick={() =>
          !billingStatus.can_chat && isPro
            ? (window.location.href = "mailto:team@opensecret.cloud")
            : navigate({ to: "/pricing" })
        }
        className="h-auto whitespace-normal py-2"
      >
        {getChatsText()}
      </Button>
      {import.meta.env.DEV && (
        <BillingDebugger
          currentStatus={billingStatus}
          onOverride={(newStatus) => {
            queryClient.setQueryData(["billingStatus"], newStatus);
          }}
        />
      )}
    </div>
  );
}
