import {
  AlertDialog,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle
} from "@/components/ui/alert-dialog";
import { useEffect, useState } from "react";
import { AlertDestructive } from "@/components/AlertDestructive";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";

interface DeleteConversationProjectDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onConfirm: () => Promise<void> | void;
  projectName: string;
}

export function DeleteConversationProjectDialog({
  open,
  onOpenChange,
  onConfirm,
  projectName
}: DeleteConversationProjectDialogProps) {
  const [confirmText, setConfirmText] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [isDeleting, setIsDeleting] = useState(false);

  useEffect(() => {
    if (open) {
      setConfirmText("");
      setError(null);
      setIsDeleting(false);
    }
  }, [open]);

  async function handleConfirm(event: React.FormEvent) {
    event.preventDefault();
    if (confirmText !== "DELETE") {
      setError("Please type DELETE to confirm.");
      return;
    }

    setIsDeleting(true);
    setError(null);

    try {
      await onConfirm();
      onOpenChange(false);
    } catch (error) {
      console.error("Failed to delete project:", error);
      setError("Failed to delete project. Please try again.");
    } finally {
      setIsDeleting(false);
    }
  }

  return (
    <AlertDialog open={open} onOpenChange={onOpenChange}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>Delete this project?</AlertDialogTitle>
          <AlertDialogDescription>
            {`This will permanently delete "${projectName}" and any chats in it. Move chats out first if you want to keep them. This action cannot be undone.`}
          </AlertDialogDescription>
        </AlertDialogHeader>

        <form onSubmit={handleConfirm} className="space-y-4">
          <div className="rounded-md bg-destructive/10 p-3 text-sm text-destructive">
            Type `DELETE` to confirm deleting this project and all chats in it.
          </div>

          <div className="space-y-2">
            <Label htmlFor="delete-project-confirm-text">Type DELETE to confirm</Label>
            <Input
              id="delete-project-confirm-text"
              value={confirmText}
              onChange={(event) => {
                setConfirmText(event.target.value);
                if (error) {
                  setError(null);
                }
              }}
              placeholder="DELETE"
              disabled={isDeleting}
              autoCapitalize="characters"
              autoCorrect="off"
              spellCheck={false}
            />
          </div>

          {error ? <AlertDestructive description={error} /> : null}

          <AlertDialogFooter>
            <AlertDialogCancel disabled={isDeleting}>Cancel</AlertDialogCancel>
            <Button
              type="submit"
              variant="destructive"
              disabled={isDeleting || confirmText !== "DELETE"}
            >
              {isDeleting ? "Deleting..." : "Delete Project"}
            </Button>
          </AlertDialogFooter>
        </form>
      </AlertDialogContent>
    </AlertDialog>
  );
}
