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
import { Loader2, AlertCircle, UserPlus, Info } from "lucide-react";
import { getBillingService } from "@/billing/billingService";
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
  const queryClient = useQueryClient();

  const seatsAvailable = teamStatus?.seats_available || 0;

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

      // Close dialog after a short delay
      setTimeout(() => {
        onOpenChange(false);
        setSuccessMessage(null);
      }, 2000);
    } catch (err) {
      console.error("Failed to invite members:", err);
      setError(err instanceof Error ? err.message : "Failed to send invites");
    } finally {
      setIsInviting(false);
    }
  };

  const handleOpenChange = (newOpen: boolean) => {
    if (!isInviting) {
      onOpenChange(newOpen);
      // Reset form when closing
      if (!newOpen) {
        setEmails("");
        setError(null);
        setSuccessMessage(null);
      }
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
                placeholder="john@example.com&#10;jane@example.com&#10;or comma-separated: john@example.com, jane@example.com"
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

            {seatsAvailable === 0 && !successMessage && (
              <Alert variant="destructive">
                <AlertCircle className="h-4 w-4" />
                <AlertDescription>
                  No seats available. Please purchase additional seats or remove existing members
                  before inviting new ones.
                </AlertDescription>
              </Alert>
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
