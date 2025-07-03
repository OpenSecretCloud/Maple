import { useState } from "react";
import { DialogHeader, DialogTitle, DialogDescription } from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { UserPlus, AlertTriangle, Crown, User } from "lucide-react";
import { TeamInviteDialog } from "./TeamInviteDialog";
import { TeamMembersList } from "./TeamMembersList";
import type { TeamStatus } from "@/types/team";

interface TeamDashboardProps {
  teamStatus?: TeamStatus;
}

export function TeamDashboard({ teamStatus }: TeamDashboardProps) {
  const [isInviteDialogOpen, setIsInviteDialogOpen] = useState(false);

  if (!teamStatus) {
    return (
      <>
        <DialogHeader>
          <DialogTitle>Team Dashboard</DialogTitle>
          <DialogDescription>Loading team information...</DialogDescription>
        </DialogHeader>
      </>
    );
  }

  const seatsUsed = teamStatus.seats_used || 0;
  const seatsPurchased = teamStatus.seats_purchased || 0;
  const isAdmin = teamStatus.role === "admin" || teamStatus.is_team_admin === true;
  const seatUsagePercentage = seatsPurchased > 0 ? (seatsUsed / seatsPurchased) * 100 : 0;

  // Simplified view for non-admin members
  if (!isAdmin) {
    return (
      <>
        <DialogHeader>
          <DialogTitle className="text-base">Team Information</DialogTitle>
        </DialogHeader>

        <div className="mt-3 space-y-3 overflow-hidden">
          {/* Compact team info */}
          <div className="bg-muted/50 rounded-lg p-3">
            <div className="flex items-center justify-between gap-2">
              <div className="min-w-0 flex-1">
                <h3 className="font-medium truncate">{teamStatus.team_name}</h3>
                <p className="text-xs text-muted-foreground">
                  Member since {new Date(teamStatus.created_at || "").toLocaleDateString()}
                </p>
              </div>
              <Badge variant="secondary" className="h-5 text-xs">
                <User className="mr-1 h-2.5 w-2.5" />
                Member
              </Badge>
            </div>
          </div>

          {/* Leave team section */}
          <div className="rounded-lg border border-muted p-3">
            <p className="text-xs text-muted-foreground mb-2.5">
              Need to leave this team? You'll need an invitation to rejoin.
            </p>
            <TeamMembersList teamStatus={teamStatus} />
          </div>
        </div>
      </>
    );
  }

  // Full admin view
  return (
    <>
      <DialogHeader>
        <DialogTitle className="text-base">Team Dashboard</DialogTitle>
      </DialogHeader>

      <div className="mt-3 space-y-3 overflow-hidden">
        {/* Seat limit exceeded warning */}
        {teamStatus.seat_limit_exceeded && (
          <div className="rounded-md bg-destructive/10 p-2.5 text-destructive text-xs flex items-start gap-2">
            <AlertTriangle className="h-3.5 w-3.5 mt-0.5 flex-shrink-0" />
            <span>Seat limit exceeded. Remove members or purchase additional seats.</span>
          </div>
        )}

        {/* Compact header with all info */}
        <div className="bg-muted/50 rounded-lg p-3 space-y-2.5">
          <div className="flex items-center justify-between gap-2">
            <div className="min-w-0 flex-1">
              <h3 className="font-medium truncate">{teamStatus.team_name}</h3>
              <p className="text-xs text-muted-foreground">
                Created {new Date(teamStatus.created_at || "").toLocaleDateString()}
              </p>
            </div>
            <Badge variant={isAdmin ? "default" : "secondary"} className="h-5 text-xs">
              <Crown className="mr-1 h-2.5 w-2.5" />
              Admin
            </Badge>
          </div>

          {/* Seat usage bar */}
          <div className="space-y-1.5">
            <div className="flex items-center justify-between text-xs">
              <span className="text-muted-foreground">Seat Usage</span>
              <span className="font-medium">
                {seatsUsed}/{seatsPurchased} ({Math.round(seatUsagePercentage)}%)
              </span>
            </div>
            <div className="h-2 w-full overflow-hidden rounded-full bg-muted">
              <div
                className="h-full transition-all bg-emerald-500"
                style={{
                  width: `${Math.min(seatUsagePercentage, 100)}%`
                }}
              />
            </div>
          </div>
        </div>

        {/* Action buttons */}
        {isAdmin && (
          <Button
            onClick={() => setIsInviteDialogOpen(true)}
            disabled={teamStatus.seat_limit_exceeded}
            size="sm"
            className="w-full"
          >
            <UserPlus className="mr-1.5 h-3.5 w-3.5" />
            Invite Members
          </Button>
        )}

        {/* Members list */}
        <TeamMembersList teamStatus={teamStatus} />
      </div>

      {/* Invite dialog */}
      <TeamInviteDialog
        open={isInviteDialogOpen}
        onOpenChange={setIsInviteDialogOpen}
        teamStatus={teamStatus}
      />
    </>
  );
}
