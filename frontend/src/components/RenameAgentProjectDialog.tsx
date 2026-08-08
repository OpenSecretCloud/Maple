import { useState, type FormEvent } from "react";

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

interface RenameAgentProjectDialogProps {
  currentDisplayName: string;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onRename: (displayName: string) => void;
}

export function RenameAgentProjectDialog({
  currentDisplayName,
  open,
  onOpenChange,
  onRename
}: RenameAgentProjectDialogProps) {
  const [displayName, setDisplayName] = useState(currentDisplayName);
  const [error, setError] = useState<string | null>(null);

  const handleSubmit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const trimmedDisplayName = displayName.trim();
    if (!trimmedDisplayName) {
      setError("Project name cannot be empty.");
      return;
    }

    onRename(trimmedDisplayName);
    onOpenChange(false);
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-[425px]">
        <DialogHeader>
          <DialogTitle>Rename Project</DialogTitle>
          <DialogDescription>
            Change this project&apos;s display name in Maple. The folder on your computer will not
            be renamed or moved.
          </DialogDescription>
        </DialogHeader>
        <form onSubmit={handleSubmit} className="grid gap-4 py-4">
          <div className="grid gap-2">
            <Label htmlFor="agent-project-display-name">Project Name</Label>
            <Input
              id="agent-project-display-name"
              value={displayName}
              onChange={(event) => {
                setDisplayName(event.target.value);
                if (error) setError(null);
              }}
              aria-invalid={error ? true : undefined}
              aria-describedby={error ? "agent-project-display-name-error" : undefined}
              autoFocus
            />
            {error ? (
              <p
                id="agent-project-display-name-error"
                className="text-sm text-destructive"
                role="alert"
              >
                {error}
              </p>
            ) : null}
          </div>
          <DialogFooter>
            <Button type="button" variant="outline" onClick={() => onOpenChange(false)}>
              Cancel
            </Button>
            <Button type="submit">Rename Project</Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
