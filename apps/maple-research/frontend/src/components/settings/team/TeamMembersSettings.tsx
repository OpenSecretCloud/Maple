import { useState } from "react";
import { useBlocker } from "@tanstack/react-router";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useOpenSecret } from "@opensecret/react";
import { AlertCircle, Clock, Crown, Loader2, RotateCw, UserMinus, X } from "lucide-react";
import { getBillingService } from "@/billing/billingService";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Separator } from "@/components/ui/separator";
import { useSettingsNavigationLock } from "@/contexts/SettingsNavigationLockContext";
import type { TeamInvite, TeamMember, TeamStatus } from "@/types/team";

type PendingAction =
  | { type: "remove"; member: TeamMember }
  | { type: "revoke"; invite: TeamInvite }
  | null;

function getTimeRemaining(expiresAt: string) {
  const now = new Date();
  const expires = new Date(expiresAt);
  const diff = expires.getTime() - now.getTime();

  if (diff <= 0) return "Expired";

  const days = Math.floor(diff / (1000 * 60 * 60 * 24));
  const hours = Math.floor((diff % (1000 * 60 * 60 * 24)) / (1000 * 60 * 60));

  if (days > 0) {
    return `${days} day${days > 1 ? "s" : ""} remaining`;
  }

  return `${hours} hour${hours > 1 ? "s" : ""} remaining`;
}

