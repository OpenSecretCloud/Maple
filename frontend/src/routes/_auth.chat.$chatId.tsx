import { useEffect, useRef, useState, useCallback, useMemo } from "react";
import { createFileRoute } from "@tanstack/react-router";
import { AsteriskIcon, Check, Copy, UserIcon, ChevronDown } from "lucide-react";
import ChatBox from "@/components/ChatBox";
import { useOpenAI } from "@/ai/useOpenAi";
import { useLocalState } from "@/state/useLocalState";
import { Markdown } from "@/components/markdown";
import { ChatMessage, Chat, AssistantToolCall } from "@/state/LocalStateContextDef";
import { AlertDestructive } from "@/components/AlertDestructive";
import { Sidebar, SidebarToggle } from "@/components/Sidebar";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { InfoPopover } from "@/components/InfoPopover";
import { Button } from "@/components/ui/button";
import { BillingStatus } from "@/billing/billingApi";
import { ToolCallMessage } from "@/components/ToolCallMessage";
import { TOOL_DEFINITIONS } from "@/ai/tools";
import { toolExecutors, ToolName } from "@/ai/toolExecutors";

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
  const { model, persistChat, getChatById, userPrompt, setUserPrompt } = useLocalState();
  const openai = useOpenAI();
  const queryClient = useQueryClient();
  const [showScrollButton, setShowScrollButton] = useState(false);

  const [error, setError] = useState("");

  const chatContainerRef = useRef<HTMLDivElement>(null);

  // System prompt to guide the assistant's behavior - using useMemo to avoid recreating on each render
  const systemPrompt = useMemo(() => ({
    role: "system" as const,
    content:
      "You are a helpful, friendly assistant. Respond conversationally to the user's messages. Follow these rules carefully:\n\n1. DO NOT use tools unless absolutely necessary for specific computational tasks or when the user EXPLICITLY asks you to use a tool.\n\n2. For greetings like 'hello' or casual conversation, NEVER use tools - just have a natural conversation.\n\n3. For mathematical calculations (like addition, subtraction), DO use the appropriate tool directly without explaining that you're going to use it - just call the function.\n\n4. When calling tools, DO NOT show your reasoning or explain what you're doing - just make the function call directly.\n\n5. DO NOT say things like \"I'll use the add function\" or \"Here's the JSON for the function call\" - just make the actual function call.\n\n6. After receiving tool results, present them clearly without technical explanations.\n\nRemember: For math questions, call tools directly. For conversation, never use tools."
  }), []);

  // Helper function to execute tool calls and get follow-up completion
  const executeToolsAndGetCompletion = useCallback(
    async (
      toolCalls: AssistantToolCall[],
      messagesWithToolCalls: ChatMessage[]
    ): Promise<{
      finalMessages: ChatMessage[];
      finalText: string;
    }> => {
      // Log the tool calls for debugging
      console.log("Executing tool calls:", JSON.stringify(toolCalls, null, 2));

      // Execute each tool call
      for (const call of toolCalls) {
        try {
          console.log(
            `Processing tool call: ${call.function.name} with args: ${call.function.arguments}`
          );

          // Check if tool call has all required fields
          if (!call.id || !call.function?.name) {
            console.error("Invalid tool call format:", call);
            messagesWithToolCalls.push({
              role: "tool",
              tool_call_id: call.id || `error-${Date.now()}`,
              content: JSON.stringify({ error: "Invalid tool call format" })
            } as ChatMessage);
            continue;
          }

          // Ensure arguments are valid JSON
          let args;
          try {
            args = JSON.parse(call.function.arguments || "{}");
          } catch (parseError) {
            console.error("Failed to parse arguments:", call.function.arguments, parseError);
            args = {};
          }

          const executor = toolExecutors[call.function.name as ToolName];
          if (!executor) {
            console.warn(`Unknown tool: ${call.function.name}`);
          }

          const result = executor
            ? executor(args)
            : { error: `Unknown tool: ${call.function.name}` };
          console.log(`Tool execution result:`, result);

          // Add the tool result to the message list
          messagesWithToolCalls.push({
            role: "tool",
            tool_call_id: call.id,
            content: JSON.stringify(result)
          } as ChatMessage);
        } catch (err) {
          const error = err as Error;
          console.error(`Error executing tool ${call.function.name}:`, error);
          messagesWithToolCalls.push({
            role: "tool",
            tool_call_id: call.id || `error-${Date.now()}`,
            content: JSON.stringify({
              error: `Tool execution failed: ${error.message || "Unknown error"}`
            })
          } as ChatMessage);
        }
      }

      // Get follow-up completion from the model - include systemPrompt for behavior guardrails
      console.log("Getting final response after tool execution...");
      const toolResponseStream = await openai.beta.chat.completions.stream({
        model,
        messages: [
          // Add system prompt to guide behavior
          systemPrompt,
          // Map messages including tool calls and results
          ...messagesWithToolCalls.map((msg) => {
            if (msg.role === "assistant" && "tool_calls" in msg && msg.tool_calls) {
              return {
                role: "assistant" as const,
                content: "",
                stop_reason: "tool_calls",
                tool_calls: msg.tool_calls.map((tc) => ({
                  id: tc.id,
                  type: "function" as const,
                  function: {
                    name: tc.function.name,
                    arguments: tc.function.arguments
                  }
                }))
              };
            } else if (msg.role === "tool") {
              return {
                role: "tool" as const,
                tool_call_id: msg.tool_call_id,
                content: msg.content
              };
            } else {
              return {
                role: msg.role as "user" | "assistant" | "system",
                content: msg.content || ""
              };
            }
          })
        ],
        tool_choice: "none", // Explicitly disable further tool use
        temperature: 0.9,
        top_p: 1,
        stream: true
      });

      // Stream in the final response after tool execution
      let finalText = "";
      for await (const chunk of toolResponseStream) {
        const content = chunk.choices[0]?.delta?.content || "";
        finalText += content;
        setCurrentStreamingMessage(finalText);
      }

      // Get the final completion
      const finalCompletion = await toolResponseStream.finalChatCompletion();
      console.log("Final text after tool execution:", finalCompletion);

      // Extract content from the completion
      finalText = finalCompletion.choices[0].message.content || "";

      // Construct final message list
      const finalMessages = [
        ...messagesWithToolCalls,
        { role: "assistant", content: finalText } as ChatMessage
      ];

      return { finalMessages, finalText };
    },
    [model, openai, systemPrompt]
  );

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
        if (!userMessage || !("content" in userMessage)) return "New Chat";

        // Simple title generation - truncate first message to 50 chars
        const simpleTitleFromMessage = userMessage.content!.slice(0, 50).trim();

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
          // We already checked that userMessage and userMessage.content exist above
          const userContent = userMessage.content!.slice(0, 500); // Reduced to 500 chars to optimize token usage

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
      if (!input.trim() || !localChat) return;
      setError("");

      const newMessages = [...localChat.messages, { role: "user", content: input } as ChatMessage];

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
        const stream = openai.beta.chat.completions.stream({
          model,
          messages: [
            // Add system prompt first
            systemPrompt,
            // Include the actual user messages
            ...newMessages.map((msg) => ({
              role: msg.role as "user" | "assistant" | "system",
              content: msg.content || ""
            }))
          ],
          tools: TOOL_DEFINITIONS,
          tool_choice: "auto",
          temperature: 0.9,
          top_p: 1,
          stream: true
        });

        let fullResponse = "";
        let isFirstChunk = true;
        let toolCalls: AssistantToolCall[] | undefined;

        // Track if we've seen any tool calls
        let hasToolCall = false;

        for await (const chunk of stream) {
          const delta = chunk.choices[0]?.delta;

          // Just detect if there's a tool call, but we won't process it incrementally
          if (delta?.tool_calls && !hasToolCall) {
            console.log("Detected tool call, will wait for final completion");
            hasToolCall = true;
            // Don't break - we'll continue to collect normal content until completion
          }

          const content = delta?.content || "";
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

        // Handle tool calls if present
        if (toolCalls && toolCalls.length > 0) {
          // 1. Render placeholder (tool call message)
          setLocalChat((prev) => ({
            ...prev,
            messages: [
              ...prev.messages,
              { role: "assistant", tool_calls: toolCalls } as ChatMessage
            ]
          }));
          setCurrentStreamingMessage(undefined);

          // 2. Prepare messages with tool calls
          const messagesWithToolCalls = [
            ...newMessages,
            { role: "assistant", tool_calls: toolCalls } as ChatMessage
          ];

          try {
            // 3. Execute tools and get completion using the helper function
            const { finalMessages } = await executeToolsAndGetCompletion(
              toolCalls,
              messagesWithToolCalls
            );

            // Log the tool call sequence to help debugging
            console.log("Tool Call Sequence:", [
              "Original message count:",
              messagesWithToolCalls.length,
              "Tool calls executed:",
              toolCalls.length,
              "Final messages with tool results:",
              finalMessages.length
            ]);

            // Update the local state
            setLocalChat((prev) => ({
              ...prev,
              messages: finalMessages
            }));

            // Create an updated chat reference with the latest messages
            const updatedToolChat = {
              ...localChat,
              messages: finalMessages,
              title: localChat.title
            };

            // Persist the chat with the updated messages
            await persistChat(updatedToolChat);
            return; // Exit early since we've handled the flow
          } catch (error) {
            console.error("Error getting final response after tool call:", error);
            setError("Failed to get response after tool execution");
          }
        } else {
          // Normal flow without tool calls
          const finalMessagesList = [
            ...newMessages,
            { role: "assistant", content: fullResponse } as ChatMessage
          ];

          // Update the local chat state
          setLocalChat((prev) => ({
            ...prev,
            messages: finalMessagesList
          }));

          // We'll save the finalMessagesList for later persistence

          // We'll handle the actual persistence later with the title
          setCurrentStreamingMessage(undefined);
        }

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

        // Get the complete final response
        const chatCompletion = await stream.finalChatCompletion();
        console.log("Final completion:", chatCompletion);

        // If we detected a tool call during streaming, extract complete tool calls and execute them
        if (hasToolCall && chatCompletion.choices[0].message.tool_calls) {
          // Extract properly formatted tool calls from the completion
          toolCalls = chatCompletion.choices[0].message.tool_calls.map((tc) => ({
            id: tc.id,
            type: "function" as const,
            function: {
              name: tc.function.name,
              arguments: tc.function.arguments || "{}"
            }
          }));
          console.log("Extracted complete tool calls:", JSON.stringify(toolCalls, null, 2));

          // Clear streaming message since we'll show tool calls instead
          setCurrentStreamingMessage(undefined);

          // 1. Render placeholder (tool call message)
          setLocalChat((prev) => ({
            ...prev,
            messages: [
              ...prev.messages,
              { role: "assistant", tool_calls: toolCalls } as ChatMessage
            ]
          }));

          // 2. Prepare messages with tool calls
          const messagesWithToolCalls = [
            ...newMessages,
            { role: "assistant", tool_calls: toolCalls } as ChatMessage
          ];

          try {
            // Show updated tool call results in the UI immediately
            setLocalChat((prev) => ({
              ...prev,
              messages: messagesWithToolCalls
            }));

            // 3. Execute tools and get completion using the helper function
            const { finalMessages } = await executeToolsAndGetCompletion(
              toolCalls,
              messagesWithToolCalls
            );

            // Create an updated chat reference with the latest messages
            const updatedToolChat = {
              ...localChat,
              messages: finalMessages,
              title: localChat.title
            };

            // Update the local state
            setLocalChat((prev) => ({
              ...prev,
              messages: finalMessages
            }));

            // Persist the chat with the updated messages
            await persistChat(updatedToolChat);

            // Reset streaming message
            setCurrentStreamingMessage(undefined);
          } catch (error) {
            console.error("Error getting final response after tool call:", error);
            setError("Failed to get response after tool execution");
          }
        }

        // Should be safe to clear this by now
        setUserPrompt("");

        // React sucks and doesn't get the latest state
        // Use current title from localChat which may have been updated asynchronously
        const currentTitle = localChat.title === "New Chat" ? title : localChat.title;

        // In the tool calling case, we've already persisted the chat
        // Only persist here for the non-tool flow
        if (!hasToolCall) {
          // Build the final message list for non-tool flow
          const finalMessages = [
            ...newMessages,
            { role: "assistant", content: fullResponse } as ChatMessage
          ];

          // For the non-tool flow, use these final messages
          await persistChat({
            ...localChat,
            messages: finalMessages,
            title: currentTitle
          });
        }

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
    [localChat, model, openai, persistChat, queryClient, setUserPrompt, chatId, executeToolsAndGetCompletion, systemPrompt]
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
                {message.role === "assistant" && message.content && (
                  <SystemMessage text={message.content} />
                )}
                {message.role === "assistant" && "tool_calls" in message && message.tool_calls && (
                  <div className="flex flex-col gap-1">
                    {message.tool_calls.map((tc: AssistantToolCall) => {
                      // Find the corresponding tool result for this tool call
                      const toolResult = localChat.messages.find(
                        (m) => m.role === "tool" && m.tool_call_id === tc.id
                      );

                      // A tool call is pending only if this specific tool call doesn't have a result yet
                      // We should NOT use the global isLoading state which affects all tool calls,
                      // including ones from previous conversations
                      const isPending = !toolResult;

                      return (
                        <ToolCallMessage
                          key={tc.id}
                          call={tc}
                          pending={isPending}
                          result={toolResult?.content}
                        />
                      );
                    })}
                  </div>
                )}
                {/* We hide the tool messages since they're now shown in the tool call component */}
                {message.role === "tool" && <div className="hidden">{message.content}</div>}
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
