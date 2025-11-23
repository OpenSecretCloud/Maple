import {
  LogOut,
  Trash,
  User,
  CreditCard,
  ArrowUpCircle,
  Mail,
  Users,
  AlertCircle,
  Key,
  Info,
  Shield,
  FileText,
  ChevronLeft
} from "lucide-react";
import { isMobile } from "@/utils/platform";

import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger
} from "@/components/ui/dropdown-menu";
import { useOpenSecret } from "@opensecret/react";
import { useNavigate, useRouter } from "@tanstack/react-router";
import { Dialog, DialogTrigger } from "./ui/dialog";
import { AccountDialog } from "./AccountDialog";
import { CreditUsage } from "./CreditUsage";
import { Badge } from "./ui/badge";

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
import { useLocalState } from "@/state/useLocalState";
import { useQueryClient, useQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { getBillingService } from "@/billing/billingService";
import { useState } from "react";
import type { TeamStatus } from "@/types/team";
import { TeamManagementDialog } from "@/components/team/TeamManagementDialog";
import { ApiKeyManagementDialog } from "@/components/apikeys/ApiKeyManagementDialog";

function ConfirmDeleteDialog() {
  const { clearHistory } = useLocalState();
  const os = useOpenSecret();
  const queryClient = useQueryClient();
  const navigate = useNavigate();

  async function handleDeleteHistory() {
    // 1. Delete archived chats (KV)
    try {
      await clearHistory();
      console.log("History (KV) cleared");
    } catch (error) {
      console.error("Error clearing history:", error);
      // Continue to delete server conversations even if this fails
    }

    // 2. Delete server conversations (API) if any exist
    try {
      // Check if we have any conversations to delete
      const conversations = await os.listConversations({ limit: 1 });
      if (conversations.data && conversations.data.length > 0) {
        await os.deleteConversations();
        console.log("Server conversations deleted");
      }
    } catch (e) {
      console.error("Error deleting conversations:", e);
    }

    // Always refresh UI and navigate home
    queryClient.invalidateQueries({ queryKey: ["chatHistory"] });
    queryClient.invalidateQueries({ queryKey: ["conversations"] });
    queryClient.invalidateQueries({ queryKey: ["archivedChats"] });
    navigate({ to: "/" });
  }

  return (
    <AlertDialogContent>
      <AlertDialogHeader>
        <AlertDialogTitle>Are you sure?</AlertDialogTitle>
        <AlertDialogDescription>This will delete your entire chat history.</AlertDialogDescription>
      </AlertDialogHeader>
      <AlertDialogFooter>
        <AlertDialogCancel>Cancel</AlertDialogCancel>
        <AlertDialogAction onClick={handleDeleteHistory}>Delete</AlertDialogAction>
      </AlertDialogFooter>
    </AlertDialogContent>
  );
}

export function AccountMenu() {
  const os = useOpenSecret();
  const router = useRouter();
  const { billingStatus } = useLocalState();
  const [isPortalLoading, setIsPortalLoading] = useState(false);
  const [isTeamDialogOpen, setIsTeamDialogOpen] = useState(false);
  const [isApiKeyDialogOpen, setIsApiKeyDialogOpen] = useState(false);
  const [showAboutMenu, setShowAboutMenu] = useState(false);

  const hasStripeAccount = billingStatus?.stripe_customer_id !== null;
  const productName = billingStatus?.product_name || "";
  const isPro = productName.toLowerCase().includes("pro");
  const isMax = productName.toLowerCase().includes("max");
  const isStarter = productName.toLowerCase().includes("starter");
  const isTeamPlan = productName.toLowerCase().includes("team");
  const showUpgrade = !isMax && !isTeamPlan;
  const showManage = (isPro || isMax || isStarter || isTeamPlan) && hasStripeAccount;

  // Fetch team status if user has team plan
  const { data: teamStatus } = useQuery<TeamStatus>({
    queryKey: ["teamStatus"],
    queryFn: async () => {
      const billingService = getBillingService();
      return await billingService.getTeamStatus();
    },
    enabled: isTeamPlan && !!os.auth.user && !!billingStatus
  });

  // Show alert badge if user has team plan but hasn't created team yet
  const showTeamSetupAlert =
    isTeamPlan && teamStatus?.has_team_subscription && !teamStatus?.team_created;

  const handleManageSubscription = async () => {
    if (!hasStripeAccount) return;

    try {
      setIsPortalLoading(true);
      const billingService = getBillingService();
      const url = await billingService.getPortalUrl();

      // Check if we're on a mobile platform
      if (isMobile()) {
        console.log(
          "[Billing] Mobile platform detected, using opener plugin to launch external browser for portal"
        );

        const { invoke } = await import("@tauri-apps/api/core");

        // Use the opener plugin directly - with NO fallback for mobile platforms
        await invoke("plugin:opener|open_url", { url })
          .then(() => {
            console.log("[Billing] Successfully opened portal URL in external browser");
          })
          .catch((err: Error) => {
            console.error("[Billing] Failed to open external browser:", err);
            alert("Failed to open browser. Please try again.");
          });

        // Add a small delay to ensure the browser has time to open
        await new Promise((resolve) => setTimeout(resolve, 300));
        return;
      }

      // Default browser opening for non-mobile platforms
      window.open(url, "_blank");
    } catch (error) {
      console.error("Error fetching portal URL:", error);
    } finally {
      setIsPortalLoading(false);
    }
  };

  const handleOpenExternalUrl = async (url: string) => {
    try {
      // Check if we're on a mobile platform
      if (isMobile()) {
        const { invoke } = await import("@tauri-apps/api/core");
        await invoke("plugin:opener|open_url", { url })
          .then(() => {
            console.log("[External Link] Successfully opened URL in external browser");
          })
          .catch((err: Error) => {
            console.error("[External Link] Failed to open external browser:", err);
            alert("Failed to open browser. Please try again.");
          });
      } else {
        // Default browser opening for non-mobile platforms
        window.open(url, "_blank", "noopener,noreferrer");
      }
    } catch (error) {
      console.error("Error opening external URL:", error);
      // Fallback to window.open
      window.open(url, "_blank", "noopener,noreferrer");
    }
  };

  async function signOut() {
    try {
      // Try to clear billing token first
      try {
        getBillingService().clearToken();
      } catch (error) {
        console.error("Error clearing billing token:", error);
        // Fallback to direct session storage removal if billing service fails
        sessionStorage.removeItem("maple_billing_token");
      }

      // Sign out from OpenSecret
      await os.signOut();

      // Navigate after everything is done
      await router.invalidate();
      await router.navigate({ to: "/" });
    } catch (error) {
      console.error("Error during sign out:", error);
      // Force reload as last resort
      window.location.href = "/";
    }
  }

  return (
    <AlertDialog>
      <Dialog>
        <DropdownMenu onOpenChange={(open) => !open && setShowAboutMenu(false)}>
          <div className="flex flex-col gap-2">
            <Link to="/pricing" className="self-end">
              <Badge
                variant="secondary"
                className="bg-[hsl(var(--primary))] text-[hsl(var(--primary-foreground))] hover:bg-[hsl(var(--primary))]/80 dark:bg-white dark:text-[hsl(var(--background))] dark:hover:bg-white/80 transition-colors cursor-pointer uppercase"
              >
                {billingStatus ? `${billingStatus.product_name} Plan` : "Loading..."}
              </Badge>
            </Link>
            <CreditUsage />
            <DropdownMenuTrigger asChild>
              <Button variant="outline" className="gap-2 relative">
                <User className="w-4 h-4" />
                Account
                {showTeamSetupAlert && (
                  <AlertCircle className="absolute -top-1 -right-1 h-4 w-4 text-amber-500 bg-background rounded-full" />
                )}
              </Button>
            </DropdownMenuTrigger>
          </div>
          <DropdownMenuContent className="w-[calc(280px-2rem)] overflow-hidden">
            <div className="relative">
              <div
                className={`transition-transform duration-300 ease-in-out ${
                  showAboutMenu ? "-translate-x-full" : "translate-x-0"
                }`}
              >
                <DropdownMenuLabel>{teamStatus?.team_name || "Maple AI"}</DropdownMenuLabel>
                <DropdownMenuSeparator />
                <DropdownMenuGroup>
                  <DialogTrigger asChild>
                    <DropdownMenuItem>
                      <User className="mr-2 h-4 w-4" />
                      <span>Profile</span>
                    </DropdownMenuItem>
                  </DialogTrigger>
                  {showUpgrade && (
                    <DropdownMenuItem asChild>
                      <Link to="/pricing">
                        <ArrowUpCircle className="mr-2 h-4 w-4" />
                        <span>Upgrade your plan</span>
                      </Link>
                    </DropdownMenuItem>
                  )}
                  {showManage && (
                    <DropdownMenuItem onClick={handleManageSubscription} disabled={isPortalLoading}>
                      <CreditCard className="mr-2 h-4 w-4" />
                      <span>{isPortalLoading ? "Loading..." : "Manage Subscription"}</span>
                    </DropdownMenuItem>
                  )}
                  {isTeamPlan && (
                    <DropdownMenuItem onClick={() => setIsTeamDialogOpen(true)}>
                      <div className="flex items-center justify-between w-full">
                        <div className="flex items-center">
                          <Users className="mr-2 h-4 w-4" />
                          <span>Manage Team</span>
                        </div>
                        {showTeamSetupAlert && (
                          <Badge
                            variant="secondary"
                            className="py-0 px-1.5 text-xs bg-amber-500 text-white"
                          >
                            Setup Required
                          </Badge>
                        )}
                      </div>
                    </DropdownMenuItem>
                  )}
                  {!isMobile() && (
                    <DropdownMenuItem onClick={() => setIsApiKeyDialogOpen(true)}>
                      <Key className="mr-2 h-4 w-4" />
                      <span>API Management</span>
                    </DropdownMenuItem>
                  )}
                  <AlertDialogTrigger asChild>
                    <DropdownMenuItem>
                      <Trash className="mr-2 h-4 w-4" />
                      <span>Delete History</span>
                    </DropdownMenuItem>
                  </AlertDialogTrigger>
                </DropdownMenuGroup>
                <DropdownMenuSeparator />
                <DropdownMenuGroup>
                  <DropdownMenuItem
                    onClick={(e) => {
                      e.preventDefault();
                      setShowAboutMenu(true);
                    }}
                    onSelect={(e) => {
                      e.preventDefault();
                    }}
                  >
                    <Info className="mr-2 h-4 w-4" />
                    <span>About Us</span>
                  </DropdownMenuItem>
                </DropdownMenuGroup>
                <DropdownMenuSeparator />
                <DropdownMenuItem onClick={signOut}>
                  <LogOut className="mr-2 h-4 w-4" />
                  <span>Log out</span>
                </DropdownMenuItem>
              </div>

              {/* About Us Submenu */}
              <div
                className={`absolute top-0 left-0 w-full transition-transform duration-300 ease-in-out ${
                  showAboutMenu ? "translate-x-0" : "translate-x-full"
                }`}
              >
                <DropdownMenuGroup>
                  <DropdownMenuItem
                    onClick={(e) => {
                      e.preventDefault();
                      setShowAboutMenu(false);
                    }}
                    onSelect={(e) => {
                      e.preventDefault();
                    }}
                  >
                    <ChevronLeft className="mr-2 h-4 w-4" />
                    <span>Back</span>
                  </DropdownMenuItem>
                </DropdownMenuGroup>
                <DropdownMenuSeparator />
                <DropdownMenuGroup>
                  <DropdownMenuItem asChild>
                    <Link to="/about">
                      <Info className="mr-2 h-4 w-4" />
                      <span>About Maple</span>
                    </Link>
                  </DropdownMenuItem>
                  <DropdownMenuItem
                    onClick={(e) => {
                      e.preventDefault();
                      handleOpenExternalUrl("https://opensecret.cloud/privacy");
                    }}
                    onSelect={(e) => {
                      e.preventDefault();
                    }}
                  >
                    <Shield className="mr-2 h-4 w-4" />
                    <span>Privacy Policy</span>
                  </DropdownMenuItem>
                  <DropdownMenuItem
                    onClick={(e) => {
                      e.preventDefault();
                      handleOpenExternalUrl("https://opensecret.cloud/terms");
                    }}
                    onSelect={(e) => {
                      e.preventDefault();
                    }}
                  >
                    <FileText className="mr-2 h-4 w-4" />
                    <span>Terms of Service</span>
                  </DropdownMenuItem>
                  <DropdownMenuItem asChild>
                    <a href="mailto:support@opensecret.cloud">
                      <Mail className="mr-2 h-4 w-4" />
                      <span>Contact Us</span>
                    </a>
                  </DropdownMenuItem>
                </DropdownMenuGroup>
              </div>
            </div>
          </DropdownMenuContent>
          <AccountDialog />
          <ConfirmDeleteDialog />
          <TeamManagementDialog
            open={isTeamDialogOpen}
            onOpenChange={setIsTeamDialogOpen}
            teamStatus={teamStatus}
          />
          <ApiKeyManagementDialog open={isApiKeyDialogOpen} onOpenChange={setIsApiKeyDialogOpen} />
        </DropdownMenu>
      </Dialog>
    </AlertDialog>
  );
}
