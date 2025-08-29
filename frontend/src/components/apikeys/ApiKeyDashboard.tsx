import { useState, useEffect } from "react";
import { DialogHeader, DialogTitle, DialogDescription } from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Plus, Loader2, Sparkles, Zap, Shield, Rocket } from "lucide-react";
import { CreateApiKeyDialog } from "./CreateApiKeyDialog";
import { ApiKeysList } from "./ApiKeysList";
import { ApiCreditsSection } from "./ApiCreditsSection";
import { ProxyConfigSection } from "./ProxyConfigSection";
import { useOpenSecret } from "@opensecret/react";
import { useQuery } from "@tanstack/react-query";
import { Separator } from "@/components/ui/separator";
import { useLocalState } from "@/state/useLocalState";
import { useNavigate } from "@tanstack/react-router";
import { proxyService } from "@/services/proxyService";

interface ApiKey {
  name: string;
  created_at: string;
}

interface ApiKeyDashboardProps {
  showCreditSuccessMessage?: boolean;
}

export function ApiKeyDashboard({ showCreditSuccessMessage = false }: ApiKeyDashboardProps) {
  const [isCreateDialogOpen, setIsCreateDialogOpen] = useState(false);
  const [isDesktop, setIsDesktop] = useState(false);
  const { listApiKeys, auth, createApiKey } = useOpenSecret();
  const { billingStatus } = useLocalState();
  const navigate = useNavigate();

  useEffect(() => {
    proxyService.isTauriDesktop().then(setIsDesktop);
  }, []);

  // Check if user has API access (Pro, Team, or Max plans only - not Starter)
  const isBillingLoading = billingStatus === null;
  const productName = billingStatus?.product_name || "";
  const isPro = productName.toLowerCase().includes("pro");
  const isMax = productName.toLowerCase().includes("max");
  const isTeamPlan = productName.toLowerCase().includes("team");
  const hasApiAccess = isPro || isMax || isTeamPlan;

  // Fetch API keys
  const {
    data: apiKeys,
    isLoading,
    error,
    refetch
  } = useQuery<ApiKey[]>({
    queryKey: ["apiKeys"],
    queryFn: async () => {
      const response = await listApiKeys();
      // Sort by creation date (newest first)
      return response.keys.sort(
        (a, b) => new Date(b.created_at).getTime() - new Date(a.created_at).getTime()
      );
    },
    enabled: !!auth.user && !auth.loading
  });

  const handleKeyCreated = () => {
    refetch();
    setIsCreateDialogOpen(false);
  };

  const handleKeyDeleted = () => {
    refetch();
  };

  const handleProxyApiKeyRequest = async (name: string): Promise<string> => {
    // Create a new API key for the proxy directly
    try {
      const response = await createApiKey(name);
      // Refetch to update the list
      await refetch();
      return response.key;
    } catch (error) {
      console.error("Failed to create API key for proxy:", error);
      throw error;
    }
  };

  // Show loading state if billing status or API keys are loading
  if (isBillingLoading || isLoading) {
    return (
      <>
        <DialogHeader>
          <DialogTitle className="text-base">API Key Management</DialogTitle>
          <DialogDescription>Loading...</DialogDescription>
        </DialogHeader>
        <div className="flex items-center justify-center py-8">
          <Loader2 className="h-6 w-6 animate-spin" />
        </div>
      </>
    );
  }

  if (error) {
    return (
      <>
        <DialogHeader>
          <DialogTitle className="text-base">API Key Management</DialogTitle>
          <DialogDescription className="text-destructive">
            Failed to load API keys. Please try again.
          </DialogDescription>
        </DialogHeader>
      </>
    );
  }

  // Show upgrade prompt for users without API access (Free and Starter plans)
  if (!hasApiAccess) {
    return (
      <>
        <DialogHeader>
          <DialogTitle className="text-base flex items-center gap-2">
            <Sparkles className="h-5 w-5 text-amber-500" />
            Unlock API Access
          </DialogTitle>
          <DialogDescription>
            Upgrade to a paid plan to access powerful API features
          </DialogDescription>
        </DialogHeader>

        <div className="mt-6 space-y-4">
          <Card className="p-6 bg-gradient-to-br from-amber-500/10 to-orange-500/10 border-amber-500/20">
            <div className="space-y-4">
              <div className="flex items-center gap-3">
                <div className="p-2 bg-amber-500/20 rounded-lg">
                  <Zap className="h-5 w-5 text-amber-500" />
                </div>
                <div>
                  <h3 className="font-semibold">Programmatic Access</h3>
                  <p className="text-sm text-muted-foreground">
                    Integrate Maple directly into your applications and workflows
                  </p>
                </div>
              </div>

              <div className="flex items-center gap-3">
                <div className="p-2 bg-blue-500/20 rounded-lg">
                  <Shield className="h-5 w-5 text-blue-500" />
                </div>
                <div>
                  <h3 className="font-semibold">Secure API Keys</h3>
                  <p className="text-sm text-muted-foreground">
                    Create and manage multiple API keys with granular control
                  </p>
                </div>
              </div>

              <div className="flex items-center gap-3">
                <div className="p-2 bg-green-500/20 rounded-lg">
                  <Rocket className="h-5 w-5 text-green-500" />
                </div>
                <div>
                  <h3 className="font-semibold">Pay-As-You-Go Credits</h3>
                  <p className="text-sm text-muted-foreground">
                    Purchase credits for API usage at just $1 per 1,000 credits
                  </p>
                </div>
              </div>
            </div>
          </Card>

          <div className="space-y-3">
            <p className="text-sm text-muted-foreground text-center">
              Starting at just <span className="font-semibold text-foreground">$20/month</span> with
              the Pro plan
            </p>

            <Button onClick={() => navigate({ to: "/pricing" })} className="w-full" size="lg">
              <Sparkles className="mr-2 h-4 w-4" />
              View Pricing Plans
            </Button>

            <p className="text-xs text-muted-foreground text-center">
              Unlock API access, increased limits, and premium features
            </p>
          </div>
        </div>
      </>
    );
  }

  return (
    <>
      <DialogHeader>
        <DialogTitle className="text-base">API Key Management</DialogTitle>
        <DialogDescription>
          Manage API keys for programmatic access to Maple services.
        </DialogDescription>
      </DialogHeader>

      <div className="mt-3 space-y-4 overflow-hidden">
        {/* API Credits Section */}
        <ApiCreditsSection showSuccessMessage={showCreditSuccessMessage} />

        <Separator />

        {/* API Keys Section */}
        <div className="space-y-3">
          <h3 className="font-medium text-sm">API Keys</h3>

          {/* Create button */}
          <Button onClick={() => setIsCreateDialogOpen(true)} size="sm" className="w-full">
            <Plus className="mr-1.5 h-3.5 w-3.5" />
            Create New API Key
          </Button>

          {/* API Keys list */}
          {apiKeys && apiKeys.length > 0 && (
            <ApiKeysList apiKeys={apiKeys} onKeyDeleted={handleKeyDeleted} />
          )}

          {/* Info text */}
          <div className="text-xs text-muted-foreground space-y-1.5 pt-2">
            <p>API keys allow you to integrate Maple into your applications and workflows.</p>
            <p>
              Keep your API keys secure and never share them publicly. Treat them like passwords.
            </p>
          </div>
        </div>

        {/* Proxy Configuration Section - Desktop Only */}
        {isDesktop && (
          <>
            <Separator />
            <ProxyConfigSection
              apiKeys={apiKeys || []}
              onRequestNewApiKey={handleProxyApiKeyRequest}
            />
          </>
        )}
      </div>

      {/* Create dialog */}
      <CreateApiKeyDialog
        open={isCreateDialogOpen}
        onOpenChange={setIsCreateDialogOpen}
        onKeyCreated={handleKeyCreated}
      />
    </>
  );
}
