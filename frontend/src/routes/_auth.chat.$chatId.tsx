import { useEffect, useRef, useState, useCallback } from "react";
import { createFileRoute } from "@tanstack/react-router";
import {
  AsteriskIcon,
  Check,
  Copy,
  UserIcon,
  ChevronDown,
  Bot,
  SquarePenIcon,
  Volume2,
  Square
} from "lucide-react";
import ChatBox from "@/components/ChatBox";
import { useOpenAI } from "@/ai/useOpenAi";
import { useLocalState } from "@/state/useLocalState";
import { Markdown, stripThinkingTags } from "@/components/markdown";
import { ChatMessage, DEFAULT_MODEL_ID } from "@/state/LocalStateContext";
import { Sidebar, SidebarToggle } from "@/components/Sidebar";
import { useQueryClient } from "@tanstack/react-query";
import { Button } from "@/components/ui/button";
import { useNavigate, useLocation } from "@tanstack/react-router";
import { useIsMobile } from "@/utils/utils";
import { useChatSession } from "@/hooks/useChatSession";

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

function renderContent(content: ChatMessage["content"], chatId: string) {
  if (typeof content === "string") {
    return <Markdown content={content} loading={false} chatId={chatId} />;
  }
  return content.map((p, idx) =>
    p.type === "text" ? (
      <Markdown key={idx} content={p.text} loading={false} chatId={chatId} />
    ) : (
      <img key={idx} src={p.image_url.url} className="max-w-full rounded-lg" />
    )
  );
}

function UserMessage({ message, chatId }: { message: ChatMessage; chatId: string }) {
  return (
    <div className="flex flex-col p-4 rounded-lg bg-muted">
      <div className="rounded-lg flex flex-col md:flex-row gap-4">
        <div>
          <UserIcon />
        </div>
        <div className="flex flex-col gap-2">{renderContent(message.content, chatId)}</div>
      </div>
    </div>
  );
}

