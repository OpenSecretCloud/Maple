import { useRef, useState, type FormEvent } from "react";

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
import { validateAgentTaskRename } from "@/components/agent/agentTaskRename";

interface RenameAgentTaskDialogProps {
  currentTitle: string;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onRename: (title: string) => Promise<void>;
  onReturnFocus: (focusVisible: boolean) => void;
}

export function RenameAgentTaskDialog({
  currentTitle,
  open,
  onOpenChange,
  onRename,
  onReturnFocus
}: RenameAgentTaskDialogProps) {
  const [title, setTitle] = useState(currentTitle);
  const [error, setError] = useState<string | null>(null);
  const [isPending, setIsPending] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);
  const isPendingRef = useRef(false);
  const returnFocusVisibleRef = useRef(true);

  const handleOpenChange = (nextOpen: boolean) => {
    if (!nextOpen && isPendingRef.current) return;
    onOpenChange(nextOpen);
  };

  const handleSubmit = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (isPendingRef.current) return;

    const validation = validateAgentTaskRename(title, currentTitle);
    if (!validation.ok) {
      setError(validation.error);
      inputRef.current?.focus();
      return;
    }

    setError(null);
    isPendingRef.current = true;
    inputRef.current?.focus();
    setIsPending(true);
    try {
      await onRename(validation.title);
      isPendingRef.current = false;
      setIsPending(false);
      onOpenChange(false);
    } catch (renameError) {
      isPendingRef.current = false;
      setIsPending(false);
      const message =
        renameError instanceof Error
          ? renameError.message
          : typeof renameError === "string"
            ? renameError
            : "";
      setError(
        message.startsWith("Agent task title") ? message : "Couldn’t rename task. Please try again."
      );
      inputRef.current?.focus();
    }
  };

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      <DialogContent
        className={isPending ? "sm:max-w-[425px] [&>button]:hidden" : "sm:max-w-[425px]"}
        onCloseAutoFocus={(event) => {
          event.preventDefault();
          onReturnFocus(returnFocusVisibleRef.current);
        }}
        onPointerDownCapture={() => {
          returnFocusVisibleRef.current = false;
        }}
        onKeyDownCapture={() => {
          returnFocusVisibleRef.current = true;
        }}
        onEscapeKeyDown={(event) => {
          returnFocusVisibleRef.current = true;
          if (isPendingRef.current) event.preventDefault();
        }}
        onInteractOutside={(event) => {
          returnFocusVisibleRef.current = false;
          if (isPendingRef.current) event.preventDefault();
        }}
      >
        <DialogHeader>
          <DialogTitle>Rename Task</DialogTitle>
          <DialogDescription>Enter a new name for this task.</DialogDescription>
        </DialogHeader>
        <form onSubmit={handleSubmit} className="grid gap-4 py-4" aria-busy={isPending}>
          <div className="grid gap-2">
            <Label htmlFor="agent-task-title">Task Title</Label>
            <Input
              ref={inputRef}
              id="agent-task-title"
              value={title}
              onChange={(event) => {
                setTitle(event.target.value);
                if (error) setError(null);
              }}
              aria-invalid={error ? true : undefined}
              aria-describedby={error ? "agent-task-title-error" : undefined}
              autoFocus
              readOnly={isPending}
            />
            {error ? (
              <p id="agent-task-title-error" className="text-sm text-destructive" role="alert">
                {error}
              </p>
            ) : null}
          </div>
          <DialogFooter>
            <Button
              type="button"
              variant="outline"
              onClick={() => handleOpenChange(false)}
              disabled={isPending}
            >
              Cancel
            </Button>
            <Button type="submit" disabled={isPending}>
              {isPending ? "Renaming…" : "Rename Task"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
