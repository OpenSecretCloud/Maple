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
  RotateCcw,
  Trash2,
  Edit3,
  GitBranch
} from "lucide-react";
import ChatBox from "@/components/ChatBox";
import { useOpenAI } from "@/ai/useOpenAi";
import { useLocalState } from "@/state/useLocalState";
import { Markdown, stripThinkingTags } from "@/components/markdown";
import { ChatMessage, Chat, DEFAULT_MODEL_ID } from "@/state/LocalStateContext";
import { AlertDestructive } from "@/components/AlertDestructive";
import { Sidebar, SidebarToggle } from "@/components/Sidebar";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Button } from "@/components/ui/button";
import { BillingStatus } from "@/billing/billingApi";
import { useNavigate, useLocation } from "@tanstack/react-router";
import { useIsMobile } from "@/utils/utils";
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "@/components/ui/tooltip";
import { Textarea } from "@/components/ui/textarea";
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

function UserMessage({
  text,
  chatId,
  messageIndex,
  onEdit,
  onDelete,
  onCopy,
  onFork
}: {
  text: string;
  chatId: string;
  messageIndex: number;
  onEdit: (index: number, newText: string) => void;
  onDelete: (index: number) => void;
  onCopy: (text: string) => void;
  onFork: (index: number) => void;
}) {
  const [isEditing, setIsEditing] = useState(false);
  const [editText, setEditText] = useState(text);
  const [isCopied, setIsCopied] = useState(false);

  const handleEdit = () => {
    if (isEditing) {
      onEdit(messageIndex, editText);
      setIsEditing(false);
    } else {
      setIsEditing(true);
    }
  };

  const handleCancel = () => {
    setEditText(text);
    setIsEditing(false);
  };

  const handleCopy = useCallback(async () => {
    try {
      await navigator.clipboard.writeText(text);
      onCopy(text);
      setIsCopied(true);
      setTimeout(() => setIsCopied(false), 2000);
    } catch (error) {
      console.error("Failed to copy text: ", error);
    }
  }, [text, onCopy]);

  const handleDelete = () => {
    onDelete(messageIndex);
  };

  const handleFork = () => {
    onFork(messageIndex);
  };
  return (
    <div className="group flex flex-col p-3 rounded-lg bg-muted">
      <div className="rounded-lg flex flex-col md:flex-row gap-4">
        <div>
          <UserIcon />
        </div>
        <div className="flex flex-col gap-2 flex-1">
          {isEditing ? (
            <div className="flex flex-col gap-2">
              <Textarea
                value={editText}
                onChange={(e) => setEditText(e.target.value)}
                className="min-h-[80px]"
                autoFocus
              />
              <div className="flex gap-2">
                <Button size="sm" onClick={handleEdit}>
                  Save
                </Button>
                <Button size="sm" variant="outline" onClick={handleCancel}>
                  Cancel
                </Button>
              </div>
            </div>
          ) : (
            <>
              <Markdown content={text} loading={false} chatId={chatId} />
              <TooltipProvider>
                <div className="flex gap-1">
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <Button
                        variant="ghost"
                        size="sm"
                        className="h-6 w-6 p-0"
                        onClick={handleCopy}
                        aria-label={isCopied ? "Copied" : "Copy message"}
                      >
                        {isCopied ? <Check className="h-3 w-3" /> : <Copy className="h-3 w-3" />}
                      </Button>
                    </TooltipTrigger>
                    <TooltipContent side="bottom" align="end" className="text-xs">
                      Copy
                    </TooltipContent>
                  </Tooltip>

                  <Tooltip>
                    <TooltipTrigger asChild>
                      <Button
                        variant="ghost"
                        size="sm"
                        className="h-6 w-6 p-0"
                        onClick={handleFork}
                        aria-label="Fork chat from here"
                      >
                        <GitBranch className="h-3 w-3" />
                      </Button>
                    </TooltipTrigger>
                    <TooltipContent side="bottom" align="end" className="text-xs">
                      Fork
                    </TooltipContent>
                  </Tooltip>

                  <AlertDialog>
                    <AlertDialogTrigger asChild>
                      <div>
                        <Tooltip>
                          <TooltipTrigger asChild>
                            <Button
                              variant="ghost"
                              size="sm"
                              className="h-6 w-6 p-0"
                              aria-label="Edit message"
                            >
                              <Edit3 className="h-3 w-3" />
                            </Button>
                          </TooltipTrigger>
                          <TooltipContent side="bottom" align="end" className="text-xs">
                            Edit
                          </TooltipContent>
                        </Tooltip>
                      </div>
                    </AlertDialogTrigger>
                    <AlertDialogContent>
                      <AlertDialogHeader>
                        <AlertDialogTitle>Edit Message</AlertDialogTitle>
                        <AlertDialogDescription>
                          Editing this message will clear all conversation that happened after it.
                          This action cannot be undone.
                        </AlertDialogDescription>
                      </AlertDialogHeader>
                      <AlertDialogFooter>
                        <AlertDialogCancel>Cancel</AlertDialogCancel>
                        <AlertDialogAction onClick={handleEdit}>
                          Edit & Clear Future Messages
                        </AlertDialogAction>
                      </AlertDialogFooter>
                    </AlertDialogContent>
                  </AlertDialog>

                  <AlertDialog>
                    <AlertDialogTrigger asChild>
                      <div>
                        <Tooltip>
                          <TooltipTrigger asChild>
                            <Button
                              variant="ghost"
                              size="sm"
                              className="h-6 w-6 p-0"
                              aria-label="Delete message"
                            >
                              <Trash2 className="h-3 w-3" />
                            </Button>
                          </TooltipTrigger>
                          <TooltipContent side="bottom" align="end" className="text-xs">
                            Delete
                          </TooltipContent>
                        </Tooltip>
                      </div>
                    </AlertDialogTrigger>
                    <AlertDialogContent>
                      <AlertDialogHeader>
                        <AlertDialogTitle>Delete Message</AlertDialogTitle>
                        <AlertDialogDescription>
                          Deleting this message will also clear all conversation that happened after
                          it. This action cannot be undone.
                        </AlertDialogDescription>
                      </AlertDialogHeader>
                      <AlertDialogFooter>
                        <AlertDialogCancel>Cancel</AlertDialogCancel>
                        <AlertDialogAction
                          onClick={handleDelete}
                          className="bg-destructive text-destructive-foreground hover:bg-destructive/90"
                        >
                          Delete & Clear Future Messages
                        </AlertDialogAction>
                      </AlertDialogFooter>
                    </AlertDialogContent>
                  </AlertDialog>
                </div>
              </TooltipProvider>
            </>
          )}
        </div>
      </div>
    </div>
  );
}

