import { SquarePenIcon, PanelRightClose, PanelRightOpen } from "lucide-react";
import { Button } from "./ui/button";
import { useLocation, useRouter } from "@tanstack/react-router";
import { ChatHistoryList } from "./ChatHistoryList";
import { AccountMenu } from "./AccountMenu";
import { useRef, useEffect } from "react";
import { cn, useClickOutside } from "@/utils/utils";

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

  async function addChat() {
    // If sidebar is open, close it
    if (isOpen) {
      onToggle();
    }
    // If we're already on "/", focus the chat box
    if (location.pathname === "/") {
      document.getElementById("message")?.focus();
    } else {
      router.navigate({ to: `/` }).then(() => {
        document.getElementById("message")?.focus();
      });
    }
  }

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

  // Close the sidebar if we navigate to a different route
  useEffect(() => {
    const unsubscribe = router.subscribe("onResolved", () => {
      if (isOpen) {
        onToggle();
      }
    });

    return () => {
      unsubscribe();
    };
  }, [router, isOpen, onToggle]);

  return (
    <div
      ref={sidebarRef}
      className={cn([
        "fixed md:static z-10 h-full overflow-y-hidden",
        isOpen ? "block w-[280px]" : "hidden md:block md:w-[280px]"
      ])}
    >
      <div className="h-full border-r border-input bg-background backdrop-blur-lg flex flex-col gap-4 px-4 py-4 md:py-8 items-stretch w-[280px]">
        <div className="flex justify-between items-center">
          <Button variant={"outline"} size="icon" className="md:w-full gap-2" onClick={addChat}>
            <SquarePenIcon className="w-4 h-4" />
            <span className="hidden md:block">New Chat</span>
          </Button>
          <Button variant="outline" size="icon" className="md:hidden" onClick={onToggle}>
            <PanelRightOpen className="h-4 w-4" />
          </Button>
        </div>
        <h2 className="font-semibold -mb-2">History</h2>
        <nav className="flex flex-col gap-2 px-4 -mx-4 h-full overflow-y-auto">
          <ChatHistoryList currentChatId={chatId} />
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
