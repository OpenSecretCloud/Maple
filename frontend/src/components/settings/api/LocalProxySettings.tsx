import { useOpenSecret } from "@opensecret/react";
import { AlertCircle, Loader2 } from "lucide-react";
import { ProxyConfigSection } from "@/components/apikeys/ProxyConfigSection";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { SettingsSection } from "../SettingsPage";
import { useApiKeys } from "./useApiKeys";

export function LocalProxySettings() {
  const { createApiKey } = useOpenSecret();
  const { data: apiKeys, isLoading, error, refetch } = useApiKeys();

  const handleRequestNewApiKey = async (name: string) => {
    try {
      const response = await createApiKey(name);
      await refetch();
      return response.key;
    } catch (createFailure) {
      console.error("Failed to create API key for proxy:", createFailure);
      throw createFailure;
    }
  };

  return (
    <SettingsSection
      title="Local OpenAI proxy"
      description="Run an OpenAI-compatible endpoint from the Maple desktop app."
    >
      {isLoading ? (
        <div className="flex items-center gap-2 py-6 text-sm text-muted-foreground">
          <Loader2 className="h-4 w-4 animate-spin" />
          Loading proxy settings...
        </div>
      ) : error ? (
        <Alert variant="destructive">
          <AlertCircle className="h-4 w-4" />
          <AlertDescription>Failed to load API keys. Please try again.</AlertDescription>
        </Alert>
      ) : (
        <ProxyConfigSection apiKeys={apiKeys ?? []} onRequestNewApiKey={handleRequestNewApiKey} />
      )}
    </SettingsSection>
  );
}
