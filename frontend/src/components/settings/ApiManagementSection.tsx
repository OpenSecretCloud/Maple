import { useState, useEffect } from "react";
import { useOpenSecret } from "@opensecret/react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { useLocalState } from "@/state/useLocalState";
import { isTauriDesktop } from "@/utils/platform";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import {
  Plus,
  Loader2,
  Sparkles,
  Zap,
  Shield,
  Rocket,
  Server,
  Key,
  CreditCard
} from "lucide-react";
import { CreateApiKeyDialog } from "@/components/apikeys/CreateApiKeyDialog";
import { ApiKeysList } from "@/components/apikeys/ApiKeysList";
import { ApiCreditsSection } from "@/components/apikeys/ApiCreditsSection";
import { ProxyConfigSection } from "@/components/apikeys/ProxyConfigSection";

interface ApiKey {
  name: string;
  created_at: string;
}

interface ApiManagementSectionProps {
  creditsSuccess?: boolean;
}

export function ApiManagementSection({ creditsSuccess = false }: ApiManagementSectionProps) {
  const [isCreateDialogOpen, setIsCreateDialogOpen] = useState(false);
  const [showCreditSuccess, setShowCreditSuccess] = useState(creditsSuccess);
  const isTauriDesktopPlatform = isTauriDesktop();
  const { listApiKeys, auth, createApiKey } = useOpenSecret();
  const { billingStatus } = useLocalState();
  const navigate = useNavigate();
  const queryClient = useQueryClient();

  // Handle credits_success
  useEffect(() => {
    if (creditsSuccess) {
      setShowCreditSuccess(true);
      queryClient.invalidateQueries({ queryKey: ["apiCreditBalance"] });
      // Clear credits_success from URL to prevent repeat flash on refresh
      navigate({ to: "/settings", search: { tab: "api" }, replace: true });
      const timer = setTimeout(() => setShowCreditSuccess(false), 5000);
      return () => clearTimeout(timer);
    }
  }, [creditsSuccess, queryClient, navigate]);

  // Check if user has API access
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
    try {
      const response = await createApiKey(name);
      await refetch();
      return response.key;
    } catch (error) {
      console.error("Failed to create API key for proxy:", error);
      throw error;
    }
  };

  return (
    <div className="space-y-8">
      <div>
        <h2 className="text-2xl font-semibold tracking-tight">API Management</h2>
        <p className="text-muted-foreground mt-1">
          Manage API keys and configure access to Maple services.{" "}
          <a
            href="https://blog.trymaple.ai/maple-proxy-documentation/"
            target="_blank"
            rel="noopener noreferrer"
            className="text-primary underline hover:no-underline"
          >
            Read more
          </a>
        </p>
      </div>

      {/* Loading state */}
      {(isBillingLoading || isLoading) && (
        <div className="flex items-center justify-center py-12">
          <Loader2 className="h-6 w-6 animate-spin" />
        </div>
      )}

      {/* Error state */}
      {error && !isLoading && (
        <div className="text-destructive text-sm">Failed to load API keys. Please try again.</div>
      )}

      {/* Upgrade prompt for users without API access */}
      {!isBillingLoading && !isLoading && !hasApiAccess && (
        <Card className="p-6 bg-gradient-to-br from-amber-500/10 to-orange-500/10 border-amber-500/20">
          <div className="space-y-4">
            <div className="flex items-center gap-2 text-lg font-semibold">
              <Sparkles className="h-5 w-5 text-amber-500" />
              Unlock API Access
            </div>
            <p className="text-sm text-muted-foreground">
              Upgrade to a paid plan to access powerful API features
            </p>
            <div className="space-y-3">
              <div className="flex items-center gap-3">
                <div className="p-2 bg-amber-500/20 rounded-lg">
                  <Zap className="h-5 w-5 text-amber-500" />
                </div>
                <div>
                  <h4 className="font-semibold">Programmatic Access</h4>
                  <p className="text-sm text-muted-foreground">
                    Integrate Maple directly into your applications
                  </p>
                </div>
              </div>
              <div className="flex items-center gap-3">
                <div className="p-2 bg-blue-500/20 rounded-lg">
                  <Shield className="h-5 w-5 text-blue-500" />
                </div>
                <div>
                  <h4 className="font-semibold">Secure API Keys</h4>
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
                  <h4 className="font-semibold">Extend Your Subscription</h4>
                  <p className="text-sm text-muted-foreground">
                    Purchase extra credits to extend your usage
                  </p>
                </div>
              </div>
            </div>
            <div className="pt-2">
              <p className="text-sm text-muted-foreground mb-3">
                Starting at just <span className="font-semibold text-foreground">$20/month</span>{" "}
                with the Pro plan
              </p>
              <Button onClick={() => navigate({ to: "/pricing" })} className="w-full sm:w-auto">
                <Sparkles className="mr-2 h-4 w-4" />
                View Pricing Plans
              </Button>
            </div>
          </div>
        </Card>
      )}

      {/* Full API dashboard for users with access */}
      {!isBillingLoading && !isLoading && hasApiAccess && !error && (
        <Tabs defaultValue="credits">
          <TabsList
            className={`grid w-full ${isTauriDesktopPlatform ? "grid-cols-3" : "grid-cols-2"}`}
          >
            <TabsTrigger value="credits" className="flex items-center gap-2">
              <CreditCard className="h-4 w-4" />
              Credits
            </TabsTrigger>
            <TabsTrigger value="api-keys" className="flex items-center gap-2">
              <Key className="h-4 w-4" />
              API Keys
            </TabsTrigger>
            {isTauriDesktopPlatform && (
              <TabsTrigger value="local-proxy" className="flex items-center gap-2">
                <Server className="h-4 w-4" />
                Local Proxy
              </TabsTrigger>
            )}
          </TabsList>

          <TabsContent value="credits" className="mt-4">
            <div className="space-y-4">
              <ApiCreditsSection showSuccessMessage={showCreditSuccess} />
            </div>
          </TabsContent>

          <TabsContent value="api-keys" className="mt-4">
            <div className="space-y-4">
              <div className="space-y-3">
                <h3 className="font-medium text-sm">API Keys</h3>
                <Button onClick={() => setIsCreateDialogOpen(true)} size="sm" className="w-full">
                  <Plus className="mr-1.5 h-3.5 w-3.5" />
                  Create New API Key
                </Button>
                {apiKeys && apiKeys.length > 0 && (
                  <ApiKeysList apiKeys={apiKeys} onKeyDeleted={handleKeyDeleted} />
                )}
                <div className="text-xs text-muted-foreground space-y-1.5 pt-2">
                  <p>API keys allow you to integrate Maple into your applications and workflows.</p>
                  <p>
                    Keep your API keys secure and never share them publicly. Treat them like
                    passwords.
                  </p>
                </div>
              </div>
            </div>
          </TabsContent>

          {isTauriDesktopPlatform && (
            <TabsContent value="local-proxy" className="mt-4">
              <div className="space-y-4">
                <ProxyConfigSection
                  apiKeys={apiKeys || []}
                  onRequestNewApiKey={handleProxyApiKeyRequest}
                />
              </div>
            </TabsContent>
          )}
        </Tabs>
      )}

      {/* Create dialog */}
      <CreateApiKeyDialog
        open={isCreateDialogOpen}
        onOpenChange={setIsCreateDialogOpen}
        onKeyCreated={handleKeyCreated}
      />
    </div>
  );
}
