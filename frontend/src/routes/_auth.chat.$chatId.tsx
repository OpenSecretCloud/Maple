import { useEffect, useRef, useState, useCallback } from "react";
import { createFileRoute } from "@tanstack/react-router";
import { Check, Copy, UserIcon, ChevronDown, Bot, SquarePenIcon, Archive } from "lucide-react";
import { useLocalState } from "@/state/useLocalState";
import { Markdown } from "@/components/markdown";
import { Sidebar, SidebarToggle } from "@/components/Sidebar";
import { Button } from "@/components/ui/button";
import { useNavigate } from "@tanstack/react-router";
import { useIsMobile } from "@/utils/utils";
import { useQuery } from "@tanstack/react-query";

// Simple message type for archived chats
interface ArchivedMessage {
  role: "user" | "assistant" | "system";
  content: string | { type: string; text?: string; image_url?: { url: string } }[];
}

export const Route = createFileRoute("/_auth/chat/$chatId")({
  component: ChatComponent
});

// Custom hook for copy to clipboard functionality
function useCopyToClipboard(text: string) {
  const [isCopied, setIsCopied] = useState(false);

  const handleCopy = useCallback(async () => {
    try {
      await navigator.clipboard.writeText(text);
      setIsCopied(true);
      setTimeout(() => setIsCopied(false), 2000);
    } catch (error) {
      console.error("Failed to copy text: ", error);
    }
  }, [text]);

  return { isCopied, handleCopy };
}

function renderContent(content: ArchivedMessage["content"], chatId: string) {
  if (typeof content === "string") {
    return <Markdown content={content} loading={false} chatId={chatId} />;
  }
  return content.map((p, idx) =>
    p.type === "text" ? (
      <Markdown key={idx} content={p.text || ""} loading={false} chatId={chatId} />
    ) : (
      <img key={idx} src={p.image_url?.url} className="max-w-full rounded-lg" alt="" />
    )
  );
}

function UserMessage({ message, chatId }: { message: ArchivedMessage; chatId: string }) {
  return (
    <div className="flex flex-col p-4 rounded-lg bg-muted">
      <div className="rounded-lg flex flex-col md:flex-row gap-4">
        <div>
          <UserIcon />
        </div>
        <div className="flex flex-col gap-2 min-w-0 flex-1 overflow-hidden">
          {renderContent(message.content, chatId)}
        </div>
      </div>
    </div>
  );
}

function AssistantMessage({ text, chatId }: { text: string; chatId: string }) {
  const { isCopied, handleCopy } = useCopyToClipboard(text);

  return (
    <div className="group flex flex-col p-4">
      <div className="rounded-lg flex flex-col md:flex-row gap-4">
        <div>
          <Bot />
        </div>
        <div className="flex flex-col gap-2 min-w-0 flex-1 overflow-hidden">
          <Markdown content={text} loading={false} chatId={chatId} />
          <div className="flex gap-2 items-center">
            <Button
              variant="ghost"
              size="sm"
              className="h-8 w-8 p-0"
              onClick={handleCopy}
              aria-label={isCopied ? "Copied" : "Copy to clipboard"}
            >
              {isCopied ? <Check className="h-4 w-4" /> : <Copy className="h-4 w-4" />}
            </Button>
          </div>
        </div>
      </div>
    </div>
  );
}

function SystemPromptMessage({ text }: { text: string }) {
  const [isExpanded, setIsExpanded] = useState(false);
  const { isCopied, handleCopy } = useCopyToClipboard(text);

  return (
    <div className="group flex flex-col p-4 rounded-lg bg-muted/50">
      <div className="rounded-lg flex flex-col md:flex-row gap-4">
        <div>
          <Bot className="h-5 w-5 text-muted-foreground" />
        </div>
        <div className="flex flex-col gap-2 w-full">
          <div className="flex items-center gap-2">
            <span className="text-sm font-medium text-muted-foreground">System Prompt</span>
          </div>
          <div className="text-sm text-foreground">
            {isExpanded ? (
              <>
                {text}
                {text.length > 100 && (
                  <>
                    {" "}
                    <button
                      className="text-primary hover:text-primary/80 underline cursor-pointer"
                      onClick={() => setIsExpanded(false)}
                    >
                      show less
                    </button>
                  </>
                )}
              </>
            ) : (
              <>
                {text.length > 100 ? text.slice(0, 100) : text}
                {text.length > 100 && (
                  <>
                    {"... "}
                    <button
                      className="text-primary hover:text-primary/80 underline cursor-pointer"
                      onClick={() => setIsExpanded(true)}
                    >
                      see more
                    </button>
                  </>
                )}
              </>
            )}
          </div>
          <Button
            variant="ghost"
            size="sm"
            className="h-8 w-8 p-0 self-start"
            onClick={handleCopy}
            aria-label={isCopied ? "Copied" : "Copy to clipboard"}
          >
            {isCopied ? <Check className="h-4 w-4" /> : <Copy className="h-4 w-4" />}
          </Button>
        </div>
      </div>
    </div>
  );
}

