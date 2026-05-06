import { useRef, useState } from "react";
import { DialogHeader, DialogTitle, DialogDescription } from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Input } from "@/components/ui/input";
import {
  UserPlus,
  AlertTriangle,
  Crown,
  User,
  Pencil,
  Check,
  X,
  Loader2,
  CreditCard,
  Users
} from "lucide-react";
import { TeamInviteDialog } from "./TeamInviteDialog";
import { TeamMembersList } from "./TeamMembersList";
import { getBillingService } from "@/billing/billingService";
import { openBillingPortal } from "@/billing/billingPortal";
import { useLocalState } from "@/state/useLocalState";
import {
  formatTeamSeatMismatchMessage,
  getTeamSeatCounts,
  getTeamSeatMismatch
} from "@/utils/teamSeats";
import { useQueryClient } from "@tanstack/react-query";
import type { TeamStatus } from "@/types/team";

interface TeamDashboardProps {
  teamStatus?: TeamStatus;
}

export function TeamDashboard({ teamStatus }: TeamDashboardProps) {
  const [isInviteDialogOpen, setIsInviteDialogOpen] = useState(false);
  const [isEditingName, setIsEditingName] = useState(false);
  const [editedName, setEditedName] = useState("");
  const [isSavingName, setIsSavingName] = useState(false);
  const [nameError, setNameError] = useState<string | null>(null);
  const [isPortalLoading, setIsPortalLoading] = useState(false);
  const [portalError, setPortalError] = useState<string | null>(null);
  const membersSectionRef = useRef<HTMLDivElement>(null);
  const queryClient = useQueryClient();
  const { billingStatus } = useLocalState();

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

  const seatCounts = getTeamSeatCounts(teamStatus);
  const seatMismatch = getTeamSeatMismatch(teamStatus);
  const seatsUsed = seatCounts.memberCount ?? 0;
  const seatsPurchased = seatCounts.billedSeatCount ?? 0;
  const isAdmin = teamStatus.role === "admin" || teamStatus.is_team_admin === true;
  const seatUsagePercentage = seatsPurchased > 0 ? (seatsUsed / seatsPurchased) * 100 : 0;
  const canOpenBillingPortal = billingStatus
    ? !!billingStatus.stripe_customer_id
    : !!teamStatus.has_team_subscription;

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

  const handleOpenBilling = async () => {
    if (!canOpenBillingPortal) return;

    try {
      setPortalError(null);
      setIsPortalLoading(true);
      await openBillingPortal();
      await queryClient.invalidateQueries({ queryKey: ["billingStatus"] });
      await queryClient.invalidateQueries({ queryKey: ["teamStatus"] });
    } catch (error) {
      console.error("Failed to open billing portal:", error);
      setPortalError(
        "Unable to open subscription management. Please try again or contact support@trymaple.ai."
      );
    } finally {
      setIsPortalLoading(false);
    }
  };

  const handleReviewMembers = () => {
    membersSectionRef.current?.scrollIntoView({ block: "nearest" });
  };

  // Simplified view for non-admin members
  if (!isAdmin) {
    return (
      <>
        <DialogHeader>
          <DialogTitle className="text-base">Team Information</DialogTitle>
        </DialogHeader>

        <div className="mt-3 space-y-3 overflow-hidden">
          {seatMismatch && (
            <div className="rounded-md border border-destructive/40 bg-destructive/10 p-3 text-destructive">
              <div className="flex items-start gap-2">
                <AlertTriangle className="mt-0.5 h-4 w-4 flex-shrink-0" />
                <div className="space-y-1">
                  <p className="text-sm font-medium">Team usage is paused</p>
                  <p className="text-xs leading-relaxed">
                    {formatTeamSeatMismatchMessage(seatMismatch, "member")}
                  </p>
                </div>
              </div>
            </div>
          )}

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
      <DialogHeader>
        <DialogTitle className="text-base">Team Dashboard</DialogTitle>
      </DialogHeader>

      <div className="mt-3 space-y-3 overflow-hidden">
        {seatMismatch && (
          <div className="rounded-md border border-destructive/40 bg-destructive/10 p-3 text-destructive">
            <div className="flex items-start gap-2">
              <AlertTriangle className="mt-0.5 h-4 w-4 flex-shrink-0" />
              <div className="min-w-0 flex-1 space-y-2">
                <div>
                  <p className="text-sm font-medium">Team usage is paused</p>
                  <p className="mt-1 text-xs leading-relaxed">
                    {formatTeamSeatMismatchMessage(seatMismatch, "admin")}
                  </p>
                </div>
                <div className="flex flex-col gap-2 sm:flex-row">
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    className="h-8 border-destructive/30 text-destructive hover:text-destructive"
                    onClick={handleReviewMembers}
                  >
                    <Users className="mr-1.5 h-3.5 w-3.5" />
                    Manage Members
                  </Button>
                  {canOpenBillingPortal && (
                    <Button
                      type="button"
                      variant="outline"
                      size="sm"
                      className="h-8 border-destructive/30 text-destructive hover:text-destructive"
                      onClick={handleOpenBilling}
                      disabled={isPortalLoading}
                    >
                      <CreditCard className="mr-1.5 h-3.5 w-3.5" />
                      {isPortalLoading ? "Opening..." : "Add Seats"}
                    </Button>
                  )}
                </div>
                {portalError && <p className="text-xs leading-relaxed">{portalError}</p>}
              </div>
            </div>
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
                className={
                  seatMismatch
                    ? "h-full bg-destructive transition-all"
                    : "h-full bg-emerald-500 transition-all"
                }
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
            disabled={!!seatMismatch}
            size="sm"
            className="w-full"
          >
            <UserPlus className="mr-1.5 h-3.5 w-3.5" />
            Invite Members
          </Button>
        )}

        {/* Members list */}
        <div ref={membersSectionRef}>
          <TeamMembersList teamStatus={teamStatus} />
        </div>
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
