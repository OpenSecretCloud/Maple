import { useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Badge } from "@/components/ui/badge";
import { Alert, AlertDescription } from "@/components/ui/alert";
import {
  UserPlus,
  AlertTriangle,
  Crown,
  User,
  Pencil,
  Check,
  X,
  Loader2,
  AlertCircle
} from "lucide-react";
import { TeamInviteDialog } from "@/components/team/TeamInviteDialog";
import { TeamMembersList } from "@/components/team/TeamMembersList";
import { getBillingService } from "@/billing/billingService";
import type { TeamStatus } from "@/types/team";

interface TeamManagementSectionProps {
  teamStatus?: TeamStatus;
}

export function TeamManagementSection({ teamStatus }: TeamManagementSectionProps) {
  const [isInviteDialogOpen, setIsInviteDialogOpen] = useState(false);
  const [isEditingName, setIsEditingName] = useState(false);
  const [editedName, setEditedName] = useState("");
  const [isSavingName, setIsSavingName] = useState(false);
  const [nameError, setNameError] = useState<string | null>(null);
  const queryClient = useQueryClient();

  // Team setup state
  const [teamName, setTeamName] = useState("");
  const [isCreating, setIsCreating] = useState(false);
  const [createError, setCreateError] = useState<string | null>(null);

  const needsSetup = teamStatus?.has_team_subscription && !teamStatus?.team_created;

  if (!teamStatus) {
    return (
      <div className="space-y-8">
        <div>
          <h2 className="text-2xl font-semibold tracking-tight">Team Management</h2>
          <p className="text-muted-foreground mt-1">Loading team information...</p>
        </div>
        <div className="flex items-center justify-center py-12">
          <Loader2 className="h-6 w-6 animate-spin" />
        </div>
      </div>
    );
  }

  // Team setup flow
  if (needsSetup) {
    const handleCreateTeam = async (e: React.FormEvent) => {
      e.preventDefault();
      if (!teamName.trim()) {
        setCreateError("Please enter a team name");
        return;
      }

      setIsCreating(true);
      setCreateError(null);

      try {
        const billingService = getBillingService();
        await billingService.createTeam({ name: teamName.trim() });
        await queryClient.invalidateQueries({ queryKey: ["teamStatus"] });
      } catch (err) {
        console.error("Failed to create team:", err);
        setCreateError(err instanceof Error ? err.message : "Failed to create team");
      } finally {
        setIsCreating(false);
      }
    };

    return (
      <div className="space-y-8">
        <div>
          <h2 className="text-2xl font-semibold tracking-tight">Set Up Your Team</h2>
          <p className="text-muted-foreground mt-1">
            You've purchased a team plan! Create your team to get started.
          </p>
        </div>

        <form onSubmit={handleCreateTeam} className="space-y-4 max-w-md">
          <div className="space-y-2">
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
                Your team plan includes <strong>{teamStatus.seats_purchased} seats</strong>. You'll
                be the team admin and can invite up to {teamStatus.seats_purchased - 1} additional
                members.
              </AlertDescription>
            </Alert>
          )}

          {createError && (
            <Alert variant="destructive">
              <AlertCircle className="h-4 w-4" />
              <AlertDescription>{createError}</AlertDescription>
            </Alert>
          )}

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
        </form>
      </div>
    );
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

  // Non-admin member view
  if (!isAdmin) {
    return (
      <div className="space-y-8">
        <div>
          <h2 className="text-2xl font-semibold tracking-tight">Team Information</h2>
          <p className="text-muted-foreground mt-1">View your team membership details.</p>
        </div>

        <div className="bg-muted/50 rounded-lg p-4 space-y-2">
          <div className="flex items-center justify-between gap-2">
            <div className="min-w-0 flex-1">
              <h3 className="font-medium truncate text-lg">{teamStatus.team_name}</h3>
              {teamStatus.created_at && (
                <p className="text-sm text-muted-foreground">
                  Member since {new Date(teamStatus.created_at).toLocaleDateString()}
                </p>
              )}
            </div>
            <Badge variant="secondary">
              <User className="mr-1 h-3 w-3" />
              Member
            </Badge>
          </div>
        </div>

        <div className="space-y-3">
          <h3 className="text-lg font-medium">Team Members</h3>
          <p className="text-sm text-muted-foreground">
            Need to leave this team? You'll need an invitation to rejoin.
          </p>
          <TeamMembersList teamStatus={teamStatus} />
        </div>
      </div>
    );
  }

  // Admin view
  return (
    <div className="space-y-8">
      <div>
        <h2 className="text-2xl font-semibold tracking-tight">Team Management</h2>
        <p className="text-muted-foreground mt-1">Manage your team members and settings.</p>
      </div>

      {/* Seat limit warning */}
      {teamStatus.seat_limit_exceeded && (
        <Alert variant="destructive">
          <AlertTriangle className="h-4 w-4" />
          <AlertDescription>
            Seat limit exceeded. Remove members or purchase additional seats.
          </AlertDescription>
        </Alert>
      )}

      {/* Team Info Card */}
      <div className="bg-muted/50 rounded-lg p-4 space-y-4 group">
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
                    className="h-8 text-sm font-medium"
                    maxLength={100}
                    autoFocus
                    disabled={isSavingName}
                  />
                  {isSavingName ? (
                    <Loader2 className="h-4 w-4 animate-spin flex-shrink-0" />
                  ) : (
                    <>
                      <Button
                        size="sm"
                        variant="ghost"
                        className="h-8 w-8 p-0"
                        onClick={handleSaveName}
                      >
                        <Check className="h-4 w-4" />
                      </Button>
                      <Button
                        size="sm"
                        variant="ghost"
                        className="h-8 w-8 p-0"
                        onClick={handleCancelEdit}
                      >
                        <X className="h-4 w-4" />
                      </Button>
                    </>
                  )}
                </div>
                <div className="flex items-center justify-between text-xs">
                  <span className="text-muted-foreground">{editedName.trim().length}/100</span>
                  {nameError && <span className="text-destructive">{nameError}</span>}
                </div>
              </div>
            ) : (
              <div className="flex items-center gap-2">
                <h3 className="font-medium truncate text-lg">{teamStatus.team_name}</h3>
                <Button
                  size="sm"
                  variant="ghost"
                  className="h-6 w-6 p-0 opacity-0 group-hover:opacity-100 transition-opacity"
                  onClick={handleStartEdit}
                >
                  <Pencil className="h-3 w-3" />
                </Button>
              </div>
            )}
            {!isEditingName && teamStatus.created_at && (
              <p className="text-sm text-muted-foreground">
                Created {new Date(teamStatus.created_at).toLocaleDateString()}
              </p>
            )}
          </div>
          {!isEditingName && (
            <Badge>
              <Crown className="mr-1 h-3 w-3" />
              Admin
            </Badge>
          )}
        </div>

        {/* Seat usage */}
        <div className="space-y-2">
          <div className="flex items-center justify-between text-sm">
            <span className="text-muted-foreground">Seat Usage</span>
            <span className="font-medium">
              {seatsUsed}/{seatsPurchased} ({Math.round(seatUsagePercentage)}%)
            </span>
          </div>
          <div className="h-2.5 w-full overflow-hidden rounded-full bg-muted">
            <div
              className="h-full transition-all bg-emerald-500"
              style={{ width: `${Math.min(seatUsagePercentage, 100)}%` }}
            />
          </div>
        </div>
      </div>

      {/* Invite button */}
      {isAdmin && (
        <Button
          onClick={() => setIsInviteDialogOpen(true)}
          disabled={teamStatus.seat_limit_exceeded}
          className="gap-2"
        >
          <UserPlus className="h-4 w-4" />
          Invite Members
        </Button>
      )}

      {/* Members list */}
      <div className="space-y-3">
        <h3 className="text-lg font-medium">Members</h3>
        <TeamMembersList teamStatus={teamStatus} />
      </div>

      {/* Invite dialog */}
      <TeamInviteDialog
        open={isInviteDialogOpen}
        onOpenChange={setIsInviteDialogOpen}
        teamStatus={teamStatus}
      />
    </div>
  );
}
