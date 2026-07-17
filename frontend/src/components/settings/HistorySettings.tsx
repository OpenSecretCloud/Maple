import { useState } from "react";
import { useBlocker, useNavigate } from "@tanstack/react-router";
import { useQueryClient } from "@tanstack/react-query";
import { useOpenSecret } from "@opensecret/react";
import { Loader2, Trash2 } from "lucide-react";
import { AlertDestructive } from "@/components/AlertDestructive";
import { Button } from "@/components/ui/button";
import { useSettingsNavigationLock } from "@/contexts/SettingsNavigationLockContext";
import { clearAgentHistoryForUser } from "@/services/agentRuntimeService";
import { SettingsPage, SettingsSection } from "./SettingsPage";

export function HistorySettings() {
  const os = useOpenSecret();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const [isConfirming, setIsConfirming] = useState(false);
  const [isDeleting, setIsDeleting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useBlocker({
    shouldBlockFn: () => isDeleting,
    disabled: !isDeleting,
    enableBeforeUnload: isDeleting
  });
  useSettingsNavigationLock(isDeleting);

  const handleDeleteHistory = async () => {
    setError(null);
    setIsDeleting(true);
    let operationBlock: Awaited<ReturnType<typeof clearAgentHistoryForUser>> | null = null;
    try {
      const conversations = await os.listConversations({ limit: 1 });
      if (conversations.data?.length) {
        await os.deleteConversations();
      }

      operationBlock = await clearAgentHistoryForUser(os.auth.user?.user.id);

      queryClient.invalidateQueries({ queryKey: ["conversations"] });
      queryClient.invalidateQueries({ queryKey: ["pinnedConversations"] });
      queryClient.invalidateQueries({ queryKey: ["projectConversations"] });
      queryClient.invalidateQueries({ queryKey: ["conversationProjects"] });
      queryClient.invalidateQueries({ queryKey: ["conversationProject"] });
      try {
        await navigate({ to: "/", ignoreBlocker: true });
      } catch (navigationError) {
        console.error("History was deleted, but navigation failed:", navigationError);
        window.location.href = "/";
        return;
      }
      window.dispatchEvent(new CustomEvent("newchat", { detail: { projectId: null } }));
    } catch (deleteError) {
      console.error("Error deleting chat and task history:", deleteError);
      setError("Maple could not delete all chat and task history. Please try again.");
    } finally {
      operationBlock?.release();
      setIsDeleting(false);
    }
  };

  return (
    <SettingsPage
      title="Chat and task history"
      description="Manage the chats and tasks stored in your private Maple workspace."
    >
      <SettingsSection
        title="Delete all chat and task history"
        description="This permanently removes every chat and task from your Maple workspace."
        tone="danger"
      >
        <div className="space-y-4">
          {error && (
            <AlertDestructive title="Chat and task history was not deleted" description={error} />
          )}
          {isConfirming ? (
            <div className="rounded-lg border border-destructive/40 bg-destructive/5 p-4">
              <p className="text-sm font-medium">Delete your entire chat and task history?</p>
              <p className="mt-1 text-sm text-muted-foreground">This action cannot be undone.</p>
              <div className="mt-4 flex flex-col-reverse gap-2 sm:flex-row sm:justify-end">
                <Button
                  type="button"
                  variant="outline"
                  onClick={() => setIsConfirming(false)}
                  disabled={isDeleting}
                >
                  Cancel
                </Button>
                <Button
                  type="button"
                  variant="destructive"
                  onClick={handleDeleteHistory}
                  disabled={isDeleting}
                >
                  {isDeleting && <Loader2 className="mr-2 h-4 w-4 animate-spin" />}
                  {isDeleting ? "Deleting..." : "Delete all history"}
                </Button>
              </div>
            </div>
          ) : (
            <Button type="button" variant="destructive" onClick={() => setIsConfirming(true)}>
              <Trash2 className="mr-2 h-4 w-4" />
              Delete all history
            </Button>
          )}
        </div>
      </SettingsSection>
    </SettingsPage>
  );
}
