import { useState, useEffect } from "react";
import { Dialog, DialogContent } from "@/components/ui/dialog";
import { TeamSetupDialog } from "./TeamSetupDialog";
import { TeamDashboard } from "./TeamDashboard";
import type { TeamStatus } from "@/types/team";

interface TeamManagementDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  teamStatus?: TeamStatus;
}

export function TeamManagementDialog({
  open,
  onOpenChange,
  teamStatus
}: TeamManagementDialogProps) {
  const [showSetupDialog, setShowSetupDialog] = useState(false);

  // Check if team needs to be set up when dialog opens
  useEffect(() => {
    if (open && teamStatus?.has_team_subscription && !teamStatus?.team_created) {
      setShowSetupDialog(true);
    }
  }, [open, teamStatus]);

  const handleTeamCreated = () => {
    setShowSetupDialog(false);
    // Force a small delay to allow the team status to update
    // This ensures the dashboard shows up instead of closing the dialog
    setTimeout(() => {
      // The query invalidation in TeamSetupDialog will cause a refetch
      // and the dashboard will automatically appear
    }, 100);
  };

  // If team setup is needed, show setup dialog instead
  if (showSetupDialog || (teamStatus?.has_team_subscription && !teamStatus?.team_created)) {
    return (
      <TeamSetupDialog
        open={open}
        onOpenChange={onOpenChange}
        teamStatus={teamStatus}
        onTeamCreated={handleTeamCreated}
      />
    );
  }

  // Otherwise show the team dashboard
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-lg max-w-[calc(100vw-2rem)] max-h-[90vh] overflow-y-auto">
        <TeamDashboard teamStatus={teamStatus} />
      </DialogContent>
    </Dialog>
  );
}
