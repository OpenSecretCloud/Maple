import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { FileText, Smartphone, Monitor, Check, Sparkles } from "lucide-react";
import { useNavigate } from "@tanstack/react-router";

interface DocumentPlatformDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  hasProAccess: boolean;
}

export function DocumentPlatformDialog({
  open,
  onOpenChange,
  hasProAccess
}: DocumentPlatformDialogProps) {
  const navigate = useNavigate();

  const handleViewPlans = () => {
    onOpenChange(false);
    navigate({ to: "/pricing" });
  };

  const handleGetApps = () => {
    onOpenChange(false);
    navigate({ to: "/downloads" });
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <div className="flex items-center gap-3 mb-2">
            <div className="p-2 rounded-lg bg-primary/10 text-primary">
              <FileText className="h-8 w-8" />
            </div>
            <DialogTitle>Document Upload on Mobile & Desktop</DialogTitle>
          </div>
          <DialogDescription className="text-base">
            {hasProAccess
              ? "You have access to document upload! Download our apps to use this feature."
              : "Document upload is available on our mobile and desktop apps with Pro plan and above."}
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-4 py-4">
          <div className="grid grid-cols-2 gap-4">
            <div className="flex flex-col items-center p-4 rounded-lg border bg-card">
              <Monitor className="h-8 w-8 mb-2 text-muted-foreground" />
              <span className="font-medium">Desktop</span>
              <span className="text-xs text-muted-foreground">macOS • Linux</span>
            </div>
            <div className="flex flex-col items-center p-4 rounded-lg border bg-card">
              <Smartphone className="h-8 w-8 mb-2 text-muted-foreground" />
              <span className="font-medium">Mobile</span>
              <span className="text-xs text-muted-foreground">iOS • Android (beta)</span>
            </div>
          </div>

          <div className="space-y-2">
            <p className="text-sm font-medium">Features include:</p>
            <ul className="space-y-2">
              <li className="flex items-start gap-2 text-sm">
                <Check className="h-4 w-4 text-green-500 mt-0.5 shrink-0" />
                <span>Local PDF processing - files are processed on your device</span>
              </li>
              <li className="flex items-start gap-2 text-sm">
                <Check className="h-4 w-4 text-green-500 mt-0.5 shrink-0" />
                <span>Support for PDF, TXT, and Markdown files</span>
              </li>
              <li className="flex items-start gap-2 text-sm">
                <Check className="h-4 w-4 text-green-500 mt-0.5 shrink-0" />
                <span>Extracted text is secured with end-to-end encryption</span>
              </li>
              <li className="flex items-start gap-2 text-sm">
                <Check className="h-4 w-4 text-green-500 mt-0.5 shrink-0" />
                <span>Files up to 10MB supported</span>
              </li>
            </ul>
          </div>

          {!hasProAccess && (
            <div className="pt-2 border-t">
              <p className="text-sm text-muted-foreground">
                Upgrade to Pro to unlock document upload along with voice recording, powerful
                models, and more.
              </p>
            </div>
          )}
        </div>

        <DialogFooter className="gap-2 sm:gap-0">
          {hasProAccess ? (
            <>
              <Button variant="outline" onClick={() => onOpenChange(false)}>
                Close
              </Button>
              <Button onClick={handleGetApps} className="gap-2">
                <Smartphone className="h-4 w-4" />
                Get Apps
              </Button>
            </>
          ) : (
            <>
              <Button variant="outline" onClick={() => onOpenChange(false)}>
                Maybe Later
              </Button>
              <Button onClick={handleViewPlans} className="gap-2">
                <Sparkles className="h-4 w-4" />
                View Plans
              </Button>
            </>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
