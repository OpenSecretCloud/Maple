import ChatBox from "@/components/ChatBox";
import { BillingStatus } from "@/components/BillingStatus";

import { createFileRoute, useNavigate, Link } from "@tanstack/react-router";
import { useOpenSecret } from "@opensecret/react";
import { useLocalState } from "@/state/useLocalState";
import { Sidebar, SidebarToggle } from "@/components/Sidebar";
import { cva } from "class-variance-authority";
import { InfoContent } from "@/components/Explainer";
import { useState, useCallback, useEffect } from "react";
import { useTranslation } from 'react-i18next';
import { Card, CardHeader } from "@/components/ui/card";
import { VerificationModal } from "@/components/VerificationModal";
import { TopNav } from "@/components/TopNav";
import { Marketing } from "@/components/Marketing";
import { SimplifiedFooter } from "@/components/SimplifiedFooter";
import { TeamManagementDialog } from "@/components/team/TeamManagementDialog";
import { useQuery } from "@tanstack/react-query";
import { getBillingService } from "@/billing/billingService";
import type { TeamStatus } from "@/types/team";

const homeVariants = cva("grid h-full w-full overflow-hidden", {
  variants: {
    sidebar: {
      true: "grid-cols-1 md:grid-cols-[280px_1fr]",
      false: "grid-cols-1"
    }
  },
  defaultVariants: {
    sidebar: false
  }
});

type IndexSearchOptions = {
  login?: string;
  next?: string;
  team_setup?: boolean;
};

function validateSearch(search: Record<string, unknown>): IndexSearchOptions {
  return {
    login: search?.login === "true" ? "true" : undefined,
    next: search.next ? (search.next as string) : undefined,
    team_setup: search?.team_setup === true || search?.team_setup === "true" ? true : undefined
  };
}

export const Route = createFileRoute("/")({
  component: Index,
  validateSearch
});

function Index() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const localState = useLocalState();

  const { login, next, team_setup } = Route.useSearch();

  const os = useOpenSecret();

  useEffect(() => {
    if (login === "true") {
      navigate({
        to: "/login",
        search: next ? { next } : undefined
      });
    }
  }, [login, next, navigate]);

  const [isSidebarOpen, setIsSidebarOpen] = useState(false);
  const [teamDialogOpen, setTeamDialogOpen] = useState(false);

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

  const toggleSidebar = useCallback(() => setIsSidebarOpen((prev) => !prev), []);

  async function handleSubmit(
    input: string,
    systemPrompt?: string,
    images?: File[],
    documentText?: string,
    documentMetadata?: { filename: string; fullContent: string }
  ) {
    // Allow submission if there's text input, images, or a document
    const hasTextInput = input.trim() !== "";
    const hasImages = images && images.length > 0;
    const hasDocument = documentText && documentText.trim() !== "";

    if (!hasTextInput && !hasImages && !hasDocument) {
      return; // Nothing to submit
    }

    // Build the final input - handle case where there might be no text input
    let finalInput = input.trim();

    if (documentText && finalInput) {
      // If there's both document and text input, combine them
      finalInput = `${documentText}\n\n${finalInput}`;
    } else if (documentText && !finalInput) {
      // If only document, just use the document text
      finalInput = documentText;
    }
    // If only images with no text, finalInput will be empty string which is fine

    localState.setUserPrompt(finalInput);
    localState.setSystemPrompt(systemPrompt?.trim() || null);
    localState.setUserImages(images || []);

    // Store document metadata if provided (we'll need to add this to LocalState)
    if (documentMetadata) {
      // For now, we'll include it in the prompt until we add proper document storage
      // TODO: Add document metadata to LocalState
    }

    const id = await localState.addChat();
    navigate({ to: "/chat/$chatId", params: { chatId: id } });
  }

  return (
    <div className={homeVariants({ sidebar: os.auth.user !== undefined })}>
      {os.auth.user && (
        <Sidebar chatId={undefined} isOpen={isSidebarOpen} onToggle={toggleSidebar} />
      )}
      <main className="flex flex-col items-center gap-8 px-4 sm:px-8 py-16 h-full overflow-y-auto">
        {os.auth.user && !isSidebarOpen && (
          <div className="fixed top-4 left-4 z-20 md:hidden">
            <SidebarToggle onToggle={toggleSidebar} />
          </div>
        )}

        {!os.auth.user && <TopNav />}
        {os.auth.user ? (
          <>
            <div className="w-full max-w-[35rem] flex flex-col gap-8 ">
              <div className="flex flex-col items-center gap-2">
                <Link to="/">
                  <img
                    src="/maple-logo.svg"
                    alt="Maple AI logo"
                    className="w-[10rem] hidden dark:block"
                  />
                  <img
                    src="/maple-logo-dark.svg"
                    alt="Maple AI logo"
                    className="w-[10rem] block dark:hidden filter drop-shadow-sm"
                  />
                </Link>
                <h2 className="text-2xl font-light leading-none tracking-tight text-center text-balance text-foreground dark:text-white">
                  {t('app.description')}
                </h2>
              </div>
              <div className="self-center">
                <BillingStatus />
              </div>
              <div className="col-span-3">
                {os.auth.user && <ChatBox startTall={true} onSubmit={handleSubmit} />}
              </div>
              <Card className="bg-card/80 backdrop-blur-sm">
                <CardHeader>
                  <InfoContent />
                </CardHeader>
              </Card>
              <SimplifiedFooter />
            </div>
          </>
        ) : (
          <Marketing />
        )}

        <VerificationModal />
      </main>

      {/* Team Management Dialog */}
      {os.auth.user && (
        <TeamManagementDialog
          open={teamDialogOpen}
          onOpenChange={setTeamDialogOpen}
          teamStatus={teamStatus}
        />
      )}
    </div>
  );
}
