import { useEffect } from "react";
import { useOpenSecret } from "@opensecret/react";
import { AlertCircle, Loader2 } from "lucide-react";
import { ProxyConfigSection } from "@/components/apikeys/ProxyConfigSection";
import { proxyService } from "@/services/proxyService";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { SettingsSection } from "../SettingsPage";
import { useApiKeys } from "./useApiKeys";
import { useProxyModels } from "./useProxyModels";

export function LocalProxySettings() {
  const { auth, createApiKey, deleteApiKey } = useOpenSecret();
  const { data: apiKeys, isLoading, error, refetch } = useApiKeys();
  const { data: models, isLoading: modelsLoading, isError: modelsError } = useProxyModels();

  const handleCreateApiKey = async (name: string) => {
    try {
      const response = await createApiKey(name);
      return response.key;
    } catch (createFailure) {
      console.error("Failed to create API key for proxy:", createFailure);
      throw createFailure;
    }
  };

  const handleRefreshApiKeys = async () => {
    await refetch();
  };

  const userId = auth.user?.user.id;
  useEffect(() => {
    if (!userId) return;
    void proxyService
      .cleanupPendingManualProxyKeys(userId, deleteApiKey)
      .then(async () => await refetch())
      .catch((cleanupFailure) => {
        console.error("Failed to retry pending manual proxy key cleanup:", cleanupFailure);
      });
  }, [deleteApiKey, refetch, userId]);

  if (isLoading) {
    return (
      <SettingsSection
        title="Local OpenAI proxy"
        description="Run an OpenAI-compatible endpoint from the Maple desktop app."
      >
        <div className="flex items-center gap-2 py-6 text-sm text-muted-foreground">
          <Loader2 className="h-4 w-4 animate-spin" />
          Loading proxy settings...
        </div>
      </SettingsSection>
    );
  }

  if (error) {
    return (
      <SettingsSection
        title="Local OpenAI proxy"
        description="Run an OpenAI-compatible endpoint from the Maple desktop app."
      >
        <Alert variant="destructive">
          <AlertCircle className="h-4 w-4" />
          <AlertDescription>Failed to load API keys. Please try again.</AlertDescription>
        </Alert>
      </SettingsSection>
    );
  }

  if (!userId) return null;

  return (
    <ProxyConfigSection
      key={userId}
      userId={userId}
      apiKeys={apiKeys ?? []}
      onCreateApiKey={handleCreateApiKey}
      onDeleteApiKey={deleteApiKey}
      onRefreshApiKeys={handleRefreshApiKeys}
      models={models ?? []}
      isModelsLoading={modelsLoading}
      isModelsError={modelsError}
    />
  );
}
