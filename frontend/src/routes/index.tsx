import { useEffect, useState } from "react";
import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { UnifiedChat } from "@/components/UnifiedChat";
import { Marketing } from "@/components/Marketing";
import { TopNav } from "@/components/TopNav";
import { VerificationModal } from "@/components/VerificationModal";
import { TeamManagementDialog } from "@/components/team/TeamManagementDialog";
import { ApiKeyManagementDialog } from "@/components/apikeys/ApiKeyManagementDialog";
import { useOpenSecret } from "@opensecret/react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { getBillingService } from "@/billing/billingService";
import { useLocalState } from "@/state/useLocalState";
import type { TeamStatus } from "@/types/team";

type IndexSearchOptions = {
  login?: string;
  next?: string;
  team_setup?: boolean;
  credits_success?: boolean;
};

function validateSearch(search: Record<string, unknown>): IndexSearchOptions {
  return {
    login: search?.login === "true" ? "true" : undefined,
    next: search.next ? (search.next as string) : undefined,
    team_setup: search?.team_setup === true || search?.team_setup === "true" ? true : undefined,
    credits_success:
      search?.credits_success === true || search?.credits_success === "true" ? true : undefined
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
  const { setBillingStatus } = useLocalState();

  const { login, next, team_setup, credits_success } = Route.useSearch();

  // Modal states
  const [teamDialogOpen, setTeamDialogOpen] = useState(false);
  const [apiKeyDialogOpen, setApiKeyDialogOpen] = useState(false);
  const [showCreditSuccess, setShowCreditSuccess] = useState(false);

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
    </>
  );
}