function SystemMessage({
  text,
  loading,
  chatId,
  messageIndex,
  onRegenerate,
  onDelete,
  onCopy,
  onFork
}: {
  text: string;
  loading?: boolean;
  chatId: string;
  messageIndex: number;
  onRegenerate: (index: number) => void;
  onDelete: (index: number) => void;
  onCopy: (text: string) => void;
  onFork: (index: number) => void;
}) {
  const textWithoutThinking = stripThinkingTags(text);
  const { isCopied, handleCopy } = useCopyToClipboard(textWithoutThinking);

  const handleRegenerate = () => {
    onRegenerate(messageIndex);
  };

  const handleDelete = () => {
    onDelete(messageIndex);
  };

  const handleFork = () => {
    onFork(messageIndex);
  };

  return (
    <div className="group flex flex-col p-3">
      <div className="rounded-lg flex flex-col md:flex-row gap-4">
        <div>
          <AsteriskIcon />
        </div>
        <div className="flex flex-col gap-2 flex-1">
          <Markdown content={text} loading={loading} chatId={chatId} />
          <TooltipProvider>
            <div className="flex gap-1">
              <Tooltip>
                <TooltipTrigger asChild>
                  <Button
                    variant="ghost"
                    size="sm"
                    className="h-6 w-6 p-0"
                    onClick={handleCopy}
                    aria-label={isCopied ? "Copied" : "Copy to clipboard"}
                  >
                    {isCopied ? <Check className="h-3 w-3" /> : <Copy className="h-3 w-3" />}
                  </Button>
                </TooltipTrigger>
                <TooltipContent side="bottom" align="end" className="text-xs">
                  Copy
                </TooltipContent>
              </Tooltip>

              <Tooltip>
                <TooltipTrigger asChild>
                  <Button
                    variant="ghost"
                    size="sm"
                    className="h-6 w-6 p-0"
                    onClick={handleFork}
                    aria-label="Fork chat from here"
                  >
                    <GitBranch className="h-3 w-3" />
                  </Button>
                </TooltipTrigger>
                <TooltipContent side="bottom" align="end" className="text-xs">
                  Fork
                </TooltipContent>
              </Tooltip>

              <AlertDialog>
                <AlertDialogTrigger asChild>
                  <div>
                    <Tooltip>
                      <TooltipTrigger asChild>
                        <Button
                          variant="ghost"
                          size="sm"
                          className="h-6 w-6 p-0"
                          aria-label="Regenerate response"
                        >
                          <RotateCcw className="h-3 w-3" />
                        </Button>
                      </TooltipTrigger>
                      <TooltipContent side="bottom" align="end" className="text-xs">
                        Regenerate
                      </TooltipContent>
                    </Tooltip>
                  </div>
                </AlertDialogTrigger>
                <AlertDialogContent>
                  <AlertDialogHeader>
                    <AlertDialogTitle>Regenerate Response</AlertDialogTitle>
                    <AlertDialogDescription>
                      Regenerating this response will clear all conversation that happened after it.
                      This action cannot be undone.
                    </AlertDialogDescription>
                  </AlertDialogHeader>
                  <AlertDialogFooter>
                    <AlertDialogCancel>Cancel</AlertDialogCancel>
                    <AlertDialogAction onClick={handleRegenerate}>
                      Regenerate & Clear Future Messages
                    </AlertDialogAction>
                  </AlertDialogFooter>
                </AlertDialogContent>
              </AlertDialog>

              <AlertDialog>
                <AlertDialogTrigger asChild>
                  <div>
                    <Tooltip>
                      <TooltipTrigger asChild>
                        <Button
                          variant="ghost"
                          size="sm"
                          className="h-6 w-6 p-0"
                          aria-label="Delete response"
                        >
                          <Trash2 className="h-3 w-3" />
                        </Button>
                      </TooltipTrigger>
                      <TooltipContent side="bottom" align="end" className="text-xs">
                        Delete
                      </TooltipContent>
                    </Tooltip>
                  </div>
                </AlertDialogTrigger>
                <AlertDialogContent>
                  <AlertDialogHeader>
                    <AlertDialogTitle>Delete Response</AlertDialogTitle>
                    <AlertDialogDescription>
                      Deleting this response will also clear all conversation that happened after
                      it. This action cannot be undone.
                    </AlertDialogDescription>
                  </AlertDialogHeader>
                  <AlertDialogFooter>
                    <AlertDialogCancel>Cancel</AlertDialogCancel>
                    <AlertDialogAction
                      onClick={handleDelete}
                      className="bg-destructive text-destructive-foreground hover:bg-destructive/90"
                    >
                      Delete & Clear Future Messages
                    </AlertDialogAction>
                  </AlertDialogFooter>
                </AlertDialogContent>
              </AlertDialog>
            </div>
          </TooltipProvider>
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
  const {
    model,
    setModel,
    persistChat,
    getChatById,
    userPrompt,
    setUserPrompt,
    systemPrompt,
    setSystemPrompt,
    addChat
  } = useLocalState();
  const openai = useOpenAI();
  const queryClient = useQueryClient();
  const [showScrollButton, setShowScrollButton] = useState(false);
  const navigate = useNavigate();
  const location = useLocation();
  const isMobile = useIsMobile();

  const [error, setError] = useState("");
  const [isSummarizing, setIsSummarizing] = useState(false);

  const chatContainerRef = useRef<HTMLDivElement>(null);

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

  // Track if we've already set the model for this chat
  const modelSetForChatRef = useRef<string | null>(null);

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

        // Build messages array with system prompt first (if exists), then user prompt
        const messages: ChatMessage[] = [];

        // Check for system prompt from LocalState
        if (systemPrompt?.trim()) {
          messages.push({ role: "system", content: systemPrompt.trim() } as ChatMessage);
        }

        // Add user prompt if exists
        if (userPrompt) {
          messages.push({ role: "user", content: userPrompt } as ChatMessage);
        }

        setLocalChat((localChat) => ({ ...localChat, messages }));
        return;
      }
      setLocalChat(queryChat);
    }
    // I don't want to re-run this effect if the user prompt or system prompt changes
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [queryChat, chatId, isPending]);

  useEffect(() => {
    if (queryChat && !isPending) {
      if (modelSetForChatRef.current !== chatId) {
        const chatModel = queryChat.model || DEFAULT_MODEL_ID;
        /** ① Set global selector ② also store on local chat state */
        setModel(chatModel);
        setLocalChat((prev) => ({ ...prev, model: chatModel }));
        modelSetForChatRef.current = chatId;
      }
    }
  }, [queryChat, chatId, isPending, setModel]);

  // IMPORTANT that this runs only once (because it uses the user's tokens!)
  const userPromptEffectRan = useRef(false);

  useEffect(() => {
    // Make sure we don't run this more than once per mount
    if (userPromptEffectRan.current) return;
    userPromptEffectRan.current = true;

    // Check if we have a user prompt to send
    if (userPrompt) {
      console.log("User prompt found for chatId:", chatId, "sending to chat");
      console.log("USER PROMPT:", userPrompt);

      // Set a small delay to ensure all state is properly initialized
      setTimeout(() => {
        sendMessage(userPrompt, systemPrompt || undefined);
      }, 100);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const [isLoading, setIsLoading] = useState(false);

  const sendMessage = useCallback(
    async (input: string, systemPrompt?: string) => {
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

          // Use the OpenAI API to generate a concise title - use the default model
          const stream = openai.beta.chat.completions.stream({
            model: DEFAULT_MODEL_ID, // Use the default model instead of user selected model
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
      if (!input.trim() || !localChat) return;
      setError("");

      // Build new messages array with system prompt if this is the first message
      let newMessages: ChatMessage[];

      if (localChat.messages.length === 0 && systemPrompt?.trim()) {
        // First message: add system prompt, then user message
        newMessages = [
          { role: "system", content: systemPrompt.trim() } as ChatMessage,
          { role: "user", content: input } as ChatMessage
        ];
      } else {
        // Subsequent messages: just add user message
        newMessages = [...localChat.messages, { role: "user", content: input } as ChatMessage];
      }

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
        // newMessages already contains system prompt if it was the first message

        const stream = openai.beta.chat.completions.stream({
          model,
          messages: newMessages,
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

        // Should be safe to clear these by now
        setUserPrompt("");
        setSystemPrompt(null);

        // React sucks and doesn't get the latest state
        // Use current title from localChat which may have been updated asynchronously
        const currentTitle = localChat.title === "New Chat" ? title : localChat.title;
        await persistChat({ ...localChat, model, title: currentTitle, messages: finalMessages });

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
    [localChat, model, openai, persistChat, queryClient, setUserPrompt, setSystemPrompt, chatId]
  );

  // Message action handlers
  const handleEditMessage = useCallback(
    async (messageIndex: number, newText: string) => {
      if (!localChat) return;

      // Get visible messages (non-system)
      const visibleMessages = localChat.messages.filter((m) => m.role !== "system");
      const messageToEdit = visibleMessages[messageIndex];

      // Find the actual index in the full messages array
      const actualIndex = localChat.messages.findIndex((m) => m === messageToEdit);

      if (actualIndex >= 0) {
        // Create new messages array - DESTRUCTIVE: clear all messages after this one
        const updatedMessages = [...localChat.messages];
        updatedMessages[actualIndex] = { ...updatedMessages[actualIndex], content: newText };

        // Remove all messages after the edited message
        const messagesUpToEdit = updatedMessages.slice(0, actualIndex + 1);

        // Update local state
        setLocalChat((prev) => ({ ...prev, messages: messagesUpToEdit }));

        // Persist the changes
        await persistChat({ ...localChat, messages: messagesUpToEdit });

        // Invalidate queries to reflect changes
        queryClient.invalidateQueries({
          queryKey: ["chat", chatId],
          refetchType: "all"
        });
      }
    },
    [localChat, persistChat, queryClient, chatId]
  );

  const handleDeleteMessage = useCallback(
    async (messageIndex: number) => {
      if (!localChat) return;

      // Get visible messages (non-system)
      const visibleMessages = localChat.messages.filter((m) => m.role !== "system");
      const messageToDelete = visibleMessages[messageIndex];

      // Find the actual index in the full messages array
      const actualIndex = localChat.messages.findIndex((m) => m === messageToDelete);

      if (actualIndex >= 0) {
        // DESTRUCTIVE: Remove the message and all messages after it
        const messagesUpToDelete = localChat.messages.slice(0, actualIndex);

        // Update local state
        setLocalChat((prev) => ({ ...prev, messages: messagesUpToDelete }));

        // Persist the changes
        await persistChat({ ...localChat, messages: messagesUpToDelete });

        // Invalidate queries to reflect changes
        queryClient.invalidateQueries({
          queryKey: ["chat", chatId],
          refetchType: "all"
        });
      }
    },
    [localChat, persistChat, queryClient, chatId]
  );

  const handleRegenerateMessage = useCallback(
    async (messageIndex: number) => {
      if (!localChat || isLoading) return;

      try {
        setIsLoading(true);
        setError("");

        // Get visible messages (non-system)
        const visibleMessages = localChat.messages.filter((m) => m.role !== "system");
        const assistantMessage = visibleMessages[messageIndex];

        // Find the actual index of this assistant message in the full messages array
        const assistantActualIndex = localChat.messages.findIndex((m) => m === assistantMessage);

        if (assistantActualIndex < 0) return;

        // DESTRUCTIVE: Clear out all messages at and after the assistant message being regenerated
        const messagesUpToBeforeAssistant = localChat.messages.slice(0, assistantActualIndex);

        // Update local state to remove the assistant message and everything after it
        setLocalChat((prev) => ({ ...prev, messages: messagesUpToBeforeAssistant }));

        // Regenerate from the existing conversation without adding a new user message
        const stream = openai.beta.chat.completions.stream({
          model,
          messages: messagesUpToBeforeAssistant,
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

        // Add the new assistant response
        const finalMessages = [
          ...messagesUpToBeforeAssistant,
          { role: "assistant", content: fullResponse } as ChatMessage
        ];

        setLocalChat((prev) => ({ ...prev, messages: finalMessages }));
        setCurrentStreamingMessage(undefined);

        // Persist the changes
        await persistChat({ ...localChat, messages: finalMessages });

        // Invalidate queries to reflect changes
        queryClient.invalidateQueries({
          queryKey: ["chat", chatId],
          refetchType: "all"
        });
      } catch (error) {
        console.error("Error regenerating message:", error);
        setError("Failed to regenerate message. Please try again.");
      } finally {
        setIsLoading(false);
      }
    },
    [localChat, isLoading, model, openai, persistChat, queryClient, chatId, setError]
  );

  const handleForkMessage = useCallback(
    async (messageIndex: number) => {
      if (!localChat) return;

      try {
        // Get visible messages (non-system)
        const visibleMessages = localChat.messages.filter((m) => m.role !== "system");
        const messageToForkFrom = visibleMessages[messageIndex];

        // Find the actual index in the full messages array
        const actualIndex = localChat.messages.findIndex((m) => m === messageToForkFrom);

        if (actualIndex >= 0) {
          // Create messages up to and including the fork point
          const messagesUpToFork = localChat.messages.slice(0, actualIndex + 1);

          // Create a new chat with the forked history
          const forkTitle = `${localChat.title} (Fork)`;
          const newChatId = await addChat(forkTitle);

          // Create the new chat with the forked messages
          const forkedChat: Chat = {
            id: newChatId,
            title: forkTitle,
            messages: messagesUpToFork,
            model: localChat.model
          };

          // Persist the forked chat
          await persistChat(forkedChat);

          // Invalidate queries to refresh chat list
          queryClient.invalidateQueries({
            queryKey: ["chatHistory"],
            refetchType: "all"
          });

          queryClient.invalidateQueries({
            queryKey: ["chat", newChatId],
            refetchType: "all"
          });

          // Navigate to the new forked chat
          navigate({ to: "/chat/$chatId", params: { chatId: newChatId } });
        }
      } catch (error) {
        console.error("Error forking chat:", error);
        setError("Failed to fork chat. Please try again.");
      }
    },
    [localChat, addChat, persistChat, queryClient, navigate, setError]
  );

  const handleCopyMessage = useCallback((text: string) => {
    // Copy functionality is handled within the individual message components
    console.log("Message copied:", text.substring(0, 50) + "...");
  }, []);

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
        ...localChat.messages
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
      const initialChatData: Chat = {
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

      // 5. Reset the flag for good measure
      userPromptEffectRan.current = false;

      // 6. Navigate to the new chat which should now have the initial message
      console.log("Navigating to new chat with pre-persisted message:", id);
      navigate({ to: "/chat/$chatId", params: { chatId: id } });
    } catch (e) {
      console.error("compressChat failed:", e);
      setError("Could not compress chat – please try again.");
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
            {localChat.messages?.map((message, index) => {
              // Get the index among visible (non-system) messages for edit/delete operations
              const visibleMessages = localChat.messages.filter((m) => m.role !== "system");
              const visibleIndex = visibleMessages.indexOf(message);
              
              return (
                <div
                  key={index}
                  id={`message-${message.role}-${index}`}
                  className="flex flex-col gap-2"
                >
                  {message.role === "system" && <SystemPromptMessage text={message.content} />}
                  {message.role === "user" && (
                    <UserMessage
                      text={message.content}
                      chatId={chatId}
                      messageIndex={visibleIndex}
                      onEdit={handleEditMessage}
                      onDelete={handleDeleteMessage}
                      onCopy={handleCopyMessage}
                      onFork={handleForkMessage}
                    />
                  )}
                  {message.role === "assistant" && (
                    <SystemMessage
                      text={message.content}
                      chatId={chatId}
                      messageIndex={visibleIndex}
                      onRegenerate={handleRegenerateMessage}
                      onDelete={handleDeleteMessage}
                      onCopy={handleCopyMessage}
                      onFork={handleForkMessage}
                    />
                  )}
                </div>
              );
            })}
            {(currentStreamingMessage || isLoading) && (
              <div className="flex flex-col gap-2">
                <SystemMessage
                  text={currentStreamingMessage || ""}
                  loading={isLoading}
                  chatId={chatId}
                  messageIndex={-1}
                  onRegenerate={() => {}}
                  onDelete={() => {}}
                  onCopy={handleCopyMessage}
                  onFork={() => {}}
                />
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
          <ChatBox
            onSubmit={sendMessage}
            messages={localChat.messages}
            isStreaming={isLoading || isSummarizing}
            onCompress={compressChat}
            isSummarizing={isSummarizing}
          />
        </div>
      </main>
    </div>
  );
}
