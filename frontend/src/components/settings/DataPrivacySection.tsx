import { useState } from "react";
import { useNavigate } from "@tanstack/react-router";
import { useOpenSecret } from "@opensecret/react";
import { useQueryClient } from "@tanstack/react-query";
import { Button } from "@/components/ui/button";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle
} from "@/components/ui/alert-dialog";
import { Trash2, AlertTriangle } from "lucide-react";
import { useLocalState } from "@/state/useLocalState";

export function DataPrivacySection() {
  const [isDeleteDialogOpen, setIsDeleteDialogOpen] = useState(false);
  const { clearHistory } = useLocalState();
  const os = useOpenSecret();
  const queryClient = useQueryClient();
  const navigate = useNavigate();

  async function handleDeleteHistory() {
    // 1. Delete archived chats (KV)
    try {
      await clearHistory();
      console.log("History (KV) cleared");
    } catch (error) {
      console.error("Error clearing history:", error);
    }

    // 2. Delete server conversations (API)
    try {
      const conversations = await os.listConversations({ limit: 1 });
      if (conversations.data && conversations.data.length > 0) {
        await os.deleteConversations();
        console.log("Server conversations deleted");
      }
    } catch (e) {
      console.error("Error deleting conversations:", e);
    }

    // Refresh UI
    queryClient.invalidateQueries({ queryKey: ["chatHistory"] });
    queryClient.invalidateQueries({ queryKey: ["conversations"] });
    queryClient.invalidateQueries({ queryKey: ["archivedChats"] });
    setIsDeleteDialogOpen(false);
    navigate({ to: "/" });
  }

  return (
    <div className="space-y-8">
      <div>
        <h2 className="text-2xl font-semibold tracking-tight">Data & Privacy</h2>
        <p className="text-muted-foreground mt-1">Manage your data and privacy settings.</p>
      </div>

      {/* Privacy Info */}
      <div className="space-y-4">
        <h3 className="text-lg font-medium">Your Privacy</h3>
        <div className="bg-muted/50 rounded-lg p-4 space-y-2">
          <p className="text-sm">
            Maple uses end-to-end encryption for all your conversations. Your data is encrypted on
            your device and only decrypted inside secure enclaves.
          </p>
          <p className="text-sm text-muted-foreground">
            No one — not even Maple — can access your plaintext data. Your conversations are never
            used for training AI models.
          </p>
        </div>
      </div>

      {/* Delete History */}
      <div className="space-y-4 border-t border-input pt-8">
        <h3 className="text-lg font-medium text-destructive">Danger Zone</h3>
        <div className="space-y-2">
          <p className="text-sm text-muted-foreground">
            Permanently delete your entire chat history. This includes all conversations stored
            locally and on the server. This action cannot be undone.
          </p>
          <Button
            variant="outline"
            className="border-destructive text-destructive hover:bg-destructive/10 gap-2"
            onClick={() => setIsDeleteDialogOpen(true)}
          >
            <Trash2 className="h-4 w-4" />
            Delete All Chat History
          </Button>
        </div>
      </div>

      {/* Confirm Dialog */}
      <AlertDialog open={isDeleteDialogOpen} onOpenChange={setIsDeleteDialogOpen}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle className="flex items-center gap-2">
              <AlertTriangle className="h-5 w-5 text-destructive" />
              Are you sure?
            </AlertDialogTitle>
            <AlertDialogDescription>
              This will permanently delete your entire chat history, including all conversations
              stored locally and on the server. This action cannot be undone.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction onClick={handleDeleteHistory}>Delete All History</AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}
