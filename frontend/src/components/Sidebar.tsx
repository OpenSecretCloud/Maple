import {
  Search,
  SquarePenIcon,
  PanelRightClose,
  PanelRightOpen,
  XCircle
} from "lucide-react";
import { Button } from "./ui/button";
import { useLocation, useRouter } from "@tanstack/react-router";
import { ChatHistoryList } from "./ChatHistoryList";
import { AccountMenu } from "./AccountMenu";
import { useRef, useEffect, KeyboardEvent, useCallback, useState } from "react";
import { cn, useClickOutside, useIsMobile } from "@/utils/utils";
import { Input } from "./ui/input";
import { useLocalState } from "@/state/useLocalState";

export function Sidebar({
  chatId,
  isOpen,
  onToggle
}: {
  chatId?: string;
  isOpen: boolean;
  onToggle: () => void;
}) {
  const router = useRouter();
  const location = useLocation();
  const {
    searchQuery,
    setSearchQuery,
    isSearchVisible,
    setIsSearchVisible,
    sidebarWidth,
    setSidebarWidth,
    isSidebarCollapsed,
    setIsSidebarCollapsed
  } = useLocalState();
  const searchInputRef = useRef<HTMLInputElement>(null);

  async function addChat() {
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

  // Use the centralized hook for mobile detection
  const isMobile = useIsMobile();

  // Resize functionality constants
  const MIN_WIDTH = 160; // Below this, sidebar will be hidden
  const MAX_WIDTH = 400; // Maximum allowed width

  // Resize state
  const [isResizing, setIsResizing] = useState(false);
  const resizeRef = useRef<HTMLDivElement>(null);

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

  // Resize handlers
  const handleMouseDown = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    setIsResizing(true);
  }, []);

  const handleMouseMove = useCallback(
    (e: MouseEvent) => {
      if (!isResizing) return;

      const newWidth = e.clientX;

      if (newWidth < MIN_WIDTH) {
        // Hide sidebar when dragged below minimum width
        setIsSidebarCollapsed(true);
      } else if (newWidth <= MAX_WIDTH) {
        // Show sidebar and update width if within bounds
        setIsSidebarCollapsed(false);
        setSidebarWidth(Math.min(newWidth, MAX_WIDTH));
      }
    },
    [isResizing, setSidebarWidth, setIsSidebarCollapsed, MIN_WIDTH, MAX_WIDTH]
  );

  const handleMouseUp = useCallback(() => {
    setIsResizing(false);
  }, []);

  // Global mouse events for resize
  useEffect(() => {
    if (isResizing) {
      document.addEventListener("mousemove", handleMouseMove);
      document.addEventListener("mouseup", handleMouseUp);
      document.body.style.cursor = "col-resize";

      return () => {
        document.removeEventListener("mousemove", handleMouseMove);
        document.removeEventListener("mouseup", handleMouseUp);
        document.body.style.cursor = "";
      };
    }
  }, [isResizing, handleMouseMove, handleMouseUp]);


  // Don't render sidebar at all if collapsed on desktop and mobile behavior is handled elsewhere
  if (isSidebarCollapsed && !isMobile) {
    return null;
  }

  return (
    <div
      ref={sidebarRef}
      className={cn([
        "fixed md:static z-10 h-full overflow-y-hidden relative",
        // On mobile: use isOpen state, on desktop: use collapsed state and dynamic width
        isMobile ? (isOpen ? "block" : "hidden") : isSidebarCollapsed ? "hidden" : "block"
      ])}
      style={{
        width: isMobile ? "280px" : `${sidebarWidth}px`
      }}
    >
      <div
        className="h-full border-r border-input dark:bg-background bg-[hsl(var(--footer-bg))] backdrop-blur-lg flex flex-col gap-4 px-4 py-4 md:py-8 items-stretch"
        style={{
          width: isMobile ? "280px" : `${sidebarWidth}px`
        }}
      >
        <div className="flex justify-between items-center">
          <Button variant="outline" size="icon" className="md:w-full gap-2" onClick={addChat}>
            <SquarePenIcon className="w-4 h-4" />
            <span className="hidden md:block">New Chat</span>
          </Button>
          <Button variant="outline" size="icon" className="md:hidden" onClick={onToggle}>
            <PanelRightOpen className="h-4 w-4" />
          </Button>
        </div>
        <div className="flex justify-between items-center">
          <h2 className="font-semibold">History</h2>
          <Button
            variant="ghost"
            size="icon"
            className="h-8 w-8"
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
        <nav className="flex flex-col gap-2 px-4 -mx-4 h-full overflow-y-auto">
          <ChatHistoryList currentChatId={chatId} searchQuery={searchQuery} />
        </nav>
        <AccountMenu />
      </div>

      {/* Resize handle - only on desktop */}
      {!isMobile && (
        <div
          className="absolute top-0 right-0 w-1 h-full cursor-col-resize hover:bg-primary/20 transition-colors group"
          onMouseDown={handleMouseDown}
          ref={resizeRef}
        >
          {/* Visual grip indicator that appears on hover */}
          <div className="absolute top-1/2 -translate-y-1/2 right-0 w-1 h-8 bg-border group-hover:bg-primary/40 rounded-l transition-colors" />
        </div>
      )}
    </div>
  );
}

export function SidebarToggle({ onToggle }: { onToggle: () => void }) {
  const { isSidebarCollapsed, setIsSidebarCollapsed, setSidebarWidth } = useLocalState();
  const isMobile = useIsMobile();

  // If sidebar is collapsed on desktop, provide a way to show it
  if (isSidebarCollapsed && !isMobile) {
    return (
      <Button
        variant="outline"
        size="icon"
        className="fixed top-4 left-4 z-20"
        onClick={() => {
          setIsSidebarCollapsed(false);
          setSidebarWidth(280);
        }}
        title="Show sidebar"
      >
        <PanelRightOpen className="h-4 w-4" />
      </Button>
    );
  }

  // Mobile toggle (existing behavior)
  return (
    <Button variant="outline" size="icon" className="md:hidden" onClick={onToggle}>
      <PanelRightClose className="h-4 w-4" />
    </Button>
  );
}
