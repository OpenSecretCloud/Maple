import { useState, useEffect } from "react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Alert, AlertDescription } from "@/components/ui/alert";

interface RenameChatDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  chatId: string;
  currentTitle: string;
  onRename: (chatId: string, newTitle: string) => Promise<void>;
}

export function RenameChatDialog({
  open,
  onOpenChange,
  chatId,
  currentTitle,
  onRename
}: RenameChatDialogProps) {
  const [newTitle, setNewTitle] = useState("");
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Set the initial value when dialog opens
  useEffect(() => {
    if (open) {
      setNewTitle(currentTitle);
    }
  }, [open, currentTitle]);

  useEffect(() => {
    if (!open) {
      // Reset form state when dialog closes
      setNewTitle(currentTitle);
      setError(null);
      setIsLoading(false);
    }
  }, [open, currentTitle]);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setError(null);

    const trimmedTitle = newTitle.trim();

    if (!trimmedTitle) {
      setError("Chat title cannot be empty.");
      return;
    }

    // Check if the new title is the same as the current title
    if (trimmedTitle === currentTitle.trim()) {
      setError("Please enter a different title.");
      return;
    }

    setIsLoading(true);
    try {
      await onRename(chatId, trimmedTitle);
      // Close dialog immediately on success
      onOpenChange(false);
    } catch (error) {
      console.error("Failed to rename chat:", error);
      setError("Failed to rename chat. Please try again.");
    } finally {
      setIsLoading(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-[425px]">
        <DialogHeader>
          <DialogTitle>Rename Chat</DialogTitle>
          <DialogDescription>Enter a new name for this chat conversation.</DialogDescription>
        </DialogHeader>
        <form onSubmit={handleSubmit} className="grid gap-4 py-4">
          {error && (
            <Alert variant="destructive">
              <AlertDescription>{error}</AlertDescription>
            </Alert>
          )}
          <div className="grid gap-2">
            <Label htmlFor="chat-title">Chat Title</Label>
            <Input
              id="chat-title"
              type="text"
              value={newTitle}
              onChange={(e) => setNewTitle(e.target.value)}
              required
              autoFocus
              onKeyDown={(e) => {
                if (e.key === "Enter") {
                  e.preventDefault();
                  // Use requestSubmit for better form validation
                  const form = e.currentTarget.form;
                  if (form) form.requestSubmit();
                }
              }}
            />
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => onOpenChange(false)} type="button">
              Cancel
            </Button>
            <Button type="submit" disabled={isLoading}>
              {isLoading ? "Renaming..." : "Rename Chat"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
