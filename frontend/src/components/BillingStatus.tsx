import { useNavigate } from "@tanstack/react-router";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Button } from "@/components/ui/button";
import { BillingDebugger } from "./BillingDebugger";
import { useLocalState } from "@/state/useLocalState";
import { getBillingService } from "@/billing/billingService";
import { useOpenSecret } from "@opensecret/react";
import type { TeamStatus } from "@/types/team";

export function BillingStatus() {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const { setBillingStatus } = useLocalState();
  const os = useOpenSecret();

  const { data: billingStatus, isLoading } = useQuery({
    queryKey: ["billingStatus"],
    queryFn: async () => {
      const billingService = getBillingService();
      const status = await billingService.getBillingStatus();
      setBillingStatus(status);
      return status;
    }
  });

  // Check if user has team plan
  const isTeamPlan = billingStatus?.product_name?.toLowerCase().includes("team");

  // Fetch team status if user has team plan
  const { data: teamStatus } = useQuery<TeamStatus>({
    queryKey: ["teamStatus"],
    queryFn: async () => {
      const billingService = getBillingService();
      return await billingService.getTeamStatus();
    },
    enabled: isTeamPlan && !!os.auth.user && !!billingStatus
  });

  if (isLoading || !billingStatus) {
    return import.meta.env.DEV ? (
      <BillingDebugger
        currentStatus={billingStatus || null}
        onOverride={(newStatus) => {
          queryClient.setQueryData(["billingStatus"], newStatus);
        }}
      />
    ) : null;
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

    // Show team name for team plans
    if (isTeamPlan && teamStatus?.team_name) {
      return teamStatus.team_name;
    }

    return `${billingStatus.product_name} Plan`;
  };

  // Only show billing status for free plan or when they can't chat
  if (!isFree && billingStatus.can_chat) {
    return import.meta.env.DEV ? (
      <BillingDebugger
        currentStatus={billingStatus}
        onOverride={(newStatus) => {
          queryClient.setQueryData(["billingStatus"], newStatus);
        }}
      />
    ) : null;
  }

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
