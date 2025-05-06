import { useState } from "react";
import { AlertDestructive } from "@/components/AlertDestructive";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  AlertDialog,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle
} from "@/components/ui/alert-dialog";
import { generateSecureSecret, hashSecret, useOpenSecret } from "@opensecret/react";
import { getBillingService } from "@/billing/billingService";
import { Loader2 } from "lucide-react";

interface DeleteAccountDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

export function DeleteAccountDialog({ open, onOpenChange }: DeleteAccountDialogProps) {
  const os = useOpenSecret();
  const [step, setStep] = useState<"request" | "confirm">("request");
  const [uuid, setUuid] = useState("");
  const [secret, setSecret] = useState("");
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [confirmText, setConfirmText] = useState("");

  // Initial request for account deletion
  const handleRequestDeletion = async () => {
    if (confirmText !== "DELETE") {
      setError("Please type DELETE to confirm");
      return;
    }

    setIsLoading(true);
    setError(null);

    try {
      // Generate a random secret and store it
      const generatedSecret = generateSecureSecret();
      const hashedSecret = await hashSecret(generatedSecret);

      // Request account deletion
      await os.requestAccountDeletion(hashedSecret);

      // Store the secret and move to confirmation step
      setSecret(generatedSecret);
      setStep("confirm");
    } catch (err) {
      setError("Failed to request account deletion. Please try again.");
      console.error(err);
    } finally {
      setIsLoading(false);
    }
  };

  // Confirm deletion with the UUID from email
  const handleConfirmDeletion = async () => {
    setIsLoading(true);
    setError(null);

    try {
      // Confirm account deletion with UUID and secret
      await os.confirmAccountDeletion(uuid, secret);

      // Clear all tokens and storage
      try {
        // Clear billing token
        getBillingService().clearToken();
      } catch (error) {
        console.error("Error clearing billing token:", error);
        // Fallback to direct session storage removal
        sessionStorage.removeItem("maple_billing_token");
      }

      // Sign out
      await os.signOut();

      // Force page refresh to go to logged out state
      window.location.href = "/";
    } catch (err) {
      setError("Failed to confirm account deletion. Please verify your code and try again.");
      console.error(err);
      setIsLoading(false);
    }
  };

  const handleCancel = () => {
    setStep("request");
    setError(null);
    setConfirmText("");
    setUuid("");
    onOpenChange(false);
  };

  return (
    <AlertDialog open={open} onOpenChange={onOpenChange}>
      <AlertDialogContent className="max-w-md">
        <AlertDialogHeader>
          <AlertDialogTitle>
            {step === "request" ? "Delete Account" : "Confirm Account Deletion"}
          </AlertDialogTitle>
          <AlertDialogDescription>
            {step === "request"
              ? "This action cannot be undone. This will permanently delete your account and all your data."
              : "Please check your email for a confirmation code and enter it below."}
          </AlertDialogDescription>
        </AlertDialogHeader>

        <div className="mt-4 space-y-4">
          {step === "request" && (
            <div className="space-y-4">
              <div className="rounded-md bg-destructive/10 p-3 text-sm text-destructive">
                <p>
                  Warning: This will permanently delete your account and all associated data. You
                  will lose access to any paid features, chat history, and settings.
                </p>
              </div>
              <div className="space-y-2">
                <Label htmlFor="confirm-text">Type DELETE to confirm</Label>
                <Input
                  id="confirm-text"
                  value={confirmText}
                  onChange={(e) => setConfirmText(e.target.value)}
                  className="w-full"
                />
              </div>
            </div>
          )}

          {step === "confirm" && (
            <div className="space-y-2">
              <Label htmlFor="confirmation-code">Confirmation Code</Label>
              <Input
                id="confirmation-code"
                value={uuid}
                onChange={(e) => setUuid(e.target.value)}
                placeholder="Enter code from email"
                className="w-full"
              />
            </div>
          )}

          {error && <AlertDestructive title="Error" description={error} />}
        </div>

        <AlertDialogFooter className="mt-4">
          <AlertDialogCancel onClick={handleCancel}>Cancel</AlertDialogCancel>
          {step === "request" ? (
            <Button
              variant="destructive"
              onClick={handleRequestDeletion}
              disabled={isLoading || confirmText !== "DELETE"}
            >
              {isLoading ? (
                <>
                  <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                  Processing...
                </>
              ) : (
                "Delete Account"
              )}
            </Button>
          ) : (
            <Button
              variant="destructive"
              onClick={handleConfirmDeletion}
              disabled={isLoading || !uuid}
            >
              {isLoading ? (
                <>
                  <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                  Processing...
                </>
              ) : (
                "Confirm Deletion"
              )}
            </Button>
          )}
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
