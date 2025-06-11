import { Search, SquarePenIcon, PanelRightClose, PanelRightOpen, XCircle } from "lucide-react";
import { Button } from "./ui/button";
import { useLocation, useRouter } from "@tanstack/react-router";
import { ChatHistoryList } from "./ChatHistoryList";
import { AccountMenu } from "./AccountMenu";
import { useRef, useEffect, KeyboardEvent, useCallback, useLayoutEffect } from "react";
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
  const { searchQuery, setSearchQuery, isSearchVisible, setIsSearchVisible } = useLocalState();
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
  const handleClickOutside = useCallback(
    (event: MouseEvent | TouchEvent) => {
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
    },
    [isOpen, onToggle]
  );

  useClickOutside(sidebarRef, handleClickOutside);

  // Use the centralized hook for mobile detection
  const isMobile = useIsMobile();

  // Track if component is mounted to prevent state updates after unmount
  const isMountedRef = useRef(true);
  useLayoutEffect(() => {
    isMountedRef.current = true;
    return () => {
      isMountedRef.current = false;
    };
  }, []);

  // This effect closes the sidebar on mobile when navigating,
  // but preserves search state between navigations
  useEffect(() => {
    // Only subscribe if we're on mobile and sidebar is open
    if (!isMobile || !isOpen) return;

    const unsubscribe = router.subscribe("onResolved", () => {
      // Use a microtask to avoid state updates during render
      queueMicrotask(() => {
        // Prevent updates if component unmounted
        if (!isMountedRef.current) return;

        // Double-check conditions after async boundary
        if (isOpen && isMobile) {
          onToggle();
        }
      });
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
      <div className="h-full border-r border-input dark:bg-background bg-[hsl(var(--footer-bg))] backdrop-blur-lg flex flex-col gap-4 px-4 py-4 md:py-8 items-stretch w-[280px]">
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
