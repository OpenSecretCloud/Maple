import { useState } from "react";
import { DialogHeader, DialogTitle, DialogDescription } from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Plus, Loader2 } from "lucide-react";
import { CreateApiKeyDialog } from "./CreateApiKeyDialog";
import { ApiKeysList } from "./ApiKeysList";
import { ApiCreditsSection } from "./ApiCreditsSection";
import { useOpenSecret } from "@opensecret/react";
import { useQuery } from "@tanstack/react-query";
import { Separator } from "@/components/ui/separator";

interface ApiKey {
  name: string;
  created_at: string;
}

interface ApiKeyDashboardProps {
  showCreditSuccessMessage?: boolean;
}

export function ApiKeyDashboard({ showCreditSuccessMessage = false }: ApiKeyDashboardProps) {
  const [isCreateDialogOpen, setIsCreateDialogOpen] = useState(false);
  const { listApiKeys, auth } = useOpenSecret();

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

  if (isLoading) {
    return (
      <>
        <DialogHeader>
          <DialogTitle className="text-base">API Key Management</DialogTitle>
          <DialogDescription>Loading API keys...</DialogDescription>
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
