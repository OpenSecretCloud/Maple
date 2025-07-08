import { useState, useEffect } from "react";
import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useOpenSecret } from "@opensecret/react";
import { getBillingService } from "@/billing/billingService";
import { TopNav } from "@/components/TopNav";
import { FullPageMain } from "@/components/FullPageMain";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Loader2, UserPlus, AlertCircle, CheckCircle, XCircle } from "lucide-react";
import type { CheckInviteResponse } from "@/types/team";

export const Route = createFileRoute("/team/invite/$inviteId")({
  component: TeamInviteAcceptance
});

function TeamInviteAcceptance() {
  const { inviteId } = Route.useParams();
  const navigate = useNavigate();
  const os = useOpenSecret();
  const queryClient = useQueryClient();
  const [isProcessing, setIsProcessing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState(false);

  const isLoggedIn = !!os.auth.user;
  const userEmail = os.auth.user?.user.email;

  // Check invite validity
  const { data: inviteData, isLoading: checkingInvite } = useQuery<CheckInviteResponse>({
    queryKey: ["teamInvite", inviteId],
    queryFn: async () => {
      if (!isLoggedIn) {
        throw new Error("Authentication required");
      }
      const billingService = getBillingService();
      return await billingService.checkTeamInvite(inviteId);
    },
    enabled: isLoggedIn,
    retry: false
  });

  // Redirect to signup if not authenticated
  useEffect(() => {
    if (!isLoggedIn && !checkingInvite) {
      navigate({
        to: "/signup",
        search: {
          next: `/team/invite/${inviteId}`
        }
      });
    }
  }, [isLoggedIn, checkingInvite, navigate, inviteId]);

  const handleAcceptInvite = async () => {
    if (!userEmail) {
      setError("Unable to determine your email address");
      return;
    }

    setIsProcessing(true);
    setError(null);

    try {
      const billingService = getBillingService();
      await billingService.acceptTeamInvite(inviteId, { email: userEmail });

      // Invalidate relevant queries
      await queryClient.invalidateQueries({ queryKey: ["teamStatus"] });
      await queryClient.invalidateQueries({ queryKey: ["billingStatus"] });

      setSuccess(true);

      // Redirect after a short delay
      setTimeout(() => {
        navigate({ to: "/" });
      }, 2000);
    } catch (err) {
      console.error("Failed to accept invite:", err);
      setError(err instanceof Error ? err.message : "Failed to accept invitation");
    } finally {
      setIsProcessing(false);
    }
  };

  const handleDeclineInvite = () => {
    navigate({ to: "/" });
  };

  // Loading state
  if (checkingInvite || !isLoggedIn) {
    return (
      <>
        <TopNav />
        <FullPageMain>
          <div className="flex items-center justify-center min-h-[60vh]">
            <Card className="w-full max-w-md">
              <CardContent className="flex items-center justify-center py-12">
                <Loader2 className="h-8 w-8 animate-spin text-muted-foreground" />
              </CardContent>
            </Card>
          </div>
        </FullPageMain>
      </>
    );
  }

  // Invalid or expired invite
  if (!inviteData?.valid) {
    return (
      <>
        <TopNav />
        <FullPageMain>
          <div className="flex items-center justify-center min-h-[60vh]">
            <Card className="w-full max-w-md">
              <CardHeader>
                <div className="flex items-center justify-center mb-4">
                  <div className="rounded-full bg-destructive/10 p-3">
                    <XCircle className="h-8 w-8 text-destructive" />
                  </div>
                </div>
                <CardTitle className="text-center">Invalid or Expired Invitation</CardTitle>
                <CardDescription className="text-center">
                  This invitation link is no longer valid. It may have expired or already been used.
                </CardDescription>
              </CardHeader>
              <CardContent>
                <Button onClick={() => navigate({ to: "/" })} className="w-full">
                  Go to Dashboard
                </Button>
              </CardContent>
            </Card>
          </div>
        </FullPageMain>
      </>
    );
  }

  // Success state
  if (success) {
    return (
      <>
        <TopNav />
        <FullPageMain>
          <div className="flex items-center justify-center min-h-[60vh]">
            <Card className="w-full max-w-md">
              <CardHeader>
                <div className="flex items-center justify-center mb-4">
                  <div className="rounded-full bg-green-100 dark:bg-green-900/20 p-3">
                    <CheckCircle className="h-8 w-8 text-green-600 dark:text-green-400" />
                  </div>
                </div>
                <CardTitle className="text-center">Welcome to the Team!</CardTitle>
                <CardDescription className="text-center">
                  You've successfully joined {inviteData?.team_name || "the team"}. Redirecting you
                  to the dashboard...
                </CardDescription>
              </CardHeader>
            </Card>
          </div>
        </FullPageMain>
      </>
    );
  }

  // Invite preview
  return (
    <>
      <TopNav />
      <FullPageMain>
        <div className="flex items-center justify-center min-h-[60vh]">
          <Card className="w-full max-w-md">
            <CardHeader>
              <div className="flex items-center justify-center mb-4">
                <div className="rounded-full bg-primary/10 p-3">
                  <UserPlus className="h-8 w-8 text-primary" />
                </div>
              </div>
              <CardTitle className="text-center">Team Invitation</CardTitle>
              <CardDescription className="text-center">
                You've been invited to join a team on Maple AI
              </CardDescription>
            </CardHeader>
            <CardContent className="space-y-4">
              <div className="bg-muted rounded-lg p-4 space-y-2">
                <div>
                  <p className="text-sm text-muted-foreground">Team</p>
                  <p className="font-medium">{inviteData.team_name}</p>
                </div>
                {inviteData.invited_by_name && (
                  <div>
                    <p className="text-sm text-muted-foreground">Invited by</p>
                    <p className="font-medium">{inviteData.invited_by_name}</p>
                  </div>
                )}
                <div>
                  <p className="text-sm text-muted-foreground">Your email</p>
                  <p className="font-medium">{userEmail}</p>
                </div>
              </div>

              {error && (
                <Alert variant="destructive">
                  <AlertCircle className="h-4 w-4" />
                  <AlertDescription>{error}</AlertDescription>
                </Alert>
              )}

              <div className="flex gap-3">
                <Button
                  variant="outline"
                  onClick={handleDeclineInvite}
                  disabled={isProcessing}
                  className="flex-1"
                >
                  Decline
                </Button>
                <Button onClick={handleAcceptInvite} disabled={isProcessing} className="flex-1">
                  {isProcessing ? (
                    <>
                      <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                      Joining...
                    </>
                  ) : (
                    <>
                      <CheckCircle className="mr-2 h-4 w-4" />
                      Accept Invitation
                    </>
                  )}
                </Button>
              </div>

              <p className="text-xs text-center text-muted-foreground">
                By accepting this invitation, you'll join the team and gain access to shared
                resources.
              </p>
            </CardContent>
          </Card>
        </div>
      </FullPageMain>
    </>
  );
}
