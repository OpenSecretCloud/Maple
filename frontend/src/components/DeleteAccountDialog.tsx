import { useRef, useState } from "react";
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
import { useQueryClient } from "@tanstack/react-query";
import { generateSecureSecret, hashSecret, useOpenSecret } from "@opensecret/react";
import { getBillingService } from "@/billing/billingService";
import { Loader2 } from "lucide-react";
import { clearAgentDataForUser } from "@/services/agentRuntimeService";

interface DeleteAccountDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

export function DeleteAccountDialog({ open, onOpenChange }: DeleteAccountDialogProps) {
  const os = useOpenSecret();
  const queryClient = useQueryClient();
  const [step, setStep] = useState<"request" | "confirm">("request");
  const [uuid, setUuid] = useState("");
  const [secret, setSecret] = useState("");
  const [isLoading, setIsLoading] = useState(false);
  const [isAccountDeleted, setIsAccountDeleted] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [confirmText, setConfirmText] = useState("");
  const cleanupBlockRef = useRef<Awaited<ReturnType<typeof clearAgentDataForUser>> | null>(null);

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
    let deletionConfirmed = isAccountDeleted;
    let agentDataCleared = cleanupBlockRef.current !== null;
    let proxyReset = false;

    try {
      const userId = os.auth.user?.user.id;

      if (!cleanupBlockRef.current) {
        // Local Agent data is cleared before the irreversible remote action.
        // The returned block stays held until the account flow either succeeds
        // or fails while the user still owns the account.
        cleanupBlockRef.current = await clearAgentDataForUser(userId);
        agentDataCleared = true;
      }

      // Credential reset is also required before remote deletion so a crash
      // cannot leave a deleted account's proxy key on disk.
      const { proxyService } = await import("@/services/proxyService");
      await proxyService.stopAndResetProxy(userId, os.deleteApiKey);
      proxyReset = true;

      if (!deletionConfirmed) {
        await os.confirmAccountDeletion(uuid, secret);
        deletionConfirmed = true;
        setIsAccountDeleted(true);
        cleanupBlockRef.current.retainUntilNextSession();
      }

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
      queryClient.clear();

      // Force page refresh to go to logged out state
      window.location.href = "/";
    } catch (err) {
      // If remote deletion did not happen, the authenticated user remains and
      // must be able to start a fresh Agent runtime after this attempt.
      if (!deletionConfirmed) {
        cleanupBlockRef.current?.release();
        cleanupBlockRef.current = null;
      }
      setError(
        !agentDataCleared
          ? "Maple couldn't safely stop and clear local Agent Mode data. Your account was not deleted; please retry."
          : !proxyReset
            ? "Local Agent Mode history was cleared, but Maple couldn't reset its proxy credentials. Your account was not deleted; please retry."
            : !deletionConfirmed
              ? "Local Agent Mode history was cleared, but account deletion was not confirmed. Verify the code and retry if you still want to delete your account."
              : "Your account data was deleted, but Maple couldn't finish signing out. Please retry."
      );
      console.error(err);
      setIsLoading(false);
    }
  };

  const handleCancel = () => {
    if (isAccountDeleted) return;
    setStep("request");
    setError(null);
    setConfirmText("");
    setUuid("");
    onOpenChange(false);
  };

  return (
    <AlertDialog
      open={open}
      onOpenChange={(nextOpen) => {
        if (!nextOpen && isAccountDeleted) return;
        onOpenChange(nextOpen);
      }}
    >
      <AlertDialogContent className="max-w-md">
        <AlertDialogHeader>
          <AlertDialogTitle>
            {step === "request" ? "Delete Account" : "Confirm Account Deletion"}
          </AlertDialogTitle>
          <AlertDialogDescription>
            {step === "request"
              ? "This action cannot be undone. This will permanently delete your account and all your data."
              : "Please check your email for a confirmation code. Submitting it clears local Agent Mode history and proxy credentials before the final account deletion request, even if the code is rejected."}
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
          <AlertDialogCancel onClick={handleCancel} disabled={isLoading || isAccountDeleted}>
            Cancel
          </AlertDialogCancel>
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
              ) : isAccountDeleted ? (
                "Finish Cleanup"
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
