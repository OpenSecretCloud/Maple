import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { AlertCircle, MessageCircle } from "lucide-react";

interface ContextLimitDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  currentModel?: string;
  hasDocument?: boolean;
}

export function ContextLimitDialog({ open, onOpenChange, hasDocument }: ContextLimitDialogProps) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <div className="flex items-center gap-3 mb-2">
            <div className="p-2 rounded-lg bg-orange-500/10 text-orange-500">
              <AlertCircle className="h-8 w-8" />
            </div>
            <DialogTitle>Message Too Large</DialogTitle>
          </div>
          <DialogDescription className="text-base">
            Your message exceeds the context limit for the current model.
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-4 py-4">
          <div className="space-y-3">
            <p className="text-sm font-medium">Here's what you can try:</p>
            <ul className="space-y-2 text-sm text-muted-foreground">
              <li className="flex items-start gap-2">
                <MessageCircle className="h-4 w-4 mt-0.5 shrink-0" />
                <span>
                  <strong>Shorten your message</strong> - Try reducing the amount of text or content
                </span>
              </li>
              {hasDocument && (
                <li className="flex items-start gap-2">
                  <MessageCircle className="h-4 w-4 mt-0.5 shrink-0" />
                  <span>
                    <strong>Use a smaller document</strong> - Try uploading a shorter document or
                    extracting only the relevant sections
                  </span>
                </li>
              )}
            </ul>
          </div>
        </div>

        <DialogFooter>
          <Button onClick={() => onOpenChange(false)}>Got it</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
