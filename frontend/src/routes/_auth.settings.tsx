import { useCallback, useEffect, useMemo, useState } from "react";
import { createFileRoute, Link, useNavigate, useRouter } from "@tanstack/react-router";
import { useOpenSecret } from "@opensecret/react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  ArrowLeft,
  CheckCircle,
  CreditCard,
  FileText,
  Info,
  Key,
  LogOut,
  Mail,
  Shield,
  Trash,
  User,
  Users
} from "lucide-react";

import { Sidebar, SidebarToggle } from "@/components/Sidebar";
import { ApiKeyDashboard } from "@/components/apikeys/ApiKeyDashboard";
import { ChangePasswordDialog } from "@/components/ChangePasswordDialog";
import { DeleteAccountDialog } from "@/components/DeleteAccountDialog";
import { PreferencesDialog } from "@/components/PreferencesDialog";
import { TeamDashboard } from "@/components/team/TeamDashboard";
import { TeamSetupDialog } from "@/components/team/TeamSetupDialog";
import { Alert, AlertDescription } from "@/components/ui/alert";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger
} from "@/components/ui/alert-dialog";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Separator } from "@/components/ui/separator";
import { getBillingService } from "@/billing/billingService";
import { useLocalState } from "@/state/useLocalState";
import type { TeamStatus } from "@/types/team";
import { cn, useIsMobile } from "@/utils/utils";
import { isMobile, isTauri } from "@/utils/platform";
import { openExternalUrlWithConfirmation } from "@/utils/openUrl";

const SETTINGS_TABS = ["account", "billing", "team", "api", "history", "about"] as const;
type SettingsTab = (typeof SETTINGS_TABS)[number];

type SettingsSearchParams = {
  tab?: SettingsTab;
  team_setup?: boolean;
  credits_success?: boolean;
};

function parseSettingsTab(value: unknown): SettingsTab | undefined {
  if (typeof value !== "string") return undefined;
  return (SETTINGS_TABS as readonly string[]).includes(value) ? (value as SettingsTab) : undefined;
}

function validateSearch(search: Record<string, unknown>): SettingsSearchParams {
  return {
    tab: parseSettingsTab(search.tab),
    team_setup: search?.team_setup === true || search?.team_setup === "true" ? true : undefined,
    credits_success:
      search?.credits_success === true || search?.credits_success === "true" ? true : undefined
  };
}

export const Route = createFileRoute("/_auth/settings")({
  component: SettingsPage,
  validateSearch
});

