import { useEffect, useState } from "react";
import { Alert, AlertDescription } from "@/components/ui/alert";
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

interface ConversationProjectDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  mode: "create" | "rename";
  initialName?: string;
  onSubmit: (name: string) => Promise<void>;
}

export function ConversationProjectDialog({
  open,
  onOpenChange,
  mode,
  initialName = "",
  onSubmit
}: ConversationProjectDialogProps) {
  const [name, setName] = useState(initialName);
  const [error, setError] = useState<string | null>(null);
  const [isLoading, setIsLoading] = useState(false);

  useEffect(() => {
    if (open) {
      setName(initialName);
    }
  }, [open, initialName]);

  useEffect(() => {
    if (!open) {
      setName(initialName);
      setError(null);
      setIsLoading(false);
    }
  }, [open, initialName]);

  const isRename = mode === "rename";
  const trimmedInitialName = initialName.trim();

  async function handleSubmit(event: React.FormEvent) {
    event.preventDefault();
    setError(null);

    const trimmedName = name.trim();
    if (!trimmedName) {
      setError("Project name cannot be empty.");
      return;
    }

    if (isRename && trimmedName === trimmedInitialName) {
      setError("Please enter a different project name.");
      return;
    }

    setIsLoading(true);
    try {
      await onSubmit(trimmedName);
      onOpenChange(false);
    } catch (submitError) {
      console.error("Failed to save conversation project:", submitError);
      const errorMessage =
        submitError instanceof Error && submitError.message
          ? `Failed to save project: ${submitError.message}`
          : "Failed to save project. Please try again.";
      setError(errorMessage);
    } finally {
      setIsLoading(false);
    }
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-[425px]">
        <DialogHeader>
          <DialogTitle>{isRename ? "Rename Project" : "Create Project"}</DialogTitle>
          <DialogDescription>
            {isRename
              ? "Choose a new name for this project."
              : "Create a project to organize chats and store project-specific instructions."}
          </DialogDescription>
        </DialogHeader>
        <form onSubmit={handleSubmit} className="grid gap-4 py-4">
          {error && (
            <Alert variant="destructive">
              <AlertDescription>{error}</AlertDescription>
            </Alert>
          )}
          <div className="grid gap-2">
            <Label htmlFor="conversation-project-name">Project name</Label>
            <Input
              id="conversation-project-name"
              type="text"
              value={name}
              onChange={(event) => setName(event.target.value)}
              autoFocus
              required
            />
          </div>
          <DialogFooter>
            <Button type="button" variant="outline" onClick={() => onOpenChange(false)}>
              Cancel
            </Button>
            <Button type="submit" disabled={isLoading}>
              {isLoading
                ? isRename
                  ? "Renaming..."
                  : "Creating..."
                : isRename
                  ? "Rename Project"
                  : "Create Project"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
