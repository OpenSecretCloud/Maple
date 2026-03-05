import { useEffect, useState } from "react";
import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { UnifiedChat } from "@/components/UnifiedChat";
import { Marketing } from "@/components/Marketing";
import { TopNav } from "@/components/TopNav";
import { VerificationModal } from "@/components/VerificationModal";
import { GuestPaymentWarningDialog } from "@/components/GuestPaymentWarningDialog";
import { TeamManagementDialog } from "@/components/team/TeamManagementDialog";
import { ApiKeyManagementDialog } from "@/components/apikeys/ApiKeyManagementDialog";
import { PromoDialog, hasSeenPromo, markPromoAsSeen } from "@/components/PromoDialog";
import { useOpenSecret } from "@opensecret/react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { getBillingService } from "@/billing/billingService";
import { useLocalState } from "@/state/useLocalState";
import type { TeamStatus } from "@/types/team";
import type { DiscountResponse } from "@/billing/billingApi";

type IndexSearchOptions = {
  login?: string;
  next?: string;
  team_setup?: boolean;
  credits_success?: boolean;
  api_settings?: boolean;
};

function validateSearch(search: Record<string, unknown>): IndexSearchOptions {
  return {
    login: search?.login === "true" ? "true" : undefined,
    next: search.next ? (search.next as string) : undefined,
    team_setup: search?.team_setup === true || search?.team_setup === "true" ? true : undefined,
    credits_success:
      search?.credits_success === true || search?.credits_success === "true" ? true : undefined,
    api_settings:
      search?.api_settings === true || search?.api_settings === "true" ? true : undefined
  };
}

export const Route = createFileRoute("/")({
  component: Index,
  validateSearch
});

function Index() {
  const navigate = useNavigate();
  const os = useOpenSecret();
  const queryClient = useQueryClient();
  const { setBillingStatus, billingStatus } = useLocalState();

  const { login, next, team_setup, credits_success, api_settings } = Route.useSearch();

  // Modal states
  const [teamDialogOpen, setTeamDialogOpen] = useState(false);
  const [apiKeyDialogOpen, setApiKeyDialogOpen] = useState(false);
  const [showCreditSuccess, setShowCreditSuccess] = useState(false);
  const [showGuestPaymentWarning, setShowGuestPaymentWarning] = useState(false);
  const [promoDialogOpen, setPromoDialogOpen] = useState(false);

  // Proactively fetch billing status for authenticated users
  useQuery({
    queryKey: ["billingStatus"],
    queryFn: async () => {
      const billingService = getBillingService();
      const status = await billingService.getBillingStatus();
      setBillingStatus(status);
      return status;
    },
    enabled: !!os.auth.user
  });

  // Handle login redirect
  useEffect(() => {
    if (login === "true") {
      navigate({
        to: "/login",
        search: next ? { next } : undefined
      });
    }
  }, [login, next, navigate]);

  // Fetch team status for the dialog
  const { data: teamStatus } = useQuery<TeamStatus>({
    queryKey: ["teamStatus"],
    queryFn: async () => {
      const billingService = getBillingService();
      return await billingService.getTeamStatus();
    },
    enabled: !!os.auth.user
  });

  // Fetch active discount/promotion for promo dialog
  const { data: discount } = useQuery<DiscountResponse>({
    queryKey: ["discount"],
    queryFn: async () => {
      const billingService = getBillingService();
      return await billingService.getDiscount();
    },
    staleTime: 5 * 60 * 1000,
    enabled: !!os.auth.user
  });

  // Auto-open team dialog if team_setup is true
  useEffect(() => {
    if (team_setup && os.auth.user && teamStatus) {
      setTeamDialogOpen(true);
      // Clear the query param to prevent re-opening on refresh
      navigate({ to: "/", replace: true });
    }
  }, [team_setup, os.auth.user, teamStatus, navigate]);

  // Handle credits_success - open API key dialog and refresh balance
  useEffect(() => {
    if (credits_success && os.auth.user) {
      setApiKeyDialogOpen(true);
      setShowCreditSuccess(true);
      // Refresh the credit balance
      queryClient.invalidateQueries({ queryKey: ["apiCreditBalance"] });
      // Clear the query param to prevent re-opening on refresh
      navigate({ to: "/", replace: true });
      // Clear success message after 5 seconds
      const timer = setTimeout(() => setShowCreditSuccess(false), 5000);
      return () => clearTimeout(timer);
    }
  }, [credits_success, os.auth.user, navigate, queryClient]);

  // Handle api_settings - open API key dialog directly
  useEffect(() => {
    if (api_settings && os.auth.user) {
      setApiKeyDialogOpen(true);
      // Clear the query param to prevent re-opening on refresh
      navigate({ to: "/", replace: true });
    }
  }, [api_settings, os.auth.user, navigate]);

  // Check if guest user needs to pay
  const isGuestUser = os.auth.user?.user.login_method?.toLowerCase() === "guest";
  const isOnFreePlan = billingStatus?.product_name?.toLowerCase().includes("free") ?? false;
  const shouldShowGuestPaymentWarning = isGuestUser && isOnFreePlan && billingStatus !== null;

  // Show guest payment warning if needed (but not on pricing page)
  useEffect(() => {
    const currentPath = window.location.pathname;
    if (shouldShowGuestPaymentWarning && currentPath !== "/pricing") {
      setShowGuestPaymentWarning(true);
    } else {
      setShowGuestPaymentWarning(false);
    }
  }, [shouldShowGuestPaymentWarning]);

  // Show promo dialog for free users with active discount (one-time per promo)
  // This has LOWEST priority - don't show if other important dialogs should be visible
  useEffect(() => {
    if (!os.auth.user || !billingStatus || !discount?.active) return;

    // Check if higher-priority dialogs should be shown
    const needsEmailVerification =
      !import.meta.env.DEV && !isGuestUser && !os.auth.user.user.email_verified;

    // Don't show promo if higher-priority dialogs are active
    if (needsEmailVerification || shouldShowGuestPaymentWarning) return;

    if (isOnFreePlan && !hasSeenPromo(discount.name)) {
      // Mark as seen IMMEDIATELY (before showing) to prevent bugs
      markPromoAsSeen(discount.name);
      setPromoDialogOpen(true);
    }
  }, [
    os.auth.user,
    billingStatus,
    discount,
    isGuestUser,
    isOnFreePlan,
    shouldShowGuestPaymentWarning
  ]);

  // Show marketing page for non-authenticated users
  if (!os.auth.user) {
    return (
      <>
        <TopNav />
        <Marketing />
        <VerificationModal />
      </>
    );
  }

  // Show unified chat for authenticated users
  return (
    <>
      <UnifiedChat />

      {/* Modals */}
      <VerificationModal />
      <GuestPaymentWarningDialog
        open={showGuestPaymentWarning}
        onOpenChange={setShowGuestPaymentWarning}
      />

      {/* Team Management Dialog */}
      <TeamManagementDialog
        open={teamDialogOpen}
        onOpenChange={setTeamDialogOpen}
        teamStatus={teamStatus}
      />

      {/* API Key Management Dialog */}
      <ApiKeyManagementDialog
        open={apiKeyDialogOpen}
        onOpenChange={setApiKeyDialogOpen}
        showCreditSuccessMessage={showCreditSuccess}
      />

      {/* Promo Dialog - shows once per promo for free users */}
      {discount?.active && (
        <PromoDialog open={promoDialogOpen} onOpenChange={setPromoDialogOpen} discount={discount} />
      )}
    </>
  );
}
