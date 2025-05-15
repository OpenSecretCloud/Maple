import {
  Search,
  SquarePenIcon,
  PanelRightClose,
  PanelRightOpen,
  XCircle,
  // FolderIcon, // Unused icon
  FolderPlusIcon,
  ChevronDown,
  ChevronRight
} from "lucide-react";
import { Button } from "./ui/button";
import { useLocation, useRouter } from "@tanstack/react-router";
import { ChatHistoryList } from "./ChatHistoryList";
import { AccountMenu } from "./AccountMenu";
import { useRef, useEffect, KeyboardEvent, useState } from "react";
import { cn, useClickOutside, useIsMobile } from "@/utils/utils";
import { Input } from "./ui/input";
import { useLocalState } from "@/state/useLocalState";
import { ProjectsSidebar } from "./ProjectsSidebar";
import { ProjectDialog } from "./ProjectDialog";
import { Separator } from "./ui/separator";

export function Sidebar({
  chatId,
  projectId,
  isOpen,
  onToggle
}: {
  chatId?: string;
  projectId?: string;
  isOpen: boolean;
  onToggle: () => void;
}) {
  const router = useRouter();
  const location = useLocation();
  const { searchQuery, setSearchQuery, isSearchVisible, setIsSearchVisible } = useLocalState();
  const searchInputRef = useRef<HTMLInputElement>(null);
  const [isProjectsExpanded, setIsProjectsExpanded] = useState(true);
  const [isProjectDialogOpen, setIsProjectDialogOpen] = useState(false);

  // Use the centralized hook for mobile detection once
  const isMobile = useIsMobile();

  async function handleAddChat() {
    // If sidebar is open, close it
    if (isOpen) {
      onToggle();
    }
    // If we're already on "/", focus the chat box
    if (location.pathname === "/") {
      document.getElementById("message")?.focus();
    } else {
      try {
        await router.navigate({ to: `/` });
        // Ensure element is available after navigation
        setTimeout(() => document.getElementById("message")?.focus(), 0);
      } catch (error) {
        console.error("Navigation failed:", error);
      }
    }
  }

  // Get the mobile status once at component level

  const navigateToProject = async (projectId: string) => {
    try {
      await router.navigate({ to: `/project/$projectId`, params: { projectId } });
      if (isOpen && isMobile) {
        onToggle();
      }
    } catch (error) {
      console.error("Navigation to project failed:", error);
    }
  };

  const toggleSearch = () => {
    setIsSearchVisible(!isSearchVisible);
    if (!isSearchVisible) {
      // Focus the search input when it becomes visible
      setTimeout(() => searchInputRef.current?.focus(), 0);
    } else {
      // Clear search when hiding
      setSearchQuery("");
    }
  };

  const clearSearch = () => {
    setSearchQuery("");
    searchInputRef.current?.focus();
  };

  const handleKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "Escape") {
      clearSearch();
    }
  };

  const sidebarRef = useRef<HTMLDivElement>(null);

  // Modified click outside handler to ignore clicks in dropdowns and dialogs
  useClickOutside(sidebarRef, (event: MouseEvent | TouchEvent) => {
    if (isOpen) {
      // Check if the click was inside a dropdown or dialog
      const target = event.target as HTMLElement;
      const isInDropdown = target.closest('[role="menu"]');
      const isInDialog = target.closest('[role="dialog"]');
      const isInAlertDialog = target.closest('[role="alertdialog"]');

      if (!isInDropdown && !isInDialog && !isInAlertDialog) {
        onToggle();
      }
    }
  });

  // Already have isMobile from above

  // This effect closes the sidebar on mobile when navigating,
  // but preserves search state between navigations
  useEffect(() => {
    const unsubscribe = router.subscribe("onResolved", () => {
      // On mobile: close the sidebar when navigating to any page
      // On desktop: keep the sidebar open
      if (isOpen && isMobile) {
        // Always close sidebar on mobile when navigating to preserve screen real estate
        onToggle();
      }
    });

    return () => {
      unsubscribe();
    };
  }, [router, isOpen, onToggle, isMobile]);

  return (
    <div
      ref={sidebarRef}
      className={cn([
        "fixed md:static z-10 h-full overflow-y-hidden",
        isOpen ? "block w-[280px]" : "hidden md:block md:w-[280px]"
      ])}
    >
      <div className="h-full border-r border-input dark:bg-background bg-[hsl(var(--footer-bg))] backdrop-blur-lg flex flex-col gap-2 px-4 py-4 md:py-6 items-stretch w-[280px]">
        <div className="flex justify-between items-center mb-1">
          <Button variant="outline" size="icon" className="md:w-full gap-2" onClick={handleAddChat}>
            <SquarePenIcon className="w-4 h-4" />
            <span className="hidden md:block">New Chat</span>
          </Button>
          <Button variant="outline" size="icon" className="md:hidden" onClick={onToggle}>
            <PanelRightOpen className="h-4 w-4" />
          </Button>
        </div>

        {/* Projects Section */}
        <div className="flex items-center justify-between mt-1">
          <button
            className="flex items-center gap-1 text-sm font-semibold"
            onClick={() => setIsProjectsExpanded(!isProjectsExpanded)}
          >
            {isProjectsExpanded ? (
              <ChevronDown className="h-4 w-4" />
            ) : (
              <ChevronRight className="h-4 w-4" />
            )}
            Projects
          </button>
          <Button
            variant="ghost"
            size="icon"
            className="h-6 w-6"
            onClick={() => setIsProjectDialogOpen(true)}
            title="Create new project"
          >
            <FolderPlusIcon className="h-4 w-4" />
          </Button>
        </div>

        {isProjectsExpanded && (
          <div className="pl-2">
            <ProjectsSidebar
              currentProjectId={projectId}
              onProjectSelect={navigateToProject}
              onCreateProject={() => setIsProjectDialogOpen(true)}
            />
          </div>
        )}

        <Separator className="my-2" />

        {/* History Section */}
        <div className="flex justify-between items-center">
          <h2 className="font-semibold text-sm">History</h2>
          <Button
            variant="ghost"
            size="icon"
            className="h-7 w-7"
            onClick={toggleSearch}
            aria-label={isSearchVisible ? "Hide search" : "Search chat history"}
          >
            <Search className="h-4 w-4" />
          </Button>
        </div>

        {isSearchVisible && (
          <div className="relative transition-all duration-200 ease-in-out">
            <Input
              ref={searchInputRef}
              type="text"
              placeholder="Search chat titles..."
              className="pl-2 pr-8 h-9"
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              onKeyDown={handleKeyDown}
              aria-label="Search chat titles"
            />
            {searchQuery && (
              <button
                onClick={clearSearch}
                className="absolute right-2.5 top-2.5 text-muted-foreground hover:text-foreground"
                aria-label="Clear search"
              >
                <XCircle className="h-4 w-4" />
              </button>
            )}
          </div>
        )}

        <nav className="flex flex-col gap-1 h-full overflow-y-auto -mx-4 px-4">
          <ChatHistoryList
            currentChatId={chatId}
            currentProjectId={projectId}
            searchQuery={searchQuery}
          />
        </nav>

        <AccountMenu />

        {/* Project Dialog */}
        <ProjectDialog
          open={isProjectDialogOpen}
          onOpenChange={setIsProjectDialogOpen}
          onSuccess={navigateToProject}
        />
      </div>
    </div>
  );
}

export function SidebarToggle({ onToggle }: { onToggle: () => void }) {
  return (
    <Button variant="outline" size="icon" className="md:hidden" onClick={onToggle}>
      <PanelRightClose className="h-4 w-4" />
    </Button>
  );
}
