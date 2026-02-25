import { useState, useEffect, useCallback } from "react";
import {
  User,
  CreditCard,
  Key,
  Users,
  Trash2,
  Info,
  LogOut,
  ArrowLeft,
  Settings,
  AlertCircle
} from "lucide-react";
import { useNavigate, useRouter } from "@tanstack/react-router";
import { useOpenSecret } from "@opensecret/react";
import { useQuery } from "@tanstack/react-query";
import { Sidebar, SidebarToggle } from "@/components/Sidebar";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { cn, useIsMobile } from "@/utils/utils";
import { useLocalState } from "@/state/useLocalState";
import { getBillingService } from "@/billing/billingService";
import { isIOS } from "@/utils/platform";
import type { TeamStatus } from "@/types/team";
import { ProfileSection } from "./ProfileSection";
import { SubscriptionSection } from "./SubscriptionSection";
import { ApiManagementSection } from "./ApiManagementSection";
import { TeamManagementSection } from "./TeamManagementSection";
import { DataPrivacySection } from "./DataPrivacySection";
import { AboutSection } from "./AboutSection";
import packageJson from "../../../package.json";

export type SettingsTab = "profile" | "subscription" | "api" | "team" | "data" | "about";

interface SettingsPageProps {
  initialTab?: string;
  creditsSuccess?: boolean;
}

const TAB_CONFIG: {
  id: SettingsTab;
  label: string;
  icon: React.ElementType;
  requiresTeam?: boolean;
  requiresApiAccess?: boolean;
}[] = [
  { id: "profile", label: "Profile", icon: User },
  { id: "subscription", label: "Subscription", icon: CreditCard },
  { id: "api", label: "API Management", icon: Key, requiresApiAccess: true },
  { id: "team", label: "Team", icon: Users, requiresTeam: true },
  { id: "data", label: "Data & Privacy", icon: Trash2 },
  { id: "about", label: "About", icon: Info }
];

