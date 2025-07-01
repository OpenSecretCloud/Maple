import { useState } from "react";
import { DialogHeader, DialogTitle, DialogDescription } from "@/components/ui/dialog";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Users, UserPlus, Settings, AlertTriangle, Crown, User } from "lucide-react";
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
  const seatsAvailable = teamStatus.seats_available || 0;
  const isAdmin = teamStatus.role === "admin" || teamStatus.is_team_admin === true;
  const seatUsagePercentage = seatsPurchased > 0 ? (seatsUsed / seatsPurchased) * 100 : 0;

  // Simplified view for non-admin members
  if (!isAdmin) {
    return (
      <>
        <DialogHeader>
          <DialogTitle>Team Information</DialogTitle>
          <DialogDescription>
            You are a member of {teamStatus.team_name}
          </DialogDescription>
        </DialogHeader>

        <div className="mt-6 space-y-6">
          {/* Team overview */}
          <Card>
            <CardHeader>
              <div className="flex items-center justify-between">
                <div>
                  <CardTitle className="text-xl">{teamStatus.team_name}</CardTitle>
                  <CardDescription>
                    Joined {new Date(teamStatus.created_at || "").toLocaleDateString()}
                  </CardDescription>
                </div>
                <Badge variant="secondary">
                  <User className="mr-1 h-3 w-3" />
                  Member
                </Badge>
              </div>
            </CardHeader>
          </Card>

          {/* Simple leave team button for members */}
          <Card>
            <CardHeader>
              <CardTitle className="text-lg">Leave Team</CardTitle>
              <CardDescription>
                If you leave the team, you'll need to be invited again to rejoin.
              </CardDescription>
            </CardHeader>
            <CardContent>
              <TeamMembersList teamStatus={teamStatus} />
            </CardContent>
          </Card>
        </div>
      </>
    );
  }

  // Full admin view
  return (
    <>
      <DialogHeader>
        <DialogTitle>Team Dashboard</DialogTitle>
        <DialogDescription>Manage your team members and monitor seat usage</DialogDescription>
      </DialogHeader>

      <div className="mt-6 space-y-6">
        {/* Seat limit exceeded warning */}
        {teamStatus.seat_limit_exceeded && (
          <Alert variant="destructive">
            <AlertTriangle className="h-4 w-4" />
            <AlertTitle>Seat Limit Exceeded</AlertTitle>
            <AlertDescription>
              Your team has exceeded its seat limit. Please remove members or purchase additional
              seats to continue inviting new members.
            </AlertDescription>
          </Alert>
        )}

        {/* Team overview */}
        <Card>
          <CardHeader>
            <div className="flex items-center justify-between">
              <div>
                <CardTitle className="text-xl">{teamStatus.team_name}</CardTitle>
                <CardDescription>
                  Created {new Date(teamStatus.created_at || "").toLocaleDateString()}
                </CardDescription>
              </div>
              <Badge variant={isAdmin ? "default" : "secondary"}>
                {isAdmin ? (
                  <>
                    <Crown className="mr-1 h-3 w-3" />
                    Admin
                  </>
                ) : (
                  <>
                    <User className="mr-1 h-3 w-3" />
                    Member
                  </>
                )}
              </Badge>
            </div>
          </CardHeader>
        </Card>

        {/* Seat usage */}
        <Card>
          <CardHeader>
            <CardTitle className="text-lg flex items-center gap-2">
              <Users className="h-5 w-5" />
              Seat Usage
            </CardTitle>
          </CardHeader>
          <CardContent>
            <div className="space-y-3">
              <div className="flex justify-between items-baseline">
                <span className="text-2xl font-bold">
                  {seatsUsed} / {seatsPurchased}
                </span>
                <span className="text-sm text-muted-foreground">seats used</span>
              </div>
              <div className="w-full bg-secondary h-2 rounded-full overflow-hidden">
                <div
                  className={`h-full transition-all ${
                    seatUsagePercentage >= 100
                      ? "bg-destructive"
                      : seatUsagePercentage >= 80
                        ? "bg-amber-500"
                        : "bg-primary"
                  }`}
                  style={{ width: `${Math.min(seatUsagePercentage, 100)}%` }}
                />
              </div>
              {seatsAvailable > 0 && (
                <p className="text-sm text-muted-foreground">
                  {seatsAvailable} {seatsAvailable === 1 ? "seat" : "seats"} available
                </p>
              )}
            </div>
          </CardContent>
        </Card>

        {/* Team statistics */}
        <div className="grid grid-cols-2 gap-4">
          <Card>
            <CardContent className="pt-6">
              <div className="text-2xl font-bold">{teamStatus.members_count || 0}</div>
              <p className="text-sm text-muted-foreground">Active Members</p>
            </CardContent>
          </Card>
          <Card>
            <CardContent className="pt-6">
              <div className="text-2xl font-bold">{teamStatus.pending_invites_count || 0}</div>
              <p className="text-sm text-muted-foreground">Pending Invites</p>
            </CardContent>
          </Card>
        </div>

        {/* Action buttons */}
        {isAdmin && (
          <div className="flex gap-3">
            <Button
              onClick={() => setIsInviteDialogOpen(true)}
              disabled={teamStatus.seat_limit_exceeded}
              className="flex-1"
            >
              <UserPlus className="mr-2 h-4 w-4" />
              Invite Members
            </Button>
            <Button variant="outline" disabled>
              <Settings className="mr-2 h-4 w-4" />
              Team Settings
            </Button>
          </div>
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
