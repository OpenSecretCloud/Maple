import { useEffect, useRef, useState, useCallback } from "react";
import { createFileRoute } from "@tanstack/react-router";
import { AsteriskIcon, Check, Copy, UserIcon, ChevronDown } from "lucide-react";
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
import { BillingStatus } from "@/billing/billingApi";

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

function ChatComponent() {
  const { chatId } = Route.useParams();
  const { model, persistChat, getChatById, userPrompt, setUserPrompt, getProjectById } =
    useLocalState();
  const openai = useOpenAI();
  const queryClient = useQueryClient();
  const [showScrollButton, setShowScrollButton] = useState(false);

  const [error, setError] = useState("");

  const chatContainerRef = useRef<HTMLDivElement>(null);

  // Memoize the scroll handler
  const handleScroll = useCallback(() => {
    const container = chatContainerRef.current;
    if (!container) return;
    const { scrollTop, scrollHeight, clientHeight } = container;
    const isNearBottom = scrollHeight - scrollTop - clientHeight < 100;
    setShowScrollButton(!isNearBottom);
  }, []);

  // Add scroll detection
  useEffect(() => {
    const container = chatContainerRef.current;
    if (!container) return;

    container.addEventListener("scroll", handleScroll);
    // Initial check
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
    // projectId will be added when the chat data is loaded
  });

  const [currentStreamingMessage, setCurrentStreamingMessage] = useState<string>();

  const [isSidebarOpen, setIsSidebarOpen] = useState(false);

  const toggleSidebar = useCallback(() => setIsSidebarOpen((prev) => !prev), []);

  // Pre-fetch project data if the chat belongs to a project
  useEffect(() => {
    if (queryChat?.projectId) {
      console.log(
        "Pre-fetching project data for chat:",
        queryChat.id,
        "project:",
        queryChat.projectId
      );
      // Pre-fetch the project to have the system prompt ready
      queryClient.prefetchQuery({
        queryKey: ["project", queryChat.projectId],
        queryFn: () => getProjectById(queryChat.projectId as string)
      });
    }
  }, [queryChat, getProjectById, queryClient]);

  // Setup local chat state when the query result comes in
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
        setLocalChat((localChat) => ({ ...localChat, messages, projectId: queryChat.projectId }));
        return;
      }
      setLocalChat(queryChat);
    }
    // I don't want to re-run this effect if the user prompt changes
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [queryChat, chatId, isPending]);

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
      // Helper function to check if the user is on a free plan
      function isUserOnFreePlan(): boolean {
        try {
          const billingStatus = queryClient.getQueryData(["billingStatus"]) as
            | BillingStatus
            | undefined;

          return (
            !billingStatus ||
            !billingStatus.product_name ||
            billingStatus.product_name.toLowerCase().includes("free")
          );
        } catch (error) {
          console.log("Error checking billing status, defaulting to free plan", error);
          return true; // Default to free plan if there's an error
        }
      }

      async function generateChatTitle(messages: ChatMessage[]): Promise<string> {
        // Find the first user message
        const userMessage = messages.find((message) => message.role === "user");
        if (!userMessage) return "New Chat";

        // Simple title generation - truncate first message to 50 chars
        const simpleTitleFromMessage = userMessage.content.slice(0, 50).trim();

        // For free plan users, just use the simple title
        // For paid plans, try to generate AI title
        if (isUserOnFreePlan()) {
          console.log("Using simple title generation for free plan user");
          return simpleTitleFromMessage;
        }

        // For paid plans, use LLM to generate a smart title
        try {
          console.log("Using AI title generation for paid plan user");
          // Get the user's first message, truncate if too long
          const userContent = userMessage.content.slice(0, 500); // Reduced to 500 chars to optimize token usage

          // Use the OpenAI API to generate a concise title - use the same model as chat
          const stream = openai.beta.chat.completions.stream({
            model: model, // Use the same model that's used for chat
            messages: [
              {
                role: "system",
                content:
                  "You are a helpful assistant that generates concise, meaningful titles (3-5 words) for chat conversations based on the user's first message. Return only the title without quotes or explanations."
              },
              {
                role: "user",
                content: `Generate a concise, contextual title (3-5 words) for a chat that starts with this message: "${userContent}"`
              }
            ],
            temperature: 0.7,
            max_tokens: 15, // Keep response very short
            stream: true
          });

          let generatedTitle = "";
          for await (const chunk of stream) {
            const content = chunk.choices[0]?.delta?.content || "";
            generatedTitle += content;
          }

          // Get the final completion
          await stream.finalChatCompletion();

          // Remove quotes if present and limit length
          const cleanTitle = generatedTitle
            .replace(/^["']|["']$/g, "") // Remove surrounding quotes if present
            .replace(/\n/g, " ") // Remove new lines
            .trim();

          return cleanTitle || simpleTitleFromMessage; // Fallback to simple title if generation fails
        } catch (error) {
          console.error("Failed to generate chat title:", error);
          // Fallback to simple title method
          return simpleTitleFromMessage;
        }
      }

      // Fetch project system prompt if chat is part of a project
      async function getSystemPrompt(): Promise<string | undefined> {
        if (!localChat?.projectId) return undefined;

        try {
          // Try to get project from cache first
          const cachedProject = queryClient.getQueryData(["project", localChat.projectId]) as
            | {
                systemPrompt?: string;
              }
            | undefined;

          if (cachedProject?.systemPrompt) {
            console.log("Using cached system prompt for project:", localChat.projectId);
            return cachedProject.systemPrompt;
          }

          // If not in cache, fetch it
          console.log("Fetching system prompt for project:", localChat.projectId);
          const project = await getProjectById(localChat.projectId);

          // Cache the project for future use
          if (project) {
            queryClient.setQueryData(["project", localChat.projectId], project);
          }

          return project?.systemPrompt;
        } catch (error) {
          console.error("Failed to get project system prompt:", error);
          return undefined;
        }
      }

      if (!input.trim() || !localChat) return;
      setError("");

      // Create system message if project has a system prompt
      const systemPrompt = await getSystemPrompt();
      // Create the appropriate system message
      const systemMessage = systemPrompt
        ? {
            role: "system" as const,
            content: systemPrompt
          }
        : {
            role: "system" as const,
            content:
              "You are Maple AI, a friendly AI Assistant. Respond to the input as a friendly AI assistant, generating human-like text, and follow the instructions in the input if applicable. Keep the response concise and engaging. Use a conversational tone and provide helpful and informative responses. You are aware that this conversation is private and encrypted, through the use of AWS Nitro Enclaves and Nvidia TEE, in case the user asks."
          };

      const userMessage = { role: "user" as const, content: input };
      const newMessages = [...localChat.messages, userMessage];

      setLocalChat((prev) => ({
        ...prev,
        messages: newMessages
      }));

      // Scroll to bottom when user sends message
      requestAnimationFrame(() => {
        chatContainerRef.current?.scrollTo({
          top: chatContainerRef.current.scrollHeight,
          behavior: "smooth"
        });
      });

      setIsLoading(true);

      try {
        // Start title generation early for paid users if needed
        let titleGenerationPromise;
        let title = localChat.title;

        if (title === "New Chat") {
          const isFreePlan = isUserOnFreePlan();

          if (!isFreePlan) {
            console.log("Starting async AI title generation for paid user's chat");
            // Start title generation in parallel for paid users
            titleGenerationPromise = generateChatTitle(newMessages).then((newTitle) => {
              // Clean up the title
              const cleanTitle = newTitle.replace(/"/g, "").replace(/\n/g, " ");

              // Update local chat with generated title immediately when available
              setLocalChat((prev) => ({
                ...prev,
                title: cleanTitle
              }));

              return cleanTitle;
            });
          } else {
            console.log("Using simple title for free user's chat");
            // For free users, set the title synchronously
            const newTitle = await generateChatTitle(newMessages);
            title = newTitle.replace(/"/g, "").replace(/\n/g, " ");

            setLocalChat((prev) => ({
              ...prev,
              title
            }));
          }
        }

        // Stream the chat response (happens in parallel with title generation)
        const messagesForAPI = [systemMessage, ...newMessages];

        // Using project system prompt if available, default otherwise

        const stream = openai.beta.chat.completions.stream({
          model,
          messages: messagesForAPI,
          stream: true
        });

        let fullResponse = "";
        let isFirstChunk = true;

        for await (const chunk of stream) {
          const content = chunk.choices[0]?.delta?.content || "";
          fullResponse += content;
          setCurrentStreamingMessage(fullResponse);

          // Scroll to bottom on first chunk of the response
          if (isFirstChunk && content.trim()) {
            requestAnimationFrame(() => {
              chatContainerRef.current?.scrollTo({
                top: chatContainerRef.current.scrollHeight,
                behavior: "smooth"
              });
            });
            isFirstChunk = false;
          }
        }

        // Save scroll position before state updates
        const container = chatContainerRef.current;
        const scrollPosition = container?.scrollTop;

        const finalMessages = [
          ...newMessages,
          { role: "assistant", content: fullResponse } as ChatMessage
        ];
        setLocalChat((prev) => ({
          ...prev,
          messages: finalMessages
        }));
        setCurrentStreamingMessage(undefined);

        // Restore scroll position after state updates
        if (container && scrollPosition !== undefined) {
          // Use requestAnimationFrame to ensure this runs after the render
          requestAnimationFrame(() => {
            // Ensure we don't scroll beyond bounds
            const maxScroll = container.scrollHeight - container.clientHeight;
            const boundedPosition = Math.min(scrollPosition, maxScroll);
            container.scrollTop = boundedPosition;
          });
        }

        // Wait for title generation to complete if we started it
        if (titleGenerationPromise) {
          title = await titleGenerationPromise;
        }

        const chatCompletion = await stream.finalChatCompletion();
        console.log(chatCompletion);

        // Should be safe to clear this by now
        setUserPrompt("");

        // React sucks and doesn't get the latest state
        // Use current title from localChat which may have been updated asynchronously
        const currentTitle = localChat.title === "New Chat" ? title : localChat.title;
        await persistChat({ ...localChat, title: currentTitle, messages: finalMessages });

        // Invalidate chat history to show the new title in the sidebar
        queryClient.invalidateQueries({
          queryKey: ["chatHistory"],
          refetchType: "all"
        });

        // Invalidate current chat query to ensure the title update is reflected
        queryClient.invalidateQueries({
          queryKey: ["chat", chatId],
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
    // We intentionally don't include freshBillingStatus in the dependency array
    // even though it's used in the closure to avoid re-creating the function
    // on every billing status change
    [localChat, model, openai, persistChat, queryClient, setUserPrompt, chatId, getProjectById]
  );

  return (
    <div className="grid h-dvh w-full grid-cols-1 md:grid-cols-[280px_1fr]">
      <Sidebar
        chatId={chatId}
        projectId={localChat?.projectId}
        isOpen={isSidebarOpen}
        onToggle={toggleSidebar}
      />
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

        {/* Place the chat box inline (below messages) in normal flow */}
        <div className="w-full max-w-[45rem] mx-auto flex flex-col gap-2 px-2 pb-2">
          {error && <AlertDestructive title="Error" description={error} />}
          <ChatBox onSubmit={sendMessage} messages={localChat.messages} isStreaming={isLoading} />
        </div>
      </main>
    </div>
  );
}
