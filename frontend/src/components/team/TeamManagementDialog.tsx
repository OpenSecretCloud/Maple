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

  // Determine if we should show the setup dialog
  const needsSetup = teamStatus?.has_team_subscription && !teamStatus?.team_created;

  // Check if team needs to be set up when dialog opens
  useEffect(() => {
    if (open && needsSetup) {
      setShowSetupDialog(true);
    } else if (!needsSetup) {
      // Reset state when team is created
      setShowSetupDialog(false);
    }
  }, [open, needsSetup]);

  const handleTeamCreated = () => {
    // Don't update state here - let the effect handle it when teamStatus updates
    // This prevents race conditions without setTimeout
  };

  // Show setup dialog if explicitly set or if team needs setup
  if (showSetupDialog && needsSetup) {
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
