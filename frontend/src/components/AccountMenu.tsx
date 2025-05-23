import { LogOut, Trash, User, CreditCard, ArrowUpCircle, Mail } from "lucide-react";

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
import { useQueryClient } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { getBillingService } from "@/billing/billingService";
import { useState } from "react";

function ConfirmDeleteDialog() {
  const { clearHistory } = useLocalState();
  const queryClient = useQueryClient();
  const navigate = useNavigate();

  async function handleDeleteHistory() {
    try {
      await clearHistory();
      console.log("History cleared");
    } catch (error) {
      console.error("Error clearing history:", error);
    } finally {
      queryClient.invalidateQueries({ queryKey: ["chatHistory"] });
      navigate({ to: "/" });
    }
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

  const hasStripeAccount = billingStatus?.stripe_customer_id !== null;
  const productName = billingStatus?.product_name || "";
  const isPro = productName.toLowerCase().includes("pro");
  const isStarter = productName.toLowerCase().includes("starter");
  const showUpgrade = !isPro;
  const showManage = (isPro || isStarter) && hasStripeAccount;

  const handleManageSubscription = async () => {
    if (!hasStripeAccount) return;

    try {
      setIsPortalLoading(true);
      const billingService = getBillingService();
      const url = await billingService.getPortalUrl();

      // Check if we're in a Tauri environment on iOS
      try {
        const isTauri = await import("@tauri-apps/api/core")
          .then((m) => m.isTauri())
          .catch(() => false);

        if (isTauri) {
          const { type } = await import("@tauri-apps/plugin-os");
          const platform = await type();
          const { invoke } = await import("@tauri-apps/api/core");

          if (platform === "ios") {
            console.log("[Billing] iOS detected, using opener plugin to launch Safari for portal");

            // Use the opener plugin directly - with NO fallback for iOS
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
        }
      } catch (err) {
        console.error("[Billing] Error checking platform:", err);
      }

      // Default browser opening for non-iOS platforms
      window.open(url, "_blank");
    } catch (error) {
      console.error("Error fetching portal URL:", error);
    } finally {
      setIsPortalLoading(false);
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
        <DropdownMenu>
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
              <Button variant="outline" className="gap-2">
                <User className="w-4 h-4" />
                Account
              </Button>
            </DropdownMenuTrigger>
          </div>
          <DropdownMenuContent className="w-[calc(280px-2rem)]">
            <DropdownMenuLabel>Maple AI</DropdownMenuLabel>
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
              <DropdownMenuItem asChild>
                <a href="mailto:support@opensecret.cloud">
                  <Mail className="mr-2 h-4 w-4" />
                  <span>Contact Us</span>
                </a>
              </DropdownMenuItem>
              <AlertDialogTrigger asChild>
                <DropdownMenuItem>
                  <Trash className="mr-2 h-4 w-4" />
                  <span>Delete History</span>
                </DropdownMenuItem>
              </AlertDialogTrigger>
            </DropdownMenuGroup>
            <DropdownMenuSeparator />
            <DropdownMenuItem onClick={signOut}>
              <LogOut className="mr-2 h-4 w-4" />
              <span>Log out</span>
            </DropdownMenuItem>
          </DropdownMenuContent>
          <AccountDialog />
          <ConfirmDeleteDialog />
        </DropdownMenu>
      </Dialog>
    </AlertDialog>
  );
}