export function TeamMembersSettings({ teamStatus }: { teamStatus: TeamStatus }) {
  const os = useOpenSecret();
  const queryClient = useQueryClient();
  const [pendingAction, setPendingAction] = useState<PendingAction>(null);
  const [isProcessing, setIsProcessing] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);

  useBlocker({
    shouldBlockFn: () => isProcessing,
    disabled: !isProcessing,
    enableBeforeUnload: isProcessing
  });
  useSettingsNavigationLock(isProcessing);

  const currentUserEmail = os.auth.user?.user.email;
  const isAdmin = teamStatus.role === "admin" || teamStatus.is_team_admin === true;

  const {
    data: membersData,
    isLoading,
    isError,
    refetch
  } = useQuery({
    queryKey: ["teamMembers"],
    queryFn: () => getBillingService().getTeamMembers(),
    enabled: teamStatus.team_created && isAdmin
  });

  const members = membersData?.members ?? [];
  const pendingInvites = membersData?.pending_invites ?? [];

  const closeConfirmation = () => {
    if (isProcessing) return;
    setPendingAction(null);
    setActionError(null);
  };

  const handleRemoveMember = async (member: TeamMember) => {
    setIsProcessing(true);
    setActionError(null);

    try {
      await getBillingService().removeTeamMember(member.user_id);
      await queryClient.invalidateQueries({ queryKey: ["teamMembers"] });
      await queryClient.invalidateQueries({ queryKey: ["teamStatus"] });
      setPendingAction(null);
    } catch (error) {
      console.error("Failed to remove member:", error);
      setActionError(error instanceof Error ? error.message : "Failed to remove team member");
    } finally {
      setIsProcessing(false);
    }
  };

  const handleRevokeInvite = async (invite: TeamInvite) => {
    setIsProcessing(true);
    setActionError(null);

    try {
      await getBillingService().revokeTeamInvite(invite.invite_id);
      await queryClient.invalidateQueries({ queryKey: ["teamMembers"] });
      await queryClient.invalidateQueries({ queryKey: ["teamStatus"] });
      setPendingAction(null);
    } catch (error) {
      console.error("Failed to revoke invite:", error);
      setActionError(error instanceof Error ? error.message : "Failed to revoke invitation");
    } finally {
      setIsProcessing(false);
    }
  };

  if (!isAdmin) return null;

  if (isLoading) {
    return (
      <div className="flex items-center justify-center py-10">
        <Loader2 className="h-6 w-6 animate-spin text-muted-foreground" />
      </div>
    );
  }

  if (isError) {
    return (
      <Alert variant="destructive">
        <AlertCircle className="h-4 w-4" />
        <AlertDescription className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
          <span>Unable to load team members.</span>
          <Button type="button" variant="outline" size="sm" onClick={() => refetch()}>
            <RotateCw className="mr-2 h-3.5 w-3.5" />
            Try again
          </Button>
        </AlertDescription>
      </Alert>
    );
  }

  return (
    <div className="space-y-5">
      <div>
        <h3 className="text-base font-semibold">Team members</h3>
        <p className="mt-1 text-sm text-muted-foreground">
          {members.length} active {members.length === 1 ? "member" : "members"}
          {pendingInvites.length > 0 && ` • ${pendingInvites.length} pending invites`}
        </p>
      </div>

      <div className="space-y-2">
        {members.map((member) => {
          const isCurrentUser = member.email === currentUserEmail;
          const memberIsAdmin = member.role === "admin";
          const isConfirming =
            pendingAction?.type === "remove" && pendingAction.member.user_id === member.user_id;

          return (
            <div key={member.user_id} className="rounded-lg border border-border/70 p-3 sm:p-4">
              <div className="flex min-w-0 items-start justify-between gap-3">
                <div className="min-w-0 flex-1">
                  <div className="flex min-w-0 flex-wrap items-center gap-1.5">
                    <p className="min-w-0 truncate text-sm font-medium">{member.email}</p>
                    {memberIsAdmin && (
                      <Badge variant="default" className="h-5">
                        <Crown className="mr-1 h-3 w-3" />
                        Admin
                      </Badge>
                    )}
                    {isCurrentUser && (
                      <Badge variant="secondary" className="h-5">
                        You
                      </Badge>
                    )}
                  </div>
                  <p className="mt-1 text-xs text-muted-foreground">
                    Joined {new Date(member.joined_at).toLocaleDateString()}
                  </p>
                </div>

                {!isCurrentUser && (
                  <Button
                    type="button"
                    variant="ghost"
                    size="sm"
                    className="shrink-0 text-destructive hover:text-destructive"
                    onClick={() => {
                      setActionError(null);
                      setPendingAction({ type: "remove", member });
                    }}
                    disabled={isProcessing}
                  >
                    <UserMinus className="mr-1.5 h-3.5 w-3.5" />
                    Remove
                  </Button>
                )}
              </div>

              {isConfirming && (
                <div className="mt-4 border-t border-border/70 pt-4">
                  <p className="text-sm font-medium">Remove team member?</p>
                  <p className="mt-1 text-sm leading-relaxed text-muted-foreground">
                    Are you sure you want to remove {member.email} from the team? They will lose
                    access to all team resources immediately.
                  </p>
                  {actionError && (
                    <Alert variant="destructive" className="mt-3">
                      <AlertCircle className="h-4 w-4" />
                      <AlertDescription>{actionError}</AlertDescription>
                    </Alert>
                  )}
                  <div className="mt-4 flex flex-col-reverse gap-2 sm:flex-row sm:justify-end">
                    <Button
                      type="button"
                      variant="outline"
                      onClick={closeConfirmation}
                      disabled={isProcessing}
                    >
                      Cancel
                    </Button>
                    <Button
                      type="button"
                      variant="destructive"
                      onClick={() => handleRemoveMember(member)}
                      disabled={isProcessing}
                    >
                      {isProcessing && <Loader2 className="mr-2 h-4 w-4 animate-spin" />}
                      {isProcessing ? "Removing..." : "Remove member"}
                    </Button>
                  </div>
                </div>
              )}
            </div>
          );
        })}
      </div>

      {pendingInvites.length > 0 && (
        <>
          <Separator />
          <div className="space-y-3">
            <h3 className="text-sm font-medium text-muted-foreground">Pending invites</h3>
            {pendingInvites.map((invite) => {
              const isConfirming =
                pendingAction?.type === "revoke" &&
                pendingAction.invite.invite_id === invite.invite_id;

              return (
                <div
                  key={invite.invite_id}
                  className="rounded-lg border border-border/70 p-3 sm:p-4"
                >
                  <div className="flex min-w-0 items-start justify-between gap-3">
                    <div className="min-w-0 flex-1">
                      <p className="truncate text-sm font-medium">{invite.email}</p>
                      <p className="mt-1 flex items-center gap-1 text-xs text-muted-foreground">
                        <Clock className="h-3 w-3" />
                        {getTimeRemaining(invite.expires_at)}
                      </p>
                    </div>
                    <Button
                      type="button"
                      variant="ghost"
                      size="sm"
                      className="shrink-0 text-destructive hover:text-destructive"
                      onClick={() => {
                        setActionError(null);
                        setPendingAction({ type: "revoke", invite });
                      }}
                      disabled={isProcessing}
                    >
                      <X className="mr-1.5 h-3.5 w-3.5" />
                      Revoke
                    </Button>
                  </div>

                  {isConfirming && (
                    <div className="mt-4 border-t border-border/70 pt-4">
                      <p className="text-sm font-medium">Revoke invitation?</p>
                      <p className="mt-1 text-sm leading-relaxed text-muted-foreground">
                        Are you sure you want to revoke the invitation for {invite.email}? They will
                        no longer be able to join the team using this invitation.
                      </p>
                      {actionError && (
                        <Alert variant="destructive" className="mt-3">
                          <AlertCircle className="h-4 w-4" />
                          <AlertDescription>{actionError}</AlertDescription>
                        </Alert>
                      )}
                      <div className="mt-4 flex flex-col-reverse gap-2 sm:flex-row sm:justify-end">
                        <Button
                          type="button"
                          variant="outline"
                          onClick={closeConfirmation}
                          disabled={isProcessing}
                        >
                          Cancel
                        </Button>
                        <Button
                          type="button"
                          variant="destructive"
                          onClick={() => handleRevokeInvite(invite)}
                          disabled={isProcessing}
                        >
                          {isProcessing && <Loader2 className="mr-2 h-4 w-4 animate-spin" />}
                          {isProcessing ? "Revoking..." : "Revoke invitation"}
                        </Button>
                      </div>
                    </div>
                  )}
                </div>
              );
            })}
          </div>
        </>
      )}

      {members.length === 0 && pendingInvites.length === 0 && (
        <p className="py-4 text-center text-sm text-muted-foreground">
          No team members yet. Start by inviting your team.
        </p>
      )}
    </div>
  );
}
