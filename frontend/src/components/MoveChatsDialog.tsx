import { useEffect, useState } from "react";
import { Check, Folder, FolderOpen } from "lucide-react";
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
import type { ConversationProjectListItem } from "@opensecret/react";
import { cn } from "@/utils/utils";

interface MoveChatsDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  count: number;
  projects: ConversationProjectListItem[];
  onConfirm: (projectId: string | null) => Promise<void>;
  isMoving?: boolean;
}

export function MoveChatsDialog({
  open,
  onOpenChange,
  count,
  projects,
  onConfirm,
  isMoving = false
}: MoveChatsDialogProps) {
  const [selectedProjectId, setSelectedProjectId] = useState<string | null | undefined>(undefined);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setSelectedProjectId(undefined);
    setError(null);
  }, [open]);

  async function handleConfirm() {
    if (selectedProjectId === undefined) {
      return;
    }

    setError(null);

    try {
      await onConfirm(selectedProjectId);
      onOpenChange(false);
    } catch (error) {
      console.error("Failed to move chats:", error);
      setError("Failed to move selected chats. Please try again.");
    }
  }

  function renderOption(
    key: string,
    label: string,
    value: string | null,
    icon: React.ReactNode,
    description?: string
  ) {
    const isSelected = selectedProjectId === value;

    return (
      <button
        key={key}
        type="button"
        onClick={() => {
          setSelectedProjectId(value);
          setError(null);
        }}
        disabled={isMoving}
        className={cn(
          "flex w-full items-center gap-3 rounded-lg border px-3 py-3 text-left transition-colors disabled:cursor-not-allowed disabled:opacity-50",
          isSelected ? "border-primary bg-primary/5 text-foreground" : "hover:bg-accent"
        )}
      >
        <div className="flex h-8 w-8 items-center justify-center rounded-md bg-muted text-muted-foreground">
          {icon}
        </div>
        <div className="min-w-0 flex-1">
          <div className="truncate font-medium">{label}</div>
          {description ? <div className="text-xs text-muted-foreground">{description}</div> : null}
        </div>
        {isSelected ? <Check className="h-4 w-4 text-primary" /> : null}
      </button>
    );
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        className="sm:max-w-[425px]"
        onInteractOutside={(event) => {
          if (isMoving) {
            event.preventDefault();
          }
        }}
        onEscapeKeyDown={(event) => {
          if (isMoving) {
            event.preventDefault();
          }
        }}
      >
        <DialogHeader>
          <DialogTitle>
            Move {count} chat{count === 1 ? "" : "s"}
          </DialogTitle>
          <DialogDescription>Choose where the selected chats should live.</DialogDescription>
        </DialogHeader>
        <div className="grid gap-2 py-4">
          {error ? (
            <Alert variant="destructive">
              <AlertDescription>{error}</AlertDescription>
            </Alert>
          ) : null}
          {renderOption("no-project", "No project", null, <FolderOpen className="h-4 w-4" />)}
          {projects.map((project) =>
            renderOption(project.id, project.name, project.id, <Folder className="h-4 w-4" />)
          )}
        </div>
        <DialogFooter>
          <Button
            type="button"
            variant="outline"
            onClick={() => onOpenChange(false)}
            disabled={isMoving}
          >
            Cancel
          </Button>
          <Button
            type="button"
            onClick={handleConfirm}
            disabled={selectedProjectId === undefined || isMoving}
          >
            {isMoving ? "Moving..." : "Move Chats"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
