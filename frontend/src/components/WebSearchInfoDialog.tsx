import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Globe, Check } from "lucide-react";

interface WebSearchInfoDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onConfirm: () => void;
}

export function WebSearchInfoDialog({ open, onOpenChange, onConfirm }: WebSearchInfoDialogProps) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <div className="flex items-center gap-3 mb-2">
            <div className="p-2 rounded-lg bg-blue-500/10 text-blue-500">
              <Globe className="h-8 w-8" />
            </div>
            <DialogTitle>Live Web Search</DialogTitle>
          </div>
          <DialogDescription className="text-base">
            When toggled on, Maple will automatically search the web when your question requires
            current or real-time information.
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-4 py-4">
          <div className="space-y-2">
            <p className="text-sm font-medium">What you get:</p>
            <ul className="space-y-2">
              <li className="flex items-start gap-2 text-sm">
                <Check className="h-4 w-4 text-blue-500 mt-0.5 shrink-0" />
                <span>Live web search powered by Brave</span>
              </li>
              <li className="flex items-start gap-2 text-sm">
                <Check className="h-4 w-4 text-blue-500 mt-0.5 shrink-0" />
                <span>Get up-to-date information from the internet</span>
              </li>
              <li className="flex items-start gap-2 text-sm">
                <Check className="h-4 w-4 text-blue-500 mt-0.5 shrink-0" />
                <span>Search queries are sent to Brave but not linked to your identity</span>
              </li>
              <li className="flex items-start gap-2 text-sm">
                <Check className="h-4 w-4 text-blue-500 mt-0.5 shrink-0" />
                <span>Results are processed privately and securely</span>
              </li>
              <li className="flex items-start gap-2 text-sm">
                <Check className="h-4 w-4 text-blue-500 mt-0.5 shrink-0" />
                <span>Perfect for current events, research, and fact-checking</span>
              </li>
            </ul>
          </div>

          <div className="pt-2 border-t">
            <p className="text-sm text-muted-foreground">
              Click the globe icon anytime to toggle web search on or off for your messages.
            </p>
          </div>
        </div>

        <DialogFooter>
          <Button onClick={onConfirm} className="w-full">
            Got it
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
