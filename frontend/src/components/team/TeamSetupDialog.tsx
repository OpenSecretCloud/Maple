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
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Loader2, AlertCircle } from "lucide-react";
import { getBillingService } from "@/billing/billingService";
import type { TeamStatus } from "@/types/team";

interface TeamSetupDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  teamStatus?: TeamStatus;
  onTeamCreated?: () => void;
}

export function TeamSetupDialog({
  open,
  onOpenChange,
  teamStatus,
  onTeamCreated
}: TeamSetupDialogProps) {
  const [teamName, setTeamName] = useState("");
  const [isCreating, setIsCreating] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const queryClient = useQueryClient();

  // Don't show dialog if team is already created
  if (teamStatus?.team_created) {
    return null;
  }

  const handleCreateTeam = async (e: React.FormEvent) => {
    e.preventDefault();

    if (!teamName.trim()) {
      setError("Please enter a team name");
      return;
    }

    setIsCreating(true);
    setError(null);

    try {
      const billingService = getBillingService();
      await billingService.createTeam({ name: teamName.trim() });

      // Invalidate team status query to refetch
      await queryClient.invalidateQueries({ queryKey: ["teamStatus"] });

      // Call the success callback
      onTeamCreated?.();

      // Close the dialog
      onOpenChange(false);
    } catch (err) {
      console.error("Failed to create team:", err);
      setError(err instanceof Error ? err.message : "Failed to create team");
    } finally {
      setIsCreating(false);
    }
  };

  const handleOpenChange = (newOpen: boolean) => {
    // Don't allow closing while creating
    if (!isCreating) {
      onOpenChange(newOpen);
      // Reset form when closing
      if (!newOpen) {
        setTeamName("");
        setError(null);
      }
    }
  };

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      <DialogContent className="sm:max-w-[425px]">
        <DialogHeader>
          <DialogTitle>Set Up Your Team</DialogTitle>
          <DialogDescription>
            You've purchased a team plan! Let's create your team to get started. You'll be able to
            invite members after setup.
          </DialogDescription>
        </DialogHeader>

        <form onSubmit={handleCreateTeam}>
          <div className="grid gap-4 py-4">
            <div className="grid gap-2">
              <Label htmlFor="teamName">Team Name</Label>
              <Input
                id="teamName"
                value={teamName}
                onChange={(e) => setTeamName(e.target.value)}
                placeholder="e.g., My Company Team"
                disabled={isCreating}
                autoFocus
              />
              <p className="text-sm text-muted-foreground">
                This is how your team will be identified. You can change it later.
              </p>
            </div>

            {teamStatus?.seats_purchased && (
              <Alert>
                <AlertCircle className="h-4 w-4" />
                <AlertDescription>
                  Your team plan includes <strong>{teamStatus.seats_purchased} seats</strong>.
                  You'll be the team admin and can invite up to {teamStatus.seats_purchased - 1}{" "}
                  additional members.
                </AlertDescription>
              </Alert>
            )}

            {error && (
              <Alert variant="destructive">
                <AlertCircle className="h-4 w-4" />
                <AlertDescription>{error}</AlertDescription>
              </Alert>
            )}
          </div>

          <DialogFooter>
            <Button
              type="button"
              variant="outline"
              onClick={() => handleOpenChange(false)}
              disabled={isCreating}
            >
              Cancel
            </Button>
            <Button type="submit" disabled={isCreating}>
              {isCreating ? (
                <>
                  <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                  Creating Team...
                </>
              ) : (
                "Create Team"
              )}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
