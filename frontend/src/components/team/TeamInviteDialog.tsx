import { useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Loader2, AlertCircle, UserPlus, Info, CreditCard } from "lucide-react";
import { getBillingService } from "@/billing/billingService";
import { useLocalState } from "@/state/useLocalState";
import { isTauri } from "@tauri-apps/api/core";
import { type } from "@tauri-apps/plugin-os";
import type { TeamStatus } from "@/types/team";

interface TeamInviteDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  teamStatus?: TeamStatus;
}

export function TeamInviteDialog({ open, onOpenChange, teamStatus }: TeamInviteDialogProps) {
  const [emails, setEmails] = useState("");
  const [isInviting, setIsInviting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [successMessage, setSuccessMessage] = useState<string | null>(null);
  const [isPortalLoading, setIsPortalLoading] = useState(false);
  const queryClient = useQueryClient();
  const { billingStatus } = useLocalState();

  const seatsAvailable = teamStatus?.seats_available || 0;
  const hasStripeAccount = billingStatus?.stripe_customer_id !== null;

  const handleManageSubscription = async () => {
    if (!hasStripeAccount) return;

    try {
      setIsPortalLoading(true);
      const billingService = getBillingService();
      const url = await billingService.getPortalUrl();

      // Check if we're in a Tauri environment on iOS
      try {
        const isTauriEnv = await isTauri();
        if (isTauriEnv) {
          const platform = await type();
          if (platform === "ios") {
            // For iOS, use the opener plugin
            const { invoke } = await import("@tauri-apps/api/core");
            await invoke("plugin:opener|open_url", { url });
            return;
          }
        }
      } catch {
        // Not in Tauri or error checking, continue with web flow
      }

      // Web or desktop flow
      window.open(url, "_blank", "noopener,noreferrer");
    } catch (error) {
      console.error("Failed to open billing portal:", error);
    } finally {
      setIsPortalLoading(false);
    }
  };

  const handleInvite = async (e: React.FormEvent) => {
    e.preventDefault();

    const emailList = emails
      .split(/[\n,]/)
      .map((email) => email.trim())
      .filter((email) => email.length > 0);

    if (emailList.length === 0) {
      setError("Please enter at least one email address");
      return;
    }

    // Validate email format
    const emailRegex = /^[^\s@]+@[^\s@]+\.[^\s@]+$/;
    const invalidEmails = emailList.filter((email) => !emailRegex.test(email));
    if (invalidEmails.length > 0) {
      setError(`Invalid email format: ${invalidEmails.join(", ")}`);
      return;
    }

    // Check seat availability
    if (emailList.length > seatsAvailable) {
      setError(
        `Cannot invite ${emailList.length} members. Only ${seatsAvailable} ${
          seatsAvailable === 1 ? "seat is" : "seats are"
        } available.`
      );
      return;
    }

    setIsInviting(true);
    setError(null);
    setSuccessMessage(null);

    try {
      const billingService = getBillingService();
      const response = await billingService.inviteTeamMembers({ emails: emailList });

      // Invalidate team status and members queries
      await queryClient.invalidateQueries({ queryKey: ["teamStatus"] });
      await queryClient.invalidateQueries({ queryKey: ["teamMembers"] });

      const inviteCount = response.invites.length;
      setSuccessMessage(
        `Successfully sent ${inviteCount} ${inviteCount === 1 ? "invite" : "invites"}`
      );

      // Clear the form
      setEmails("");
    } catch (err) {
      console.error("Failed to invite members:", err);
      setError(err instanceof Error ? err.message : "Failed to send invites");
    } finally {
      setIsInviting(false);
    }
  };

  const handleOpenChange = (newOpen: boolean) => {
    if (!isInviting) {
      if (newOpen && !open) {
        // Reset form when opening
        setEmails("");
        setError(null);
        setSuccessMessage(null);
      } else if (!newOpen && open) {
        // When closing, delay clearing success message to prevent flash
        onOpenChange(newOpen);
        setTimeout(() => {
          setEmails("");
          setError(null);
          setSuccessMessage(null);
        }, 300); // Wait for dialog close animation
        return;
      }
      onOpenChange(newOpen);
    }
  };

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      <DialogContent className="sm:max-w-[500px]">
        <DialogHeader>
          <DialogTitle>Invite Team Members</DialogTitle>
          <DialogDescription>
            Send invitations to new team members. They'll receive an email to join your team.
          </DialogDescription>
        </DialogHeader>

        <form onSubmit={handleInvite}>
          <div className="grid gap-4 py-4">
            <div className="grid gap-2">
              <Label htmlFor="emails">Email Addresses</Label>
              <Textarea
                id="emails"
                value={emails}
                onChange={(e) => setEmails(e.target.value)}
                placeholder={`john@example.com
jane@example.com
or comma-separated: john@example.com, jane@example.com`}
                disabled={isInviting}
                rows={5}
                className="resize-none"
              />
              <p className="text-sm text-muted-foreground">
                Enter one email per line or separate with commas
              </p>
            </div>

            {seatsAvailable > 0 && (
              <Alert>
                <Info className="h-4 w-4" />
                <AlertDescription>
                  You have <strong>{seatsAvailable}</strong>{" "}
                  {seatsAvailable === 1 ? "seat" : "seats"} available for new members.
                </AlertDescription>
              </Alert>
            )}

            {seatsAvailable === 0 && !successMessage && !isInviting && (
              <>
                <Alert variant="destructive">
                  <AlertCircle className="h-4 w-4" />
                  <AlertDescription>
                    No seats available. Please purchase additional seats or remove existing members
                    before inviting new ones.
                  </AlertDescription>
                </Alert>
                {hasStripeAccount && (
                  <Button
                    variant="outline"
                    size="sm"
                    className="w-full"
                    onClick={handleManageSubscription}
                    disabled={isPortalLoading}
                  >
                    <CreditCard className="mr-2 h-4 w-4" />
                    {isPortalLoading ? "Loading..." : "Manage Subscription"}
                  </Button>
                )}
              </>
            )}

            {error && (
              <Alert variant="destructive">
                <AlertCircle className="h-4 w-4" />
                <AlertDescription>{error}</AlertDescription>
              </Alert>
            )}

            {successMessage && (
              <Alert className="border-green-200 bg-green-50 dark:border-green-800 dark:bg-green-950">
                <UserPlus className="h-4 w-4 text-green-600 dark:text-green-400" />
                <AlertDescription className="text-green-800 dark:text-green-200">
                  {successMessage}
                </AlertDescription>
              </Alert>
            )}
          </div>

          <DialogFooter>
            <Button
              type="button"
              variant="outline"
              onClick={() => handleOpenChange(false)}
              disabled={isInviting}
            >
              Cancel
            </Button>
            <Button type="submit" disabled={isInviting || seatsAvailable === 0}>
              {isInviting ? (
                <>
                  <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                  Sending Invites...
                </>
              ) : (
                <>
                  <UserPlus className="mr-2 h-4 w-4" />
                  Send Invites
                </>
              )}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