function SettingsPage() {
  const os = useOpenSecret();
  const router = useRouter();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const isMobileViewport = useIsMobile();
  const { setBillingStatus, billingStatus } = useLocalState();

  const { tab, team_setup, credits_success } = Route.useSearch();
  const [isSidebarOpen, setIsSidebarOpen] = useState(!isMobileViewport);
  const [showApiCreditSuccessMessage, setShowApiCreditSuccessMessage] = useState(false);
  const [autoOpenTeamSetup, setAutoOpenTeamSetup] = useState(false);

  const toggleSidebar = useCallback(() => setIsSidebarOpen((prev) => !prev), []);

  // Proactively fetch billing status for authenticated users
  useQuery({
    queryKey: ["billingStatus"],
    queryFn: async () => {
      const billingService = getBillingService();
      const status = await billingService.getBillingStatus();
      setBillingStatus(status);
      return status;
    },
    enabled: !!os.auth.user
  });

  const productName = billingStatus?.product_name || "";
  const isTeamPlan = productName.toLowerCase().includes("team");

  const { data: teamStatus } = useQuery<TeamStatus>({
    queryKey: ["teamStatus"],
    queryFn: async () => {
      const billingService = getBillingService();
      return await billingService.getTeamStatus();
    },
    enabled: isTeamPlan && !!os.auth.user && !!billingStatus
  });

  const effectiveTab: SettingsTab = useMemo(() => {
    if (team_setup) return "team";
    if (credits_success) return "api";
    return tab ?? "account";
  }, [tab, team_setup, credits_success]);

  // Support legacy/team flow query params by routing into the appropriate settings section.
  useEffect(() => {
    if (!team_setup || !os.auth.user) return;
    setAutoOpenTeamSetup(true);
    navigate({ to: "/settings", search: { tab: "team" }, replace: true });
  }, [team_setup, os.auth.user, navigate]);

  useEffect(() => {
    if (!credits_success || !os.auth.user) return;

    setShowApiCreditSuccessMessage(true);
    queryClient.invalidateQueries({ queryKey: ["apiCreditBalance"] });
    navigate({ to: "/settings", search: { tab: "api" }, replace: true });

    // Let ApiCreditsSection manage its own 5s hide timer; we just prevent re-show on remounts.
    const timer = setTimeout(() => setShowApiCreditSuccessMessage(false), 6000);
    return () => clearTimeout(timer);
  }, [credits_success, os.auth.user, navigate, queryClient]);

  const showTeamSetupAlert =
    isTeamPlan && teamStatus?.has_team_subscription && teamStatus?.team_created === false;

  const isMobilePlatform = isMobile();

  const signOut = useCallback(async () => {
    try {
      try {
        getBillingService().clearToken();
      } catch (error) {
        console.error("Error clearing billing token:", error);
        sessionStorage.removeItem("maple_billing_token");
      }

      await os.signOut();
      await router.invalidate();
      await router.navigate({ to: "/" });
    } catch (error) {
      console.error("Error during sign out:", error);
      window.location.href = "/";
    }
  }, [os, router]);

  const navItems: Array<{
    tab: SettingsTab;
    label: string;
    icon: React.ComponentType<{ className?: string }>;
    hidden?: boolean;
    badge?: React.ReactNode;
  }> = [
    { tab: "account", label: "Account", icon: User },
    { tab: "billing", label: "Billing", icon: CreditCard },
    {
      tab: "team",
      label: "Team",
      icon: Users,
      badge: showTeamSetupAlert ? (
        <Badge variant="secondary" className="py-0 px-1.5 text-[10px] bg-amber-500 text-white">
          Setup
        </Badge>
      ) : undefined
    },
    { tab: "api", label: "API", icon: Key, hidden: isMobilePlatform },
    { tab: "history", label: "History", icon: Trash },
    { tab: "about", label: "About", icon: Info }
  ];

  return (
    <div
      className={cn([
        "grid h-dvh min-h-0 w-full grid-cols-1 overflow-hidden",
        isSidebarOpen ? "md:grid-cols-[280px_1fr]" : ""
      ])}
    >
      <Sidebar isOpen={isSidebarOpen} onToggle={toggleSidebar} />

      <div className="flex flex-col flex-1 min-w-0 min-h-0 bg-background overflow-hidden relative">
        {!isSidebarOpen && (
          <div className="fixed top-[9.5px] left-4 z-20">
            <SidebarToggle onToggle={toggleSidebar} />
          </div>
        )}

        <div className="h-14 flex items-center px-4 border-b">
          <div className="flex items-center gap-2">
            <Button
              variant="ghost"
              size="sm"
              className="gap-2"
              onClick={() => navigate({ to: "/" })}
            >
              <ArrowLeft className="h-4 w-4" />
              Back
            </Button>
            <Separator orientation="vertical" className="h-6" />
            <h1 className="text-base font-semibold">Settings</h1>
          </div>

          <div className="ml-auto flex items-center gap-2">
            <Button variant="outline" size="sm" className="gap-2" onClick={signOut}>
              <LogOut className="h-4 w-4" />
              Log out
            </Button>
          </div>
        </div>

        <div className="flex-1 min-h-0 overflow-y-auto">
          <div className="mx-auto w-full max-w-5xl p-4 md:p-6">
            <div className="grid gap-6 md:grid-cols-[220px_1fr]">
              <nav className="space-y-1">
                {navItems
                  .filter((i) => !i.hidden)
                  .map((item) => (
                    <Button
                      key={item.tab}
                      variant={effectiveTab === item.tab ? "secondary" : "ghost"}
                      className="w-full justify-start gap-2"
                      asChild
                    >
                      <Link to="/settings" search={{ tab: item.tab }}>
                        <item.icon className="h-4 w-4" />
                        <span className="flex-1 text-left">{item.label}</span>
                        {item.badge}
                      </Link>
                    </Button>
                  ))}
              </nav>

              <div className="min-w-0">
                {effectiveTab === "account" && <AccountSection onSignOut={signOut} />}
                {effectiveTab === "billing" && (
                  <BillingSection
                    billingStatus={billingStatus}
                    isMobilePlatform={isMobilePlatform}
                  />
                )}
                {effectiveTab === "team" && (
                  <TeamSection
                    billingStatus={billingStatus}
                    teamStatus={teamStatus}
                    autoOpenSetup={autoOpenTeamSetup}
                  />
                )}
                {effectiveTab === "api" && (
                  <ApiSection
                    isMobilePlatform={isMobilePlatform}
                    showCreditSuccessMessage={showApiCreditSuccessMessage}
                  />
                )}
                {effectiveTab === "history" && <HistorySection />}
                {effectiveTab === "about" && <AboutSection />}
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}

function AccountSection({ onSignOut }: { onSignOut: () => Promise<void> }) {
  const os = useOpenSecret();
  const { billingStatus } = useLocalState();
  const [isChangePasswordOpen, setIsChangePasswordOpen] = useState(false);
  const [isPreferencesOpen, setIsPreferencesOpen] = useState(false);
  const [isDeleteAccountOpen, setIsDeleteAccountOpen] = useState(false);
  const [verificationStatus, setVerificationStatus] = useState<"unverified" | "pending">(
    "unverified"
  );

  const isGuestUser = os.auth.user?.user.login_method?.toLowerCase() === "guest";
  const isEmailUser = os.auth.user?.user.login_method === "email";
  const canChangePassword = isEmailUser || isGuestUser;

  const handleResendVerification = async () => {
    try {
      await os.requestNewVerificationEmail();
      setVerificationStatus("pending");
    } catch (error) {
      console.error("Failed to resend verification email:", error);
    }
  };

  return (
    <div className="space-y-6">
      <Card>
        <CardHeader>
          <CardTitle className="text-base">Profile</CardTitle>
          <CardDescription>Your account details and login status.</CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          {!isGuestUser && (
            <div className="space-y-2">
              <Label>Email</Label>
              <div className="flex items-center gap-2">
                <Input value={os.auth.user?.user.email || ""} disabled />
                {os.auth.user?.user.email_verified ? (
                  <CheckCircle className="h-4 w-4 text-green-600 dark:text-green-500" />
                ) : (
                  <span className="text-xs text-muted-foreground">Unverified</span>
                )}
              </div>
              {!os.auth.user?.user.email_verified && (
                <div className="text-sm text-muted-foreground">
                  {verificationStatus === "unverified" ? (
                    <button
                      type="button"
                      onClick={handleResendVerification}
                      className="text-primary hover:underline"
                    >
                      Resend verification email
                    </button>
                  ) : (
                    "Pending — check your inbox"
                  )}
                </div>
              )}
            </div>
          )}

          <div className="space-y-2">
            <Label>Plan</Label>
            <div className="flex items-center gap-2">
              <Badge variant="secondary">
                {billingStatus ? `${billingStatus.product_name} Plan` : "Loading..."}
              </Badge>
            </div>
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle className="text-base">Security</CardTitle>
          <CardDescription>Password and personal preferences.</CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-2">
          <Button variant="outline" onClick={() => setIsPreferencesOpen(true)}>
            User Preferences
          </Button>
          {canChangePassword && (
            <Button onClick={() => setIsChangePasswordOpen(true)}>Change Password</Button>
          )}
          <Button
            variant="outline"
            className="border-destructive text-destructive hover:bg-destructive/10"
            onClick={() => setIsDeleteAccountOpen(true)}
          >
            Delete Account
          </Button>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle className="text-base">Session</CardTitle>
          <CardDescription>Sign out of your account on this device.</CardDescription>
        </CardHeader>
        <CardContent>
          <Button variant="outline" className="gap-2" onClick={onSignOut}>
            <LogOut className="h-4 w-4" />
            Log out
          </Button>
        </CardContent>
      </Card>

      {canChangePassword && (
        <ChangePasswordDialog open={isChangePasswordOpen} onOpenChange={setIsChangePasswordOpen} />
      )}
      <PreferencesDialog open={isPreferencesOpen} onOpenChange={setIsPreferencesOpen} />
      <DeleteAccountDialog open={isDeleteAccountOpen} onOpenChange={setIsDeleteAccountOpen} />
    </div>
  );
}

function BillingSection({
  billingStatus,
  isMobilePlatform
}: {
  billingStatus: ReturnType<typeof useLocalState>["billingStatus"];
  isMobilePlatform: boolean;
}) {
  const [isPortalLoading, setIsPortalLoading] = useState(false);

  const productName = billingStatus?.product_name || "";
  const hasStripeAccount = billingStatus?.stripe_customer_id !== null;
  const isPro = productName.toLowerCase().includes("pro");
  const isMax = productName.toLowerCase().includes("max");
  const isStarter = productName.toLowerCase().includes("starter");
  const isTeamPlan = productName.toLowerCase().includes("team");
  const showUpgrade = !isMax && !isTeamPlan;
  const showManage = (isPro || isMax || isStarter || isTeamPlan) && hasStripeAccount;

  const handleManageSubscription = async () => {
    if (!hasStripeAccount) return;

    try {
      setIsPortalLoading(true);
      const billingService = getBillingService();
      const url = await billingService.getPortalUrl();

      if (isMobilePlatform) {
        const { invoke } = await import("@tauri-apps/api/core");
        await invoke("plugin:opener|open_url", { url }).catch((err: Error) => {
          console.error("[Billing] Failed to open external browser:", err);
          alert("Failed to open browser. Please try again.");
        });
        await new Promise((resolve) => setTimeout(resolve, 300));
        return;
      }

      window.open(url, "_blank");
    } catch (error) {
      console.error("Error fetching portal URL:", error);
    } finally {
      setIsPortalLoading(false);
    }
  };

  return (
    <div className="space-y-6">
      <Card>
        <CardHeader>
          <CardTitle className="text-base">Plan</CardTitle>
          <CardDescription>Manage your subscription and billing.</CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="flex items-center justify-between gap-2">
            <div className="min-w-0">
              <div className="font-medium truncate">
                {billingStatus ? `${billingStatus.product_name} Plan` : "Loading..."}
              </div>
              {billingStatus?.current_period_end && (
                <div className="text-sm text-muted-foreground">
                  {billingStatus.payment_provider === "subscription_pass" ||
                  billingStatus.payment_provider === "zaprite"
                    ? "Expires on "
                    : "Renews on "}
                  {new Date(Number(billingStatus.current_period_end) * 1000).toLocaleDateString(
                    undefined,
                    {
                      year: "numeric",
                      month: "long",
                      day: "numeric"
                    }
                  )}
                </div>
              )}
            </div>
            {billingStatus && (
              <Badge variant="secondary" className="shrink-0">
                {billingStatus.payment_provider}
              </Badge>
            )}
          </div>

          <div className="flex flex-col sm:flex-row gap-2">
            {showUpgrade && (
              <Button asChild className="gap-2">
                <Link to="/pricing">
                  <CreditCard className="h-4 w-4" />
                  View Pricing
                </Link>
              </Button>
            )}
            {showManage && (
              <Button
                variant="outline"
                className="gap-2"
                onClick={handleManageSubscription}
                disabled={isPortalLoading}
              >
                <CreditCard className="h-4 w-4" />
                {isPortalLoading ? "Loading..." : "Manage Subscription"}
              </Button>
            )}
          </div>
        </CardContent>
      </Card>
    </div>
  );
}

function TeamSection({
  billingStatus,
  teamStatus,
  autoOpenSetup
}: {
  billingStatus: ReturnType<typeof useLocalState>["billingStatus"];
  teamStatus?: TeamStatus;
  autoOpenSetup: boolean;
}) {
  const productName = billingStatus?.product_name || "";
  const isTeamPlan = productName.toLowerCase().includes("team");

  const needsSetup = teamStatus?.has_team_subscription && teamStatus?.team_created === false;
  const [isSetupOpen, setIsSetupOpen] = useState(false);
  const [hasAutoOpened, setHasAutoOpened] = useState(false);

  useEffect(() => {
    if (!autoOpenSetup || !needsSetup || hasAutoOpened) return;
    setIsSetupOpen(true);
    setHasAutoOpened(true);
  }, [autoOpenSetup, needsSetup, hasAutoOpened]);

  if (!isTeamPlan) {
    return (
      <Card>
        <CardHeader>
          <CardTitle className="text-base">Teams</CardTitle>
          <CardDescription>
            Upgrade to a Team plan to manage members, seats, and shared usage.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <Button asChild>
            <Link to="/teams">Learn about Teams</Link>
          </Button>
        </CardContent>
      </Card>
    );
  }

  return (
    <div className="space-y-4">
      {needsSetup && (
        <Card>
          <CardHeader>
            <CardTitle className="text-base">Set up your team</CardTitle>
            <CardDescription>
              You have an active team subscription. Create your team to start inviting members.
            </CardDescription>
          </CardHeader>
          <CardContent className="space-y-3">
            <Button onClick={() => setIsSetupOpen(true)}>Create Team</Button>
            <TeamSetupDialog
              open={isSetupOpen}
              onOpenChange={setIsSetupOpen}
              teamStatus={teamStatus}
            />
          </CardContent>
        </Card>
      )}

      {!needsSetup && (
        <Card className="p-6">
          <TeamDashboard teamStatus={teamStatus} />
        </Card>
      )}
    </div>
  );
}

function ApiSection({
  isMobilePlatform,
  showCreditSuccessMessage
}: {
  isMobilePlatform: boolean;
  showCreditSuccessMessage: boolean;
}) {
  if (isMobilePlatform) {
    return (
      <Card>
        <CardHeader>
          <CardTitle className="text-base">API</CardTitle>
          <CardDescription>API management is currently available on desktop/web.</CardDescription>
        </CardHeader>
      </Card>
    );
  }

  return (
    <Card className="p-6">
      <ApiKeyDashboard showCreditSuccessMessage={showCreditSuccessMessage} />
    </Card>
  );
}

function HistorySection() {
  const { clearHistory } = useLocalState();
  const os = useOpenSecret();
  const queryClient = useQueryClient();
  const navigate = useNavigate();

  const handleDeleteHistory = async () => {
    try {
      await clearHistory();
    } catch (error) {
      console.error("Error clearing history:", error);
    }

    try {
      const conversations = await os.listConversations({ limit: 1 });
      if (conversations.data && conversations.data.length > 0) {
        await os.deleteConversations();
      }
    } catch (error) {
      console.error("Error deleting conversations:", error);
    }

    queryClient.invalidateQueries({ queryKey: ["chatHistory"] });
    queryClient.invalidateQueries({ queryKey: ["conversations"] });
    queryClient.invalidateQueries({ queryKey: ["archivedChats"] });
    navigate({ to: "/" });
  };

  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-base">History</CardTitle>
        <CardDescription>Delete your local and server chat history.</CardDescription>
      </CardHeader>
      <CardContent>
        <AlertDialog>
          <AlertDialogTrigger asChild>
            <Button variant="destructive" className="gap-2">
              <Trash className="h-4 w-4" />
              Delete History
            </Button>
          </AlertDialogTrigger>
          <AlertDialogContent>
            <AlertDialogHeader>
              <AlertDialogTitle>Are you sure?</AlertDialogTitle>
              <AlertDialogDescription>
                This will delete your entire chat history (local + server).
              </AlertDialogDescription>
            </AlertDialogHeader>
            <AlertDialogFooter>
              <AlertDialogCancel>Cancel</AlertDialogCancel>
              <AlertDialogAction onClick={handleDeleteHistory}>Delete</AlertDialogAction>
            </AlertDialogFooter>
          </AlertDialogContent>
        </AlertDialog>
      </CardContent>
    </Card>
  );
}

