import { useState } from "react";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Input } from "@/components/ui/input";
import { UserPlus, AlertTriangle, Crown, User, Pencil, Check, X, Loader2 } from "lucide-react";
import { TeamInviteDialog } from "./TeamInviteDialog";
import { TeamMembersList } from "./TeamMembersList";
import { getBillingService } from "@/billing/billingService";
import { useQueryClient } from "@tanstack/react-query";
import type { TeamStatus } from "@/types/team";

interface TeamDashboardProps {
  teamStatus?: TeamStatus;
}

function DashboardHeader({
  title,
  description
}: {
  title: React.ReactNode;
  description?: React.ReactNode;
}) {
  return (
    <div className="space-y-1">
      <h2 className="text-base font-semibold leading-none tracking-tight">{title}</h2>
      {description ? <div className="text-sm text-muted-foreground">{description}</div> : null}
    </div>
  );
}

export function TeamDashboard({ teamStatus }: TeamDashboardProps) {
  const [isInviteDialogOpen, setIsInviteDialogOpen] = useState(false);
  const [isEditingName, setIsEditingName] = useState(false);
  const [editedName, setEditedName] = useState("");
  const [isSavingName, setIsSavingName] = useState(false);
  const [nameError, setNameError] = useState<string | null>(null);
  const queryClient = useQueryClient();

  if (!teamStatus) {
    return <DashboardHeader title="Team Dashboard" description="Loading team information..." />;
  }

  const seatsUsed = teamStatus.seats_used || 0;
  const seatsPurchased = teamStatus.seats_purchased || 0;
  const isAdmin = teamStatus.role === "admin" || teamStatus.is_team_admin === true;
  const seatUsagePercentage = seatsPurchased > 0 ? (seatsUsed / seatsPurchased) * 100 : 0;

  const handleStartEdit = () => {
    setEditedName(teamStatus.team_name || "");
    setIsEditingName(true);
    setNameError(null);
  };

  const handleCancelEdit = () => {
    setIsEditingName(false);
    setEditedName("");
    setNameError(null);
  };

  const handleSaveName = async () => {
    const trimmedName = editedName.trim();

    // Validation
    if (!trimmedName) {
      setNameError("Team name cannot be empty");
      return;
    }

    if (trimmedName.length > 100) {
      setNameError("Team name must be 100 characters or less");
      return;
    }

    if (trimmedName === teamStatus.team_name) {
      handleCancelEdit();
      return;
    }

    setIsSavingName(true);
    setNameError(null);

    try {
      const billingService = getBillingService();
      await billingService.updateTeamName(trimmedName);

      // Invalidate queries to refresh the data
      await queryClient.invalidateQueries({ queryKey: ["teamStatus"] });
      await queryClient.invalidateQueries({ queryKey: ["billingStatus"] });

      setIsEditingName(false);
      setEditedName("");
    } catch (error) {
      console.error("Failed to update team name:", error);
      setNameError(error instanceof Error ? error.message : "Failed to update team name");
    } finally {
      setIsSavingName(false);
    }
  };

  // Simplified view for non-admin members
  if (!isAdmin) {
    return (
      <>
        <DashboardHeader title="Team Information" />

        <div className="mt-3 space-y-3 overflow-hidden">
          {/* Compact team info */}
          <div className="bg-muted/50 rounded-lg p-3">
            <div className="flex items-center justify-between gap-2">
              <div className="min-w-0 flex-1">
                <h3 className="font-medium truncate">{teamStatus.team_name}</h3>
                {teamStatus.created_at && (
                  <p className="text-xs text-muted-foreground">
                    Member since {new Date(teamStatus.created_at).toLocaleDateString()}
                  </p>
                )}
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
      <DashboardHeader title="Team Dashboard" />

      <div className="mt-3 space-y-3 overflow-hidden">
        {/* Seat limit exceeded warning */}
        {teamStatus.seat_limit_exceeded && (
          <div className="rounded-md bg-destructive/10 p-2.5 text-destructive text-xs flex items-start gap-2">
            <AlertTriangle className="h-3.5 w-3.5 mt-0.5 flex-shrink-0" />
            <span>Seat limit exceeded. Remove members or purchase additional seats.</span>
          </div>
        )}

        {/* Compact header with all info */}
        <div className="bg-muted/50 rounded-lg p-3 space-y-2.5 group">
          <div className="flex items-center justify-between gap-2">
            <div className="min-w-0 flex-1">
              {isEditingName ? (
                <div className="space-y-2">
                  <div className="flex items-center gap-2">
                    <Input
                      value={editedName}
                      onChange={(e) => setEditedName(e.target.value)}
                      onKeyDown={(e) => {
                        if (e.key === "Enter") handleSaveName();
                        if (e.key === "Escape") handleCancelEdit();
                      }}
                      className="h-7 text-sm font-medium"
                      maxLength={100}
                      autoFocus
                      disabled={isSavingName}
                    />
                    {isSavingName ? (
                      <div className="h-7 w-7 flex items-center justify-center">
                        <Loader2 className="h-3.5 w-3.5 animate-spin" />
                      </div>
                    ) : (
                      <>
                        <Button
                          size="sm"
                          variant="ghost"
                          className="h-7 w-7 p-0"
                          onClick={handleSaveName}
                        >
                          <Check className="h-3.5 w-3.5" />
                        </Button>
                        <Button
                          size="sm"
                          variant="ghost"
                          className="h-7 w-7 p-0"
                          onClick={handleCancelEdit}
                        >
                          <X className="h-3.5 w-3.5" />
                        </Button>
                      </>
                    )}
                  </div>
                  <div className="flex items-center justify-between text-xs">
                    <span className="text-muted-foreground">
                      {editedName.trim().length}/100 characters
                    </span>
                    {nameError && <span className="text-destructive">{nameError}</span>}
                  </div>
                </div>
              ) : (
                <div className="flex items-center gap-2">
                  <h3 className="font-medium truncate">{teamStatus.team_name}</h3>
                  <Button
                    size="sm"
                    variant="ghost"
                    className="h-5 w-5 p-0 opacity-0 group-hover:opacity-100 transition-opacity"
                    onClick={handleStartEdit}
                  >
                    <Pencil className="h-3 w-3" />
                  </Button>
                </div>
              )}
              {!isEditingName && teamStatus.created_at && (
                <p className="text-xs text-muted-foreground">
                  Created {new Date(teamStatus.created_at).toLocaleDateString()}
                </p>
              )}
            </div>
            {!isEditingName && (
              <Badge variant={isAdmin ? "default" : "secondary"} className="h-5 text-xs">
                <Crown className="mr-1 h-2.5 w-2.5" />
                Admin
              </Badge>
            )}
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
