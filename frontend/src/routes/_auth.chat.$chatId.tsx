import { useEffect, useRef, useState, useCallback } from "react";
import { createFileRoute } from "@tanstack/react-router";
import { AsteriskIcon, Check, Copy, UserIcon } from "lucide-react";
import ChatBox from "@/components/ChatBox";
import { useOpenAI } from "@/ai/useOpenAi";
import { useLocalState } from "@/state/useLocalState";
import { Markdown } from "@/components/markdown";
import { ChatMessage, Chat } from "@/state/LocalStateContext";
import { AlertDestructive } from "@/components/AlertDestructive";
import { Sidebar, SidebarToggle } from "@/components/Sidebar";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { InfoPopover } from "@/components/InfoPopover";
import { Button } from "@/components/ui/button";

export const Route = createFileRoute("/_auth/chat/$chatId")({
  component: ChatComponent
});

function UserMessage({ text }: { text: string }) {
  return (
    <div className="flex flex-col p-4 rounded-lg bg-muted">
      <div className="rounded-lg flex flex-col md:flex-row gap-4">
        <div>
          <UserIcon />
        </div>
        <div className="flex flex-col gap-2">
          <Markdown content={text} loading={false} />
        </div>
      </div>
    </div>
  );
}

function SystemMessage({ text, loading }: { text: string; loading?: boolean }) {
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

  return (
    <div className="group flex flex-col p-4">
      <div className="rounded-lg flex flex-col md:flex-row gap-4">
        <div>
          <AsteriskIcon />
        </div>
        <div className="flex flex-col gap-2">
          <Markdown content={text} loading={loading} />
          <Button
            variant="ghost"
            size="sm"
            className="self-start -mx-2 -mb-2 group-hover:opacity-100 opacity-0 transition-opacity"
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

const scrollToMessage = (messageId: string) => {
  const element = document.getElementById(messageId);
  if (element) {
    element.scrollIntoView({ behavior: "smooth", block: "start" });
  }
};

function ChatComponent() {
  const { chatId } = Route.useParams();
  const { model, persistChat, getChatById, userPrompt, setUserPrompt } = useLocalState();
  const openai = useOpenAI();
  const queryClient = useQueryClient();

  const [error, setError] = useState("");

  const chatContainerRef = useRef<HTMLDivElement>(null);

  // Query the chat from the backend, in case it already exists
  const {
    isPending,
    error: queryError,
    data: queryChat
  } = useQuery({
    queryKey: ["chat", chatId],
    queryFn: () => {
      return getChatById(chatId);
    },
    retry: false
  });

  useEffect(() => {
    if (queryError) {
      console.error("Error fetching chat:", queryError);
      setError("Error fetching chat. Please try again.");
    }
  }, [queryError]);

  // We need to keep a local state so we can stream in chat responses
  const [localChat, setLocalChat] = useState<Chat>({
    id: chatId,
    title: "New Chat",
    messages: []
  });

  const [currentStreamingMessage, setCurrentStreamingMessage] = useState<string>();

  const [isSidebarOpen, setIsSidebarOpen] = useState(false);

  const toggleSidebar = useCallback(() => setIsSidebarOpen((prev) => !prev), []);

  useEffect(() => {
    if (queryChat && !isPending) {
      console.debug("Chat loaded from query:", queryChat);
      if (queryChat.id !== chatId) {
        console.error("Chat ID mismatch");
        setLocalChat((localChat) => ({ ...localChat, messages: [] }));
        return;
      }
      if (queryChat.messages.length === 0) {
        console.warn("Chat has no messages, using user prompt");
        const messages = userPrompt
          ? ([{ role: "user", content: userPrompt }] as ChatMessage[])
          : [];
        setLocalChat((localChat) => ({ ...localChat, messages }));
        return;
      }
      setLocalChat(queryChat);
    }
    // I don't want to re-run this effect if the user prompt changes
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [queryChat, chatId, isPending]);

  async function generateChatTitle(messages: ChatMessage[]): Promise<string> {
    // Find the first user message
    const userMessage = messages.find((message) => message.role === "user");
    if (!userMessage) return "New Chat";
    // Use the first 50 characters of the user message
    return `${userMessage.content.slice(0, 50)}`;
  }

  // IMPORTANT that this runs only once (because it uses the user's tokens!)
  const userPromptEffectRan = useRef(false);

  useEffect(() => {
    if (userPromptEffectRan.current) return;
    userPromptEffectRan.current = true;
    if (userPrompt) {
      console.log("User prompt found, sending to chat");
      console.log("USER PROMPT:", userPrompt);
      sendMessage(userPrompt);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const [isLoading, setIsLoading] = useState(false);

  const sendMessage = useCallback(
    async (input: string) => {
      if (!input.trim() || !localChat) return;
      setError("");

      const newMessages = [...localChat.messages, { role: "user", content: input } as ChatMessage];

      setLocalChat((prev) => ({
        ...prev,
        messages: newMessages
      }));

      // Scroll to the new user message
      setTimeout(() => {
        scrollToMessage(`message-user-${newMessages.length - 1}`);
      }, 0);

      setIsLoading(true);

      try {
        const stream = openai.beta.chat.completions.stream({
          model,
          messages: newMessages,
          stream: true
        });

        let fullResponse = "";

        for await (const chunk of stream) {
          const content = chunk.choices[0]?.delta?.content || "";
          fullResponse += content;
          setCurrentStreamingMessage(fullResponse);
        }

        const finalMessages = [
          ...newMessages,
          { role: "assistant", content: fullResponse } as ChatMessage
        ];
        setLocalChat((prev) => ({
          ...prev,
          messages: finalMessages
        }));
        setCurrentStreamingMessage(undefined);

        let title = localChat.title;

        // Generate and update the chat title, if the current title isn't "New Chat"
        if (title === "New Chat") {
          console.log("Generating chat title");
          const newTitle = await generateChatTitle(finalMessages);

          // Get rid of quotes and any newlines in the title
          title = newTitle.replace(/"/g, "").replace(/\n/g, " ");
        }

        const chatCompletion = await stream.finalChatCompletion();
        console.log(chatCompletion);

        // Should be safe to clear this by now
        setUserPrompt("");

        // React sucks and doesn't get the latest state
        await persistChat({ ...localChat, title, messages: finalMessages });

        queryClient.invalidateQueries({
          queryKey: ["chatHistory"],
          refetchType: "all"
        });

        // Only invalidate billing status after everything is complete
        queryClient.invalidateQueries({
          queryKey: ["billingStatus"],
          refetchType: "all"
        });
      } catch (error) {
        // If there's an error, we should still refetch the billing status
        // to make sure our optimistic update was correct
        queryClient.invalidateQueries({
          queryKey: ["billingStatus"],
          refetchType: "all"
        });
        console.error("Error:", error);
        if (error instanceof Error) setError(error.message);
      }

      setIsLoading(false);
    },
    [localChat, model, openai, persistChat, queryClient, setUserPrompt]
  );

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
          className="flex-1 min-h-0 overflow-y-auto flex flex-col relative"
        >
          <InfoPopover />
          <div className="mt-4 md:mt-8 w-full h-10 flex items-center justify-center">
            <h2 className="text-lg font-semibold self-center truncate max-w-[20rem] mx-[6rem] py-2">
              {localChat.title}
            </h2>
          </div>
          <div className="flex flex-col w-full max-w-[45rem] mx-auto gap-4 px-2 pt-4">
            {localChat.messages?.map((message, index) => (
              <div
                key={index}
                id={`message-${message.role}-${index}`}
                className="flex flex-col gap-2"
              >
                {message.role === "user" && <UserMessage text={message.content} />}
                {message.role === "assistant" && <SystemMessage text={message.content} />}
              </div>
            ))}
            {(currentStreamingMessage || isLoading) && (
              <div className="flex flex-col gap-2">
                <SystemMessage text={currentStreamingMessage || ""} loading={isLoading} />
              </div>
            )}
          </div>
        </div>

        {/* Place the chat box inline (below messages) in normal flow */}
        <div className="w-full max-w-[45rem] mx-auto flex flex-col gap-2 px-2 pb-2">
          {error && <AlertDestructive title="Error" description={error} />}
          <ChatBox onSubmit={sendMessage} messages={localChat.messages} />
        </div>
      </main>
    </div>
  );
}