function AboutSection() {
  return (
    <div className="space-y-4">
      <Card>
        <CardHeader>
          <CardTitle className="text-base">About Maple</CardTitle>
          <CardDescription>Product information and legal links.</CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-2">
          <Button variant="outline" asChild className="justify-start gap-2">
            <Link to="/about">
              <Info className="h-4 w-4" />
              About Maple
            </Link>
          </Button>
          <Button
            variant="outline"
            className="justify-start gap-2"
            onClick={() => openExternalUrlWithConfirmation("https://opensecret.cloud/privacy")}
          >
            <Shield className="h-4 w-4" />
            Privacy Policy
          </Button>
          <Button
            variant="outline"
            className="justify-start gap-2"
            onClick={() => openExternalUrlWithConfirmation("https://opensecret.cloud/terms")}
          >
            <FileText className="h-4 w-4" />
            Terms of Service
          </Button>
          <Button
            variant="outline"
            className="justify-start gap-2"
            onClick={() => openExternalUrlWithConfirmation("mailto:support@opensecret.cloud")}
          >
            <Mail className="h-4 w-4" />
            Contact Support
          </Button>
        </CardContent>
      </Card>

      {isTauri() && (
        <Alert>
          <AlertDescription>External links open in your system browser.</AlertDescription>
        </Alert>
      )}
    </div>
  );
}
