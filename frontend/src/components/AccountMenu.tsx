import { AlertCircle, Settings } from "lucide-react";
import { useOpenSecret } from "@opensecret/react";
import { useQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";

import { getBillingService } from "@/billing/billingService";
import { CreditUsage } from "@/components/CreditUsage";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { useLocalState } from "@/state/useLocalState";
import type { TeamStatus } from "@/types/team";

export function AccountMenu() {
  const os = useOpenSecret();
  const { billingStatus } = useLocalState();
  const productName = billingStatus?.product_name || "";
  const isTeamPlan = productName.toLowerCase().includes("team");

  // Fetch team status if user has team plan
  const { data: teamStatus } = useQuery<TeamStatus>({
    queryKey: ["teamStatus"],
    queryFn: async () => {
      const billingService = getBillingService();
      return await billingService.getTeamStatus();
    },
    enabled: isTeamPlan && !!os.auth.user && !!billingStatus
  });

  // Show alert badge if user has team plan but hasn't created team yet
  const showTeamSetupAlert =
    isTeamPlan && teamStatus?.has_team_subscription && !teamStatus?.team_created;

  const settingsSearch = showTeamSetupAlert
    ? ({ tab: "team", team_setup: true } as const)
    : ({ tab: "account" } as const);

  return (
    <div className="flex flex-col gap-2">
      <Link to="/pricing" className="self-end">
        <Badge
          variant="secondary"
          className="bg-[hsl(var(--primary))] text-[hsl(var(--primary-foreground))] hover:bg-[hsl(var(--primary))]/80 dark:bg-white dark:text-[hsl(var(--background))] dark:hover:bg-white/80 transition-colors cursor-pointer uppercase"
        >
          {billingStatus ? `${billingStatus.product_name} Plan` : "Loading..."}
        </Badge>
      </Link>
      <CreditUsage />
      <Button variant="outline" className="gap-2 relative" asChild>
        <Link to="/settings" search={settingsSearch}>
          <Settings className="w-4 h-4" />
          Settings
          {showTeamSetupAlert && (
            <AlertCircle className="absolute -top-1 -right-1 h-4 w-4 text-amber-500 bg-background rounded-full" />
          )}
        </Link>
      </Button>
    </div>
  );
}
