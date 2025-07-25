import { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Separator } from "@/components/ui/separator";
import { Alert, AlertDescription } from "@/components/ui/alert";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle
} from "@/components/ui/alert-dialog";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger
} from "@/components/ui/dropdown-menu";
import {
  Crown,
  Clock,
  MoreVertical,
  UserMinus,
  X,
  LogOut,
  Loader2,
  AlertCircle
} from "lucide-react";
import { getBillingService } from "@/billing/billingService";
import { useOpenSecret } from "@opensecret/react";
import type { TeamStatus, TeamMember, TeamInvite } from "@/types/team";

interface TeamMembersListProps {
  teamStatus?: TeamStatus;
}

export function TeamMembersList({ teamStatus }: TeamMembersListProps) {
  const os = useOpenSecret();
  const queryClient = useQueryClient();
  const [removeMemberDialog, setRemoveMemberDialog] = useState<{
    open: boolean;
    member?: TeamMember;
  }>({ open: false });
  const [revokeInviteDialog, setRevokeInviteDialog] = useState<{
    open: boolean;
    invite?: TeamInvite;
  }>({ open: false });
  const [leaveTeamDialog, setLeaveTeamDialog] = useState(false);
  const [isProcessing, setIsProcessing] = useState(false);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);

  const currentUserEmail = os.auth.user?.user.email;
  const isAdmin = teamStatus?.role === "admin" || teamStatus?.is_team_admin === true;

  // Fetch team members only for admins
  const { data: membersData, isLoading } = useQuery({
    queryKey: ["teamMembers"],
    queryFn: async () => {
      const billingService = getBillingService();
      return await billingService.getTeamMembers();
    },
    enabled: !!teamStatus?.team_created && isAdmin
  });

  const members = membersData?.members || [];
  const pendingInvites = membersData?.pending_invites || [];

  const handleRemoveMember = async (userId: string) => {
    setIsProcessing(true);
    setErrorMessage(null);
    try {
      const billingService = getBillingService();
      await billingService.removeTeamMember(userId);

      // Invalidate queries
      await queryClient.invalidateQueries({ queryKey: ["teamMembers"] });
      await queryClient.invalidateQueries({ queryKey: ["teamStatus"] });

      setRemoveMemberDialog({ open: false });
    } catch (error) {
      console.error("Failed to remove member:", error);
      const message = error instanceof Error ? error.message : "Failed to remove team member";
      setErrorMessage(message);
      // Keep dialog open to show error
    } finally {
      setIsProcessing(false);
    }
  };

  const handleRevokeInvite = async (inviteId: string) => {
    setIsProcessing(true);
    setErrorMessage(null);
    try {
      const billingService = getBillingService();
      await billingService.revokeTeamInvite(inviteId);

      // Invalidate queries
      await queryClient.invalidateQueries({ queryKey: ["teamMembers"] });
      await queryClient.invalidateQueries({ queryKey: ["teamStatus"] });

      setRevokeInviteDialog({ open: false });
    } catch (error) {
      console.error("Failed to revoke invite:", error);
      const message = error instanceof Error ? error.message : "Failed to revoke invitation";
      setErrorMessage(message);
    } finally {
      setIsProcessing(false);
    }
  };

  const handleLeaveTeam = async () => {
    setIsProcessing(true);
    setErrorMessage(null);
    try {
      const billingService = getBillingService();
      await billingService.leaveTeam();

      // Invalidate queries and refresh page
      await queryClient.invalidateQueries({ queryKey: ["teamStatus"] });
      window.location.reload();
    } catch (error) {
      console.error("Failed to leave team:", error);
      const message = error instanceof Error ? error.message : "Failed to leave team";
      setErrorMessage(message);
    } finally {
      setIsProcessing(false);
    }
  };

  const getTimeRemaining = (expiresAt: string) => {
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
  };

  // Simplified view for non-admin members
  if (!isAdmin) {
    return (
      <>
        <Button
          variant="outline"
          onClick={() => setLeaveTeamDialog(true)}
          className="w-full text-destructive hover:text-destructive"
        >
          <LogOut className="mr-2 h-4 w-4" />
          Leave Team
        </Button>

        {/* Leave Team Dialog */}
        <AlertDialog
          open={leaveTeamDialog}
          onOpenChange={(open) => {
            if (!open) setErrorMessage(null);
            setLeaveTeamDialog(open);
          }}
        >
          <AlertDialogContent>
            <AlertDialogHeader>
              <AlertDialogTitle>Leave team?</AlertDialogTitle>
              <AlertDialogDescription>
                Are you sure you want to leave the team? You will lose access to all team resources
                and will need to be invited again to rejoin.
              </AlertDialogDescription>
            </AlertDialogHeader>
            {errorMessage && (
              <Alert variant="destructive" className="mt-4">
                <AlertCircle className="h-4 w-4" />
                <AlertDescription>{errorMessage}</AlertDescription>
              </Alert>
            )}
            <AlertDialogFooter>
              <AlertDialogCancel disabled={isProcessing}>Cancel</AlertDialogCancel>
              <AlertDialogAction
                onClick={handleLeaveTeam}
                disabled={isProcessing}
                className="bg-destructive text-white hover:bg-destructive/90"
              >
                {isProcessing ? (
                  <>
                    <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                    Leaving...
                  </>
                ) : (
                  "Leave Team"
                )}
              </AlertDialogAction>
            </AlertDialogFooter>
          </AlertDialogContent>
        </AlertDialog>
      </>
    );
  }

  // Admin view with full member list
  if (isLoading) {
    return (
      <div className="flex items-center justify-center py-8">
        <Loader2 className="h-6 w-6 animate-spin text-muted-foreground" />
      </div>
    );
  }

  return (
    <>
      <div className="space-y-4">
        <div>
          <h3 className="text-lg font-semibold">Team Members</h3>
          <p className="text-sm text-muted-foreground">
            {members.length} active {members.length === 1 ? "member" : "members"}
            {pendingInvites.length > 0 && ` • ${pendingInvites.length} pending invites`}
          </p>
        </div>
        {/* Active Members */}
        <div className="space-y-3">
          {members.map((member: TeamMember) => {
            const isCurrentUser = member.email === currentUserEmail;
            const memberIsAdmin = member.role === "admin";

            return (
              <div
                key={member.user_id}
                className="flex items-center justify-between gap-2 overflow-hidden"
              >
                <div className="min-w-0 flex-1 overflow-hidden">
                  <div className="flex items-center gap-2 overflow-hidden">
                    <p className="text-sm font-medium truncate">{member.email}</p>
                    <div className="flex items-center gap-1 flex-shrink-0">
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
                  </div>
                  <p className="text-xs text-muted-foreground">
                    Joined {new Date(member.joined_at).toLocaleDateString()}
                  </p>
                </div>

                {!isCurrentUser && isAdmin && (
                  <DropdownMenu>
                    <DropdownMenuTrigger asChild>
                      <Button variant="ghost" size="icon" className="h-8 w-8">
                        <MoreVertical className="h-4 w-4" />
                      </Button>
                    </DropdownMenuTrigger>
                    <DropdownMenuContent align="end">
                      <DropdownMenuItem
                        onClick={() => setRemoveMemberDialog({ open: true, member })}
                        className="text-destructive"
                      >
                        <UserMinus className="mr-2 h-4 w-4" />
                        Remove from team
                      </DropdownMenuItem>
                    </DropdownMenuContent>
                  </DropdownMenu>
                )}

                {isCurrentUser && !isAdmin && (
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() => setLeaveTeamDialog(true)}
                    className="text-destructive hover:text-destructive"
                  >
                    <LogOut className="mr-2 h-4 w-4" />
                    Leave team
                  </Button>
                )}
              </div>
            );
          })}
        </div>

        {/* Pending Invites */}
        {pendingInvites.length > 0 && (
          <>
            <Separator />
            <div className="space-y-3">
              <h4 className="text-sm font-medium text-muted-foreground">Pending Invites</h4>
              {pendingInvites.map((invite: TeamInvite) => (
                <div
                  key={invite.invite_id}
                  className="flex items-center justify-between gap-2 overflow-hidden"
                >
                  <div className="min-w-0 flex-1 overflow-hidden">
                    <p className="text-sm font-medium truncate">{invite.email}</p>
                    <p className="text-xs text-muted-foreground flex items-center gap-1">
                      <Clock className="h-3 w-3" />
                      {getTimeRemaining(invite.expires_at)}
                    </p>
                  </div>

                  {isAdmin && (
                    <Button
                      variant="ghost"
                      size="icon"
                      className="h-8 w-8"
                      onClick={() => setRevokeInviteDialog({ open: true, invite })}
                    >
                      <X className="h-4 w-4" />
                    </Button>
                  )}
                </div>
              ))}
            </div>
          </>
        )}

        {members.length === 0 && pendingInvites.length === 0 && (
          <p className="text-sm text-muted-foreground text-center py-4">
            No team members yet. Start by inviting your team!
          </p>
        )}
      </div>

      {/* Remove Member Dialog */}
      <AlertDialog
        open={removeMemberDialog.open}
        onOpenChange={(open) => {
          if (!open) setErrorMessage(null);
          setRemoveMemberDialog({ open });
        }}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Remove team member?</AlertDialogTitle>
            <AlertDialogDescription>
              Are you sure you want to remove {removeMemberDialog.member?.email} from the team? They
              will lose access to all team resources immediately.
            </AlertDialogDescription>
          </AlertDialogHeader>
          {errorMessage && (
            <Alert variant="destructive" className="mt-4">
              <AlertCircle className="h-4 w-4" />
              <AlertDescription>{errorMessage}</AlertDescription>
            </Alert>
          )}
          <AlertDialogFooter>
            <AlertDialogCancel disabled={isProcessing}>Cancel</AlertDialogCancel>
            <AlertDialogAction
              onClick={() =>
                removeMemberDialog.member && handleRemoveMember(removeMemberDialog.member.user_id)
              }
              disabled={isProcessing}
              className="bg-destructive text-white hover:bg-destructive/90"
            >
              {isProcessing ? (
                <>
                  <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                  Removing...
                </>
              ) : (
                "Remove member"
              )}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      {/* Revoke Invite Dialog */}
      <AlertDialog
        open={revokeInviteDialog.open}
        onOpenChange={(open) => {
          if (!open) setErrorMessage(null);
          setRevokeInviteDialog({ open });
        }}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Revoke invitation?</AlertDialogTitle>
            <AlertDialogDescription>
              Are you sure you want to revoke the invitation for {revokeInviteDialog.invite?.email}?
              They will no longer be able to join the team using this invitation.
            </AlertDialogDescription>
          </AlertDialogHeader>
          {errorMessage && (
            <Alert variant="destructive" className="mt-4">
              <AlertCircle className="h-4 w-4" />
              <AlertDescription>{errorMessage}</AlertDescription>
            </Alert>
          )}
          <AlertDialogFooter>
            <AlertDialogCancel disabled={isProcessing}>Cancel</AlertDialogCancel>
            <AlertDialogAction
              onClick={() =>
                revokeInviteDialog.invite && handleRevokeInvite(revokeInviteDialog.invite.invite_id)
              }
              disabled={isProcessing}
              className="bg-destructive text-white hover:bg-destructive/90"
            >
              {isProcessing ? (
                <>
                  <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                  Revoking...
                </>
              ) : (
                "Revoke invitation"
              )}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      {/* Leave Team Dialog */}
      <AlertDialog
        open={leaveTeamDialog}
        onOpenChange={(open) => {
          if (!open) setErrorMessage(null);
          setLeaveTeamDialog(open);
        }}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Leave team?</AlertDialogTitle>
            <AlertDialogDescription>
              Are you sure you want to leave the team? You will lose access to all team resources
              and will need to be invited again to rejoin.
            </AlertDialogDescription>
          </AlertDialogHeader>
          {errorMessage && (
            <Alert variant="destructive" className="mt-4">
              <AlertCircle className="h-4 w-4" />
              <AlertDescription>{errorMessage}</AlertDescription>
            </Alert>
          )}
          <AlertDialogFooter>
            <AlertDialogCancel disabled={isProcessing}>Cancel</AlertDialogCancel>
            <AlertDialogAction
              onClick={handleLeaveTeam}
              disabled={isProcessing}
              className="bg-destructive text-white hover:bg-destructive/90"
            >
              {isProcessing ? (
                <>
                  <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                  Leaving...
                </>
              ) : (
                "Leave team"
              )}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>
  );
}