function SystemMessage({
  text,
  loading,
  chatId
}: {
  text: string;
  loading?: boolean;
  chatId: string;
}) {
  const textWithoutThinking = stripThinkingTags(text);
  const { isCopied, handleCopy } = useCopyToClipboard(textWithoutThinking);
  const [isPlaying, setIsPlaying] = useState(false);
  const audioRef = useRef<HTMLAudioElement | null>(null);
  const openai = useOpenAI();

  const handleTTS = useCallback(async () => {
    if (isPlaying) {
      // Stop playing
      if (audioRef.current) {
        audioRef.current.pause();
        audioRef.current = null;
      }
      setIsPlaying(false);
      return;
    }

    try {
      setIsPlaying(true);

      // Generate speech using OpenAI TTS
      const response = await openai.audio.speech.create({
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        model: "kokoro" as any,
        // eslint-disable-next-line @typescript-eslint/no-explicit-any
        voice: "af_sky+af_bella" as any,
        input: textWithoutThinking,
        response_format: "mp3"
      });

      // Convert response to blob and create audio URL
      const blob = new Blob([await response.arrayBuffer()], { type: "audio/mp3" });
      const audioUrl = URL.createObjectURL(blob);

      // Create and play audio
      const audio = new Audio(audioUrl);
      audioRef.current = audio;

      audio.onended = () => {
        setIsPlaying(false);
        URL.revokeObjectURL(audioUrl);
        audioRef.current = null;
      };

      audio.onerror = () => {
        console.error("Error playing audio");
        setIsPlaying(false);
        URL.revokeObjectURL(audioUrl);
        audioRef.current = null;
      };

      await audio.play();
    } catch (error) {
      console.error("TTS error:", error);
      setIsPlaying(false);
    }
  }, [textWithoutThinking, isPlaying, openai]);

  // Cleanup on unmount
  useEffect(() => {
    return () => {
      if (audioRef.current) {
        audioRef.current.pause();
        audioRef.current = null;
      }
    };
  }, []);

  return (
    <div className="group flex flex-col p-4">
      <div className="rounded-lg flex flex-col md:flex-row gap-4">
        <div>
          <AsteriskIcon />
        </div>
        <div className="flex flex-col gap-2">
          <Markdown content={text} loading={loading} chatId={chatId} />
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
            <Button
              variant="ghost"
              size="sm"
              className="h-8 w-8 p-0"
              onClick={handleTTS}
              aria-label={isPlaying ? "Stop audio" : "Play audio"}
            >
              {isPlaying ? <Square className="h-4 w-4" /> : <Volume2 className="h-4 w-4" />}
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
  const {
    model,
    setModel,
    persistChat,
    getChatById,
    userPrompt,
    setUserPrompt,
    systemPrompt,
    setSystemPrompt,
    userImages,
    setUserImages,
    addChat
  } = useLocalState();
  const openai = useOpenAI();
  const queryClient = useQueryClient();
  const [showScrollButton, setShowScrollButton] = useState(false);
  const navigate = useNavigate();
  const location = useLocation();
  const isMobile = useIsMobile();

  const [isSummarizing, setIsSummarizing] = useState(false);
  const [imageConversionError, setImageConversionError] = useState<string | null>(null);

  const chatContainerRef = useRef<HTMLDivElement>(null);

  // Use the chat session hook
  const {
    chat: localChat,
    phase,
    currentStreamingMessage,
    appendUserMessage,
    streamingError,
    isPending
  } = useChatSession(chatId, {
    getChatById,
    persistChat,
    openai,
    model,
    onImageConversionError: (failedCount) => {
      setImageConversionError(`${failedCount} image(s) failed to process. Please try again.`);
      // Clear error after 5 seconds
      setTimeout(() => setImageConversionError(null), 5000);
    }
  });

  // Handle initial user prompt - using a ref to prevent double execution
  const initialPromptProcessedRef = useRef(false);

  // Reset the ref when chatId changes
  useEffect(() => {
    initialPromptProcessedRef.current = false;
  }, [chatId]);

  useEffect(() => {
    // Check if we have a prompt to process and haven't processed it yet
    if (
      userPrompt &&
      localChat.messages.length === 0 &&
      phase === "idle" &&
      !initialPromptProcessedRef.current
    ) {
      // Mark as processed immediately
      initialPromptProcessedRef.current = true;

      // Capture values before clearing
      const prompt = userPrompt;
      const sysPrompt = systemPrompt;
      const images = userImages;

      // Clear state immediately
      setUserPrompt("");
      setSystemPrompt(null);
      setUserImages([]);

      // Send message with system prompt as separate parameter
      appendUserMessage(prompt, images, undefined, undefined, sysPrompt || undefined).catch(
        (error) => {
          // Only reset if it wasn't an abort
          if (!(error instanceof Error) || error.message !== "Stream aborted") {
            console.error("[ChatComponent] Failed to append message:", error);
            setUserPrompt(prompt);
            setSystemPrompt(sysPrompt);
            setUserImages(images);
            initialPromptProcessedRef.current = false;
          }
        }
      );
    }
  }, [
    userPrompt,
    systemPrompt,
    userImages,
    localChat.messages.length,
    phase,
    appendUserMessage,
    setUserPrompt,
    setSystemPrompt,
    setUserImages
  ]);

  // Handle mobile new chat (matching sidebar behavior)
  const handleMobileNewChat = useCallback(async () => {
    // If we're already on "/", focus the chat box
    if (location.pathname === "/") {
      document.getElementById("message")?.focus();
    } else {
      try {
        await navigate({ to: "/" });
        // Ensure element is available after navigation
        setTimeout(() => document.getElementById("message")?.focus(), 0);
      } catch (error) {
        console.error("Navigation failed:", error);
      }
    }
  }, [navigate, location.pathname]);

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

  const [isSidebarOpen, setIsSidebarOpen] = useState(false);

  const toggleSidebar = useCallback(() => setIsSidebarOpen((prev) => !prev), []);

  // Set model when chat first loads
  const hasSetModelRef = useRef(false);
  useEffect(() => {
    if (localChat.model && !hasSetModelRef.current) {
      setModel(localChat.model);
      hasSetModelRef.current = true;
    }
  }, [localChat.model, setModel]);

  // Reset the ref when chatId changes
  useEffect(() => {
    hasSetModelRef.current = false;
  }, [chatId]);

  // Removed auto-persist on model change to prevent unwanted saves
  // The model will be saved with the chat when messages are sent

  const isLoading = phase === "streaming";
  const isPersisting = phase === "persisting";

  // Auto-scroll when user sends message (new user message appears)
  const prevUserMessageCountRef = useRef(
    localChat.messages.filter((m) => m.role === "user").length
  );

  useEffect(() => {
    const userMessageCount = localChat.messages.filter((m) => m.role === "user").length;
    const hasNewUserMessage = userMessageCount > prevUserMessageCountRef.current;

    if (hasNewUserMessage) {
      // Scroll when user sends a message
      const container = chatContainerRef.current;
      if (container) {
        requestAnimationFrame(() => {
          container.scrollTo({
            top: container.scrollHeight,
            behavior: "smooth"
          });
        });
      }
    }

    prevUserMessageCountRef.current = userMessageCount;
  }, [localChat.messages]);

  // Auto-scroll when assistant starts streaming (currentStreamingMessage appears)
  const prevHadStreamingMessage = useRef(false);

  useEffect(() => {
    const hasStreamingMessage = !!currentStreamingMessage;
    const justStartedStreaming = hasStreamingMessage && !prevHadStreamingMessage.current;

    if (justStartedStreaming) {
      // Scroll when assistant starts streaming
      const container = chatContainerRef.current;
      if (container) {
        // Small delay to ensure the streaming message box is rendered
        setTimeout(() => {
          container.scrollTo({
            top: container.scrollHeight,
            behavior: "smooth"
          });
        }, 100);
      }
    }

    prevHadStreamingMessage.current = hasStreamingMessage;
  }, [currentStreamingMessage]);

  const sendMessage = useCallback(
    async (
      input: string,
      systemPrompt?: string,
      images?: File[],
      documentText?: string,
      documentMetadata?: { filename: string; fullContent: string }
    ) => {
      // Use the appendUserMessage from the hook with system prompt as separate parameter
      await appendUserMessage(input, images, documentText, documentMetadata, systemPrompt);
      // Note: Auto-scrolling is handled by the effect that watches for streaming start
    },
    [appendUserMessage]
  );

  // Chat compression function
  const compressChat = useCallback(async () => {
    try {
      setIsSummarizing(true);

      // 1. Build summarization prompt with detailed instructions
      const summarizerSystem = `You are "Summarizer-v1", an expert summarization assistant.

TASK
• First, write a concise paragraph (3–5 sentences) summarizing the overall conversation.
• Then, produce 10–20 markdown bullet points, each ≤ 30 words.
• Break complex ideas into multiple bullets for clarity.

CONTENT TO CAPTURE
1. **Key Information** – essential facts, data points, and statements.
2. **Decisions and Conclusions** – final choices or outcomes.
3. **Open Questions and Action Items** – unresolved issues or tasks to be completed.
4. **User Preferences and Priorities** – expressed likes, dislikes, or priorities.

STYLE & RULES
• Do **not** mention the assistant, the user, message counts, or dates.
• Do **not** quote anyone verbatim; paraphrase.
• Avoid passive voice; start each bullet with a strong noun or verb.
• Use present tense where possible ("Decide to migrate…", "User prefers…").
• No headings or extra text—*just* the paragraph summary followed by the bullet list.

END OF INSTRUCTIONS`;

      const summarizationMessages = [
        { role: "system" as const, content: summarizerSystem },
        ...localChat.messages.map((msg) => {
          // Convert content to string for summarization
          const content =
            typeof msg.content === "string"
              ? msg.content
              : msg.content.map((part) => (part.type === "text" ? part.text : "[image]")).join(" ");

          return {
            role: msg.role,
            content: content
          };
        })
      ];

      // 2. Stream the summary
      let summary = "";
      const stream = openai.beta.chat.completions.stream({
        model: DEFAULT_MODEL_ID, // Use the default model instead of user selected model
        messages: summarizationMessages,
        temperature: 0.3,
        max_tokens: 600,
        stream: true
      });

      for await (const chunk of stream) {
        summary += chunk.choices[0]?.delta?.content ?? "";
      }
      await stream.finalChatCompletion();

      // 3. Build initial message for the new chat
      const initialMsg =
        `Below is the summary of our previous chats:\n\n${summary}\n\n` +
        `I will follow up with additional conversations based on our previous chat summary`;

      // Try a completely different approach - work directly with storage

      // 1. First create a new chat with the title directly inherited from the original chat
      console.log("Creating new chat with summary from original chat");
      const inheritedTitle = localChat.title; // Use the exact same title as the original chat
      const id = await addChat(inheritedTitle);

      // 2. Completely reset user prompt
      setUserPrompt("");

      // 3. Take the direct storage approach instead of relying on React state/effects
      // Create a fake user message directly in storage that the next page will read
      const initialChatData = {
        id: id,
        title: inheritedTitle,
        messages: [{ role: "user" as const, content: initialMsg }]
      };

      // Explicitly persist this chat with the initial message directly
      await persistChat(initialChatData);

      // 4. Force refetch of both chat data and history list when navigating
      queryClient.invalidateQueries({
        queryKey: ["chat", id],
        refetchType: "all"
      });

      // Make sure history list is also invalidated to show the new chat
      queryClient.invalidateQueries({
        queryKey: ["chatHistory"],
        refetchType: "all"
      });

      // 5. Navigate to the new chat which should now have the initial message
      console.log("Navigating to new chat with pre-persisted message:", id);
      navigate({ to: "/chat/$chatId", params: { chatId: id } });
    } catch (e) {
      console.error("compressChat failed:", e);
      // Note: We don't have a setError function anymore since errors are handled by the hook
    } finally {
      setIsSummarizing(false);
    }
  }, [localChat, openai, addChat, navigate, setUserPrompt, persistChat, queryClient]);

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
          <div className="mt-4 md:mt-8 w-full h-10 flex items-center justify-center relative">
            {/* Mobile new chat button */}
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
              {localChat.title}
            </h2>
          </div>
          <div className="flex flex-col w-full max-w-[45rem] mx-auto gap-4 px-2 pt-4">
            {/* Show all messages including system messages */}
            {localChat.messages?.map((message, index) => (
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
                  <SystemMessage
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
            {currentStreamingMessage && (
              <div className="flex flex-col gap-2">
                <SystemMessage text={currentStreamingMessage} loading={isLoading} chatId={chatId} />
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
        <div className="w-full max-w-[45rem] mx-auto flex flex-col px-2 pb-2">
          {/* Display streaming error if present */}
          {streamingError && (
            <div className="mb-2 p-3 bg-destructive/10 text-destructive rounded-md text-sm">
              {streamingError}
            </div>
          )}
          <ChatBox
            onSubmit={sendMessage}
            messages={localChat.messages}
            isStreaming={isLoading || isPersisting || isSummarizing || isPending}
            onCompress={compressChat}
            isSummarizing={isSummarizing}
            imageConversionError={imageConversionError}
          />
        </div>
      </main>
    </div>
  );
}
