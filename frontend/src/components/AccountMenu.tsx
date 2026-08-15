import { useQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { useOpenSecret } from "@opensecret/react";
import { AlertCircle, Settings } from "lucide-react";
import { getBillingService } from "@/billing/billingService";
import { useBillingStatusQuery } from "@/billing/useBillingStatusQuery";
import { useCompactSettingsLayout } from "@/components/settings/useCompactSettingsLayout";
import { useBillingState } from "@/state/useLocalState";
import type { TeamStatus } from "@/types/team";
import { SETTINGS_HOME_PARENT_STATE_KEY } from "@/utils/settingsNavigation";
import { getTeamSeatMismatch } from "@/utils/teamSeats";
import { cn } from "@/utils/utils";

export function AccountMenu({ pagePresentation = false }: { pagePresentation?: boolean }) {
  const os = useOpenSecret();
  const { billingStatus, billingStatusAccountId, clearBillingStatus, setBillingStatus } =
    useBillingState();
  const isCompactSettingsLayout = useCompactSettingsLayout();
  const isTeamPlan = billingStatus?.product_name?.toLowerCase().includes("team") ?? false;

  // Keep shared plan and team-attention state current on every authenticated
  // route, including Agent Mode. Some routes do not own a route-level refresh.
  useBillingStatusQuery({
    accountId: os.auth.user?.user.id ?? null,
    billingStatusAccountId,
    clearBillingStatus,
    refetchOnMount: true,
    setBillingStatus
  });

  const { data: teamStatus } = useQuery<TeamStatus>({
    queryKey: ["teamStatus"],
    queryFn: () => getBillingService().getTeamStatus(),
    enabled: isTeamPlan && !!os.auth.user && !!billingStatus
  });

  const needsTeamSetup = !!teamStatus?.has_team_subscription && teamStatus.team_created === false;
  const teamSeatMismatch = getTeamSeatMismatch(teamStatus);
  const attentionLabel = teamSeatMismatch
    ? "Team usage paused"
    : needsTeamSetup
      ? "Team setup required"
      : undefined;

  return (
    <Link
      to={isCompactSettingsLayout ? "/settings" : "/settings/account"}
      state={
        isCompactSettingsLayout && pagePresentation
          ? (previous) => ({ ...previous, [SETTINGS_HOME_PARENT_STATE_KEY]: true })
          : undefined
      }
      aria-label={attentionLabel ? `Open settings, ${attentionLabel}` : "Open settings"}
      title="Settings"
      className={cn(
        "relative flex shrink-0 items-center justify-center rounded-full bg-[hsl(var(--sidebar-chrome))] text-[hsl(var(--on-sidebar-chrome))] shadow-none ring-0 transition-colors hover:bg-[hsl(var(--sidebar-chrome-hover))] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
        pagePresentation ? "h-11 w-11" : "h-9 w-9"
      )}
    >
      <Settings className={pagePresentation ? "h-5 w-5" : "h-4 w-4"} />
      {(teamSeatMismatch || needsTeamSetup) && (
        <AlertCircle
          className={`absolute -right-1 -top-1 h-4 w-4 rounded-full bg-background ${
            teamSeatMismatch ? "text-destructive" : "text-maple-warning"
          }`}
        />
      )}
    </Link>
  );
}