function ChatComponent() {
  const { chatId } = Route.useParams();
  const { getChatById } = useLocalState();
  const navigate = useNavigate();
  const isMobile = useIsMobile();
  const [showScrollButton, setShowScrollButton] = useState(false);
  const [isSidebarOpen, setIsSidebarOpen] = useState(false);
  const chatContainerRef = useRef<HTMLDivElement>(null);

  // Fetch chat from KV store
  const { data: chat, isPending } = useQuery({
    queryKey: ["archivedChat", chatId],
    queryFn: () => getChatById(chatId),
    retry: false
  });

  const toggleSidebar = useCallback(() => setIsSidebarOpen((prev) => !prev), []);

  // Handle mobile new chat
  const handleMobileNewChat = useCallback(async () => {
    try {
      await navigate({ to: "/" });
      setTimeout(() => document.getElementById("message")?.focus(), 0);
    } catch (error) {
      console.error("Navigation failed:", error);
    }
  }, [navigate]);

  // Scroll detection
  const handleScroll = useCallback(() => {
    const container = chatContainerRef.current;
    if (!container) return;
    const { scrollTop, scrollHeight, clientHeight } = container;
    const isNearBottom = scrollHeight - scrollTop - clientHeight < 100;
    setShowScrollButton(!isNearBottom);
  }, []);

  useEffect(() => {
    const container = chatContainerRef.current;
    if (!container) return;
    container.addEventListener("scroll", handleScroll);
    handleScroll();
    return () => container.removeEventListener("scroll", handleScroll);
  }, [handleScroll]);

  const scrollToBottom = useCallback(() => {
    if (chatContainerRef.current) {
      chatContainerRef.current.scrollTo({
        top: chatContainerRef.current.scrollHeight,
        behavior: "smooth"
      });
    }
  }, []);

  if (isPending) {
    return (
      <div className="grid h-dvh w-full grid-cols-1 md:grid-cols-[280px_1fr]">
        <Sidebar chatId={chatId} isOpen={isSidebarOpen} onToggle={toggleSidebar} />
        <main className="flex h-dvh flex-col items-center justify-center">
          <p className="text-muted-foreground">Loading archived chat...</p>
        </main>
      </div>
    );
  }

  if (!chat) {
    return (
      <div className="grid h-dvh w-full grid-cols-1 md:grid-cols-[280px_1fr]">
        <Sidebar chatId={chatId} isOpen={isSidebarOpen} onToggle={toggleSidebar} />
        <main className="flex h-dvh flex-col items-center justify-center">
          <p className="text-muted-foreground">Archived chat not found</p>
        </main>
      </div>
    );
  }

  // Convert Chat messages to ArchivedMessage format - safely handle the types
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const archivedMessages: ArchivedMessage[] = (chat.messages || []).map((msg: any) => ({
    role: (msg.role || "user") as "user" | "assistant" | "system",
    content: msg.content || ""
  }));

  return (
    <div className="grid h-dvh w-full grid-cols-1 md:grid-cols-[280px_1fr]">
      <Sidebar chatId={chatId} isOpen={isSidebarOpen} onToggle={toggleSidebar} />
      <main className="flex h-dvh flex-col bg-card/90 backdrop-blur-lg bg-center overflow-hidden">
        {!isSidebarOpen && (
          <div className="fixed top-4 left-4 z-20 md:hidden">
            <SidebarToggle onToggle={toggleSidebar} />
          </div>
        )}
        <div
          ref={chatContainerRef}
          className="flex-1 min-h-0 overflow-y-auto overflow-x-hidden flex flex-col relative"
        >
          <div className="mt-4 md:mt-8 w-full h-10 flex items-center justify-center relative">
            {isMobile && (
              <Button
                variant="outline"
                size="icon"
                className="absolute right-4"
                onClick={handleMobileNewChat}
                aria-label="New chat"
              >
                <SquarePenIcon className="h-4 w-4" />
              </Button>
            )}
            <h2 className="text-base font-semibold self-center truncate max-w-[20rem] mx-[6rem] py-2">
              {chat.title}
            </h2>
          </div>
          <div className="flex flex-col w-full max-w-[45rem] mx-auto gap-4 px-2 pt-4">
            {archivedMessages.map((message, index) => (
              <div
                key={index}
                id={`message-${message.role}-${index}`}
                className="flex flex-col gap-2"
              >
                {message.role === "system" && (
                  <SystemPromptMessage
                    text={
                      typeof message.content === "string"
                        ? message.content
                        : message.content.find((p) => p.type === "text")?.text || ""
                    }
                  />
                )}
                {message.role === "user" && <UserMessage message={message} chatId={chatId} />}
                {message.role === "assistant" && (
                  <AssistantMessage
                    text={
                      typeof message.content === "string"
                        ? message.content
                        : message.content.find((p) => p.type === "text")?.text || ""
                    }
                    chatId={chatId}
                  />
                )}
              </div>
            ))}
          </div>
          {showScrollButton && (
            <button
              onClick={scrollToBottom}
              className="fixed bottom-24 right-4 p-2 bg-primary text-primary-foreground rounded-full shadow-lg hover:bg-primary/90 transition-colors z-10"
              aria-label="Scroll to bottom"
            >
              <ChevronDown className="h-5 w-5" />
            </button>
          )}
        </div>

        {/* Archived Chat Banner - replacing ChatBox */}
        <div className="w-full max-w-[45rem] mx-auto flex flex-col px-2 pb-4">
          <div className="bg-muted/50 border border-border rounded-lg p-4 flex items-center gap-3">
            <Archive className="h-5 w-5 text-muted-foreground flex-shrink-0" />
            <div className="flex-1">
              <p className="text-sm font-medium">Archived Chat</p>
              <p className="text-xs text-muted-foreground">
                This is a read-only chat from your history. Start a new chat to continue the
                conversation.
              </p>
            </div>
            <Button onClick={handleMobileNewChat} size="sm">
              New Chat
            </Button>
          </div>
        </div>
      </main>
    </div>
  );
}