export function SettingsPage({ initialTab, creditsSuccess }: SettingsPageProps) {
  const [activeTab, setActiveTab] = useState<SettingsTab>(
    isValidTab(initialTab) ? initialTab : "profile"
  );
  const isMobile = useIsMobile();
  const [isSidebarOpen, setIsSidebarOpen] = useState(!isMobile);
  const navigate = useNavigate();
  const router = useRouter();
  const os = useOpenSecret();
  const { billingStatus, setBillingStatus } = useLocalState();

  // Fetch billing status for direct navigation (when index.tsx hasn't loaded it)
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

  // Fetch team status
  const { data: teamStatus } = useQuery<TeamStatus>({
    queryKey: ["teamStatus"],
    queryFn: async () => {
      const billingService = getBillingService();
      return await billingService.getTeamStatus();
    },
    enabled: isTeamPlan && !!os.auth.user && !!billingStatus
  });

  // Fetch products with version check for iOS
  const isIOSPlatform = isIOS();
  const { data: products } = useQuery({
    queryKey: ["products-version-check", isIOSPlatform],
    queryFn: async () => {
      try {
        const billingService = getBillingService();
        if (isIOSPlatform) {
          const version = `v${packageJson.version}`;
          return await billingService.getProducts(version);
        }
        return await billingService.getProducts();
      } catch (error) {
        console.error("Error fetching products for version check:", error);
        return null;
      }
    },
    enabled: isIOSPlatform
  });

  // Determine if API Management should be shown
  const showApiManagement = (() => {
    if (!isIOSPlatform) return true;
    if (!products) return false;
    return products.some((product) => product.is_available !== false);
  })();

  // Show team setup alert
  const showTeamSetupAlert =
    isTeamPlan && teamStatus?.has_team_subscription && !teamStatus?.team_created;

  // Update tab when initialTab changes (e.g., from URL)
  useEffect(() => {
    if (initialTab && isValidTab(initialTab)) {
      setActiveTab(initialTab);
    }
  }, [initialTab]);

  const handleTabChange = useCallback(
    (tab: SettingsTab) => {
      setActiveTab(tab);
      // Update URL without full navigation
      navigate({
        to: "/settings",
        search: { tab },
        replace: true
      });
    },
    [navigate]
  );

  const handleBackToChat = useCallback(() => {
    navigate({ to: "/" });
  }, [navigate]);

  const toggleSidebar = useCallback(() => setIsSidebarOpen((prev) => !prev), []);

  async function signOut() {
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
  }

  // Filter tabs based on user's plan
  const visibleTabs = TAB_CONFIG.filter((tab) => {
    if (tab.requiresTeam && !isTeamPlan) return false;
    if (tab.requiresApiAccess && !showApiManagement) return false;
    return true;
  });

  // Ensure activeTab is always a visible tab (prevent showing hidden tab content)
  useEffect(() => {
    const isTabVisible = visibleTabs.some((tab) => tab.id === activeTab);
    if (!isTabVisible && visibleTabs.length > 0) {
      setActiveTab(visibleTabs[0].id);
    }
  }, [activeTab, visibleTabs]);

  return (
    <div className="grid h-dvh w-full grid-cols-1 md:grid-cols-[280px_1fr]">
      <Sidebar isOpen={isSidebarOpen} onToggle={toggleSidebar} />
      <main className="flex h-dvh flex-col bg-card/90 backdrop-blur-lg overflow-hidden">
        {/* Header */}
        <div className="h-14 flex items-center px-4 border-b border-input gap-3">
          {!isSidebarOpen && (
            <div className="fixed top-[9.5px] left-4 z-20">
              <SidebarToggle onToggle={toggleSidebar} />
            </div>
          )}
          <Button variant="ghost" size="sm" onClick={handleBackToChat} className="gap-2">
            <ArrowLeft className="h-4 w-4" />
            <span className="hidden sm:inline">Back to Chat</span>
          </Button>
          <div className="flex items-center gap-2">
            <Settings className="h-4 w-4 text-muted-foreground" />
            <h1 className="text-lg font-semibold">Settings</h1>
          </div>
        </div>

        {/* Content area */}
        <div className="flex-1 min-h-0 flex flex-col md:flex-row overflow-hidden">
          {/* Settings navigation - horizontal on mobile, vertical sidebar on desktop */}
          <nav
            className={cn(
              "flex-shrink-0 border-b md:border-b-0 md:border-r border-input bg-muted/30",
              "md:w-56 md:overflow-y-auto"
            )}
          >
            {/* Mobile: horizontal scrollable tabs */}
            <div className="flex md:hidden overflow-x-auto px-2 py-2 gap-1 scrollbar-hide">
              {visibleTabs.map((tab) => (
                <button
                  key={tab.id}
                  onClick={() => handleTabChange(tab.id)}
                  className={cn(
                    "flex items-center gap-1.5 px-3 py-2 rounded-md text-sm font-medium whitespace-nowrap transition-colors",
                    activeTab === tab.id
                      ? "bg-primary text-primary-foreground"
                      : "text-muted-foreground hover:text-foreground hover:bg-muted"
                  )}
                >
                  <tab.icon className="h-4 w-4" />
                  {tab.label}
                  {tab.id === "team" && showTeamSetupAlert && (
                    <AlertCircle className="h-3 w-3 text-amber-500" />
                  )}
                </button>
              ))}
            </div>

            {/* Desktop: vertical nav */}
            <div className="hidden md:flex flex-col py-4 px-3 gap-1">
              {visibleTabs.map((tab) => (
                <button
                  key={tab.id}
                  onClick={() => handleTabChange(tab.id)}
                  className={cn(
                    "flex items-center gap-3 px-3 py-2.5 rounded-md text-sm font-medium transition-colors text-left w-full",
                    activeTab === tab.id
                      ? "bg-primary text-primary-foreground"
                      : "text-muted-foreground hover:text-foreground hover:bg-muted"
                  )}
                >
                  <tab.icon className="h-4 w-4 flex-shrink-0" />
                  <span className="flex-1">{tab.label}</span>
                  {tab.id === "team" && showTeamSetupAlert && (
                    <Badge
                      variant="secondary"
                      className="py-0 px-1.5 text-xs bg-amber-500 text-white"
                    >
                      !
                    </Badge>
                  )}
                </button>
              ))}

              {/* Logout button at bottom of nav */}
              <div className="mt-auto pt-4 border-t border-input mt-4">
                <button
                  onClick={signOut}
                  className="flex items-center gap-3 px-3 py-2.5 rounded-md text-sm font-medium transition-colors text-left w-full text-destructive hover:bg-destructive/10"
                >
                  <LogOut className="h-4 w-4 flex-shrink-0" />
                  <span>Log out</span>
                </button>
              </div>
            </div>
          </nav>

          {/* Settings content */}
          <div className="flex-1 overflow-y-auto">
            <div className="max-w-2xl mx-auto px-4 sm:px-6 py-6 sm:py-8">
              {activeTab === "profile" && <ProfileSection />}
              {activeTab === "subscription" && <SubscriptionSection />}
              {activeTab === "api" && <ApiManagementSection creditsSuccess={creditsSuccess} />}
              {activeTab === "team" && <TeamManagementSection teamStatus={teamStatus} />}
              {activeTab === "data" && <DataPrivacySection />}
              {activeTab === "about" && <AboutSection />}
            </div>

            {/* Mobile logout button */}
            <div className="md:hidden px-4 pb-6">
              <div className="max-w-2xl mx-auto">
                <Button
                  variant="outline"
                  className="w-full border-destructive text-destructive hover:bg-destructive/10"
                  onClick={signOut}
                >
                  <LogOut className="mr-2 h-4 w-4" />
                  Log out
                </Button>
              </div>
            </div>
          </div>
        </div>
      </main>
    </div>
  );
}

function isValidTab(tab: string | undefined): tab is SettingsTab {
  return (
    tab !== undefined && ["profile", "subscription", "api", "team", "data", "about"].includes(tab)
  );
}
