import { useState } from "react";
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
import { AlertTriangle, Copy, CheckCircle } from "lucide-react";
import { Checkbox } from "@/components/ui/checkbox";

interface GuestCredentialsDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  uuid: string;
  onContinue: () => void;
}

export function GuestCredentialsDialog({
  open,
  onOpenChange,
  uuid,
  onContinue
}: GuestCredentialsDialogProps) {
  const [hasBackedUp, setHasBackedUp] = useState(false);
  const [uuidCopied, setUuidCopied] = useState(false);

  const handleCopyUuid = async () => {
    try {
      await navigator.clipboard.writeText(uuid);
      setUuidCopied(true);
      setTimeout(() => setUuidCopied(false), 2000);
    } catch (error) {
      console.error("Failed to copy UUID:", error);
    }
  };

  const handleContinue = () => {
    if (hasBackedUp) {
      setHasBackedUp(false);
      setUuidCopied(false);
      onContinue();
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        className="sm:max-w-[500px] [&>button]:hidden"
        onInteractOutside={(e) => e.preventDefault()}
        onEscapeKeyDown={(e) => e.preventDefault()}
      >
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2 text-amber-600 dark:text-amber-500">
            <AlertTriangle className="w-5 h-5" />
            Save Your Anonymous Account ID
          </DialogTitle>
          <DialogDescription className="text-base font-medium">
            This is your ONLY chance to see your Account ID. Save it now!
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-4 py-4">
          {/* Critical Warning Banner */}
          <div className="rounded-lg border-2 border-red-500 bg-red-500/10 p-4">
            <div className="flex items-start gap-3">
              <AlertTriangle className="w-5 h-5 text-red-500 flex-shrink-0 mt-0.5" />
              <div className="space-y-2 text-sm">
                <p className="font-semibold text-red-600 dark:text-red-500">
                  Critical: Save your Account ID immediately!
                </p>
                <p>
                  Your Account ID will <strong>never be shown again</strong> after you close this
                  dialog. If you lose it, you will <strong>permanently lose access</strong> to your
                  account. We cannot recover it for you.
                </p>
              </div>
            </div>
          </div>

          {/* UUID Display */}
          <div className="space-y-2">
            <Label htmlFor="uuid" className="text-base font-medium">
              Your Account ID
            </Label>
            <div className="flex gap-2">
              <Input id="uuid" value={uuid} readOnly className="font-mono text-sm" />
              <Button
                type="button"
                variant="outline"
                size="icon"
                onClick={handleCopyUuid}
                className="flex-shrink-0"
              >
                {uuidCopied ? (
                  <CheckCircle className="w-4 h-4 text-green-500" />
                ) : (
                  <Copy className="w-4 h-4" />
                )}
              </Button>
            </div>
            <p className="text-xs text-muted-foreground">
              You will need this Account ID along with your password to sign in.
            </p>
          </div>

          {/* Backup Instructions */}
          <div className="rounded-lg border border-border bg-muted/50 p-4 space-y-2">
            <p className="text-sm font-medium">How to save your Account ID:</p>
            <ul className="text-sm space-y-1 list-disc list-inside text-muted-foreground">
              <li>Copy it to a password manager (recommended)</li>
              <li>Write it down on paper and store it safely</li>
              <li>Save it in a secure note or encrypted file</li>
              <li>Take a screenshot and store it securely</li>
            </ul>
          </div>

          {/* Confirmation Checkbox */}
          <div className="flex items-start space-x-3 pt-2">
            <Checkbox
              id="backed-up"
              checked={hasBackedUp}
              onCheckedChange={(checked: boolean) => setHasBackedUp(checked === true)}
            />
            <Label
              htmlFor="backed-up"
              className="text-sm font-medium leading-relaxed cursor-pointer"
            >
              I have securely saved my Account ID and understand I cannot recover my account without
              it
            </Label>
          </div>
        </div>

        <DialogFooter>
          <Button onClick={handleContinue} disabled={!hasBackedUp} className="w-full sm:w-auto">
            Continue to Payment
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
