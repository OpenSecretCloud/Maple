/**
 * UnifiedChat Component
 *
 * This is the main chat interface that replaced the old ChatBox component.
 * It uses the OpenAI Conversations/Responses API instead of localStorage.
 *
 * Recent changes (2025-01-25):
 * - Simplified type system to use OpenAI's native types directly
 * - Content is always an array of proper OpenAI content types (no more string | array)
 * - Uses ConversationContent union for all message content
 * - ModelSelector refactored to just take hasImages boolean instead of full messages
 *
 * Old files to handle:
 * - DELETE: frontend/src/components/ChatBox.tsx (replaced by this component)
 * - DELETE: frontend/src/hooks/useChatSession.ts (old localStorage chat management)
 * - DELETE: frontend/src/routes/index.backup.tsx (just a backup)
 * - SIMPLIFY: frontend/src/routes/_auth.chat.$chatId.tsx
 *   - Make read-only for viewing old localStorage chats
 *   - Remove all interaction code
 *   - Add "archived chat" banner
 *   - Keep for backwards compatibility with chat history
 */
import { useState, useRef, useEffect, useCallback, memo, useMemo } from "react";
import { flushSync } from "react-dom";
import {
  ArrowUp,
  Copy,
  Check,
  Plus,
  Image,
  FileText,
  X,
  Mic,
  SquarePen,
  Search,
  Loader2,
  Globe,
  Expand,
  Shrink,
  Volume2,
  Square,
  LockKeyhole
} from "lucide-react";
import RecordRTC from "recordrtc";
import { useQueryClient } from "@tanstack/react-query";
import { v4 as uuidv4 } from "uuid";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import { Sidebar, SidebarToggle } from "@/components/Sidebar";
import { MapleWordmark } from "@/components/MapleWordmark";
import { useIsMobile, useIsLandscapeMobile } from "@/utils/utils";
import { fileToDataURL } from "@/utils/file";
import { truncateMarkdownPreservingLinks } from "@/utils/markdown";
import { useOpenAI } from "@/ai/useOpenAi";
import { DEFAULT_MODEL_ID, getInitialWebSearchEnabled } from "@/state/LocalStateContext";
import { Markdown, ThinkingBlock } from "@/components/markdown";
import { ModelSelector } from "@/components/ModelSelector";
import { useLocalState } from "@/state/useLocalState";
import { useOpenSecret } from "@opensecret/react";
import { UpgradePromptDialog } from "@/components/UpgradePromptDialog";
import { DocumentPlatformDialog } from "@/components/DocumentPlatformDialog";
import { ContextLimitDialog } from "@/components/ContextLimitDialog";
import { RecordingOverlay } from "@/components/RecordingOverlay";
import { TTSDownloadDialog } from "@/components/TTSDownloadDialog";
import { useTTS } from "@/services/tts/TTSContext";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { AlertCircle } from "lucide-react";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger
} from "@/components/ui/dropdown-menu";
import { isTauri } from "@/utils/platform";
import { ConversationProjectPicker } from "@/components/ConversationProjectPicker";
import {
  getSidebarLayoutStyle,
  SIDEBAR_AWARE_FIXED_CENTER_CLASS,
  SIDEBAR_GRID_COLUMNS_CLASS
} from "@/constants/layout";
import type {
  InputTextContent,
  OutputTextContent,
  TextContent,
  SummaryTextContent,
  RefusalContent,
  InputImageContent,
  ComputerScreenshotContent,
  InputFileContent
} from "openai/resources/conversations/conversations.js";
import type {
  ResponseFunctionWebSearch,
  ResponseFunctionToolCall,
  ResponseFunctionToolCallOutputItem,
  ResponseOutputItemAddedEvent,
  ResponseOutputItemDoneEvent,
  ResponseReasoningItem,
  ResponseReasoningTextDeltaEvent,
  ResponseReasoningTextDoneEvent,
  ResponseTextDeltaEvent,
  ResponseTextDoneEvent
} from "openai/resources/responses/responses.js";
import type { Message as OpenAIMessage } from "openai/resources/conversations/conversations.js";

const CHAT_ALERT_CLASS = `fixed top-16 left-1/2 -translate-x-1/2 z-50 w-full max-w-2xl px-4 ${SIDEBAR_AWARE_FIXED_CENTER_CLASS}`;

type ConversationContent =
  | InputTextContent
  | OutputTextContent
  | TextContent
  | SummaryTextContent
  | RefusalContent
  | InputImageContent
  | ComputerScreenshotContent
  | InputFileContent
  | ResponseFunctionWebSearch
  | ToolCallItem
  | ToolOutputItem;

type MessageStatus = "completed" | "in_progress" | "incomplete" | "streaming" | "error";
type ExtendedMessage = Omit<OpenAIMessage, "status"> & {
  status?: MessageStatus;
};

type ReasoningContentItem = { type: "reasoning_text"; text: string };
type ReasoningItem = Omit<ResponseReasoningItem, "content" | "status"> & {
  content?: ReasoningContentItem[];
  status?: MessageStatus;
};

type ToolCallItem = {
  id: string;
  type: "tool_call";
  call_id: string;
  name: string;
  arguments: string;
  status?: MessageStatus;
};

type ToolOutputItem = {
  id: string;
  type: "tool_output";
  call_id: string;
  output: string;
  status?: MessageStatus;
};

// Union type for all possible conversation items (messages, tool calls, tool outputs, web search, reasoning)
// This combines OpenAI's native types with response streaming types
type Message =
  | ExtendedMessage
  | (ResponseFunctionWebSearch & { id: string })
  | ToolCallItem
  | ToolOutputItem
  | ReasoningItem;

// Helper function to merge messages while ensuring uniqueness by ID
// This prevents duplicate key warnings in React by deduplicating messages
function mergeMessagesById(existingMessages: Message[], newMessages: Message[]): Message[] {
  const messagesMap = new Map<string, Message>();

  // First, add all existing messages
  existingMessages.forEach((msg) => messagesMap.set(msg.id, msg));

  // Then, add/update with new messages (overwrites if ID already exists)
  newMessages.forEach((msg) => messagesMap.set(msg.id, msg));

  // Return as array, maintaining insertion order (Map preserves insertion order)
  return Array.from(messagesMap.values());
}

function MapleChatAvatar() {
  return (
    <img
      src="/m-avatar.svg"
      alt=""
      width={32}
      height={32}
      draggable={false}
      className="h-8 w-8 shrink-0 select-none"
    />
  );
}

function updateMessageById(
  messages: Message[],
  messageId: string,
  updater: (message: Message) => Message
): Message[] {
  const messageToUpdate = messages.find((message) => message.id === messageId);
  if (!messageToUpdate) return messages;
  return mergeMessagesById(messages, [updater(messageToUpdate)]);
}

function upsertAssistantTextContent(
  message: ExtendedMessage,
  contentIndex: number,
  text: string,
  status?: ExtendedMessage["status"]
): ExtendedMessage {
  const content = [...(message.content ?? [])];
  const existingPart = content[contentIndex];

  if (
    existingPart &&
    (existingPart.type === "input_text" ||
      existingPart.type === "output_text" ||
      existingPart.type === "text") &&
    "text" in existingPart
  ) {
    content[contentIndex] = {
      ...existingPart,
      text
    };
  } else {
    content[contentIndex] = {
      type: "output_text",
      text,
      annotations: []
    };
  }

  return {
    ...message,
    content,
    ...(status ? { status } : {})
  };
}

function normalizeReasoningItem(item: ResponseReasoningItem | ReasoningItem): ReasoningItem {
  const summary = Array.isArray(item.summary) ? item.summary : [];
  const contentItems = Array.isArray(item.content) ? item.content : [];
  const content = (contentItems.length > 0 ? contentItems : summary)
    .map((contentItem) =>
      typeof contentItem?.text === "string"
        ? ({
            type: "reasoning_text",
            text: contentItem.text
          } as const)
        : null
    )
    .filter((contentItem): contentItem is ReasoningContentItem => contentItem !== null);

  return {
    ...item,
    summary,
    content
  };
}

function normalizeToolCallItem(item: unknown): ToolCallItem | null {
  if (!item || typeof item !== "object" || !("type" in item)) return null;

  if ((item as { type?: string }).type === "tool_call") {
    const toolCall = item as Partial<ToolCallItem>;
    if (typeof toolCall.id !== "string" || typeof toolCall.call_id !== "string") return null;

    return {
      id: toolCall.id,
      type: "tool_call",
      call_id: toolCall.call_id,
      name: typeof toolCall.name === "string" ? toolCall.name : "function",
      arguments:
        typeof toolCall.arguments === "string"
          ? toolCall.arguments
          : JSON.stringify(toolCall.arguments || {}),
      status: toolCall.status
    };
  }

  if ((item as { type?: string }).type === "function_call") {
    const toolCall = item as ResponseFunctionToolCall & { id: string };
    if (!toolCall.id) return null;

    return {
      id: toolCall.id,
      type: "tool_call",
      call_id: toolCall.call_id,
      name: toolCall.name,
      arguments: toolCall.arguments,
      status: toolCall.status
    };
  }

  return null;
}

function normalizeToolOutputItem(item: unknown): ToolOutputItem | null {
  if (!item || typeof item !== "object" || !("type" in item)) return null;

  if ((item as { type?: string }).type === "tool_output") {
    const toolOutput = item as Partial<ToolOutputItem>;
    if (typeof toolOutput.id !== "string" || typeof toolOutput.call_id !== "string") return null;

    return {
      id: toolOutput.id,
      type: "tool_output",
      call_id: toolOutput.call_id,
      output:
        typeof toolOutput.output === "string"
          ? toolOutput.output
          : JSON.stringify(toolOutput.output || ""),
      status: toolOutput.status
    };
  }

  if ((item as { type?: string }).type === "function_call_output") {
    const toolOutput = item as ResponseFunctionToolCallOutputItem & { id: string };
    if (!toolOutput.id) return null;

    return {
      id: toolOutput.id,
      type: "tool_output",
      call_id: toolOutput.call_id,
      output: toolOutput.output,
      status: toolOutput.status
    };
  }

  return null;
}

function isToolCallItem(item: Message): item is ToolCallItem {
  return item.type === "tool_call";
}

function isToolOutputItem(item: Message): item is ToolOutputItem {
  return item.type === "tool_output";
}

function toolOutputHasResult(item: ToolOutputItem): boolean {
  return item.status === "completed" || item.output.length > 0;
}

function getReasoningContentLength(content?: ReasoningContentItem[]): number {
  return (content ?? []).reduce((total, contentItem) => total + contentItem.text.length, 0);
}

function getMessageContentLength(content?: ExtendedMessage["content"]): number {
  return (content ?? []).reduce((total, contentItem) => {
    if (
      (contentItem.type === "input_text" ||
        contentItem.type === "output_text" ||
        contentItem.type === "text") &&
      "text" in contentItem
    ) {
      return total + contentItem.text.length;
    }

    if (contentItem.type === "input_image") {
      return total + 1;
    }

    return total;
  }, 0);
}

function upsertReasoningTextContent(
  reasoning: ReasoningItem,
  contentIndex: number,
  text: string,
  status?: ReasoningItem["status"]
): ReasoningItem {
  const content = [...(reasoning.content ?? [])];
  content[contentIndex] = {
    type: "reasoning_text",
    text
  };

  return {
    ...reasoning,
    content,
    ...(status ? { status } : {})
  };
}

function normalizeConversationItem(item: unknown): Message | null {
  const toolCall = normalizeToolCallItem(item);
  if (toolCall) return toolCall;

  const toolOutput = normalizeToolOutputItem(item);
  if (toolOutput) return toolOutput;

  if (!item || typeof item !== "object" || !("id" in item) || !("type" in item)) {
    return null;
  }

  const typedItem = item as { type: string };

  if (typedItem.type === "reasoning") {
    return normalizeReasoningItem(item as ResponseReasoningItem | ReasoningItem);
  }

  if (typedItem.type === "message" || typedItem.type === "web_search_call") {
    return item as Message;
  }

  return null;
}

function isAssistantConversationItem(item: Message): boolean {
  return item.type !== "message" || (item as ExtendedMessage).role !== "user";
}

function summarizeConversationItemForLog(item: unknown): Record<string, unknown> {
  const normalizedItem = normalizeConversationItem(item);

  if (!normalizedItem) {
    return {};
  }

  if (normalizedItem.type === "reasoning") {
    return {
      itemType: normalizedItem.type,
      itemId: normalizedItem.id,
      status: normalizedItem.status,
      contentLength: getReasoningContentLength(normalizedItem.content)
    };
  }

  if (normalizedItem.type === "message") {
    const message = normalizedItem as ExtendedMessage;

    return {
      itemType: message.type,
      itemId: message.id,
      role: message.role,
      status: message.status,
      contentParts: message.content?.length ?? 0,
      contentLength: getMessageContentLength(message.content)
    };
  }

  if (normalizedItem.type === "web_search_call") {
    return {
      itemType: normalizedItem.type,
      itemId: normalizedItem.id,
      status: normalizedItem.status
    };
  }

  if (normalizedItem.type === "tool_call") {
    return {
      itemType: normalizedItem.type,
      itemId: normalizedItem.id,
      callId: normalizedItem.call_id,
      name: normalizedItem.name,
      status: normalizedItem.status,
      argumentsLength: normalizedItem.arguments.length
    };
  }

  return {
    itemType: normalizedItem.type,
    itemId: normalizedItem.id,
    callId: normalizedItem.call_id,
    status: normalizedItem.status,
    outputLength: normalizedItem.output.length
  };
}

function summarizeStreamEventForLog(eventType: string, event: unknown): Record<string, unknown> {
  const summary: Record<string, unknown> = {};
  const eventRecord = event as
    | {
        sequence_number?: number;
        item_id?: string;
        response?: {
          id?: string;
          status?: string;
          output?: unknown[];
        };
      }
    | undefined;

  if (typeof eventRecord?.sequence_number === "number") {
    summary.sequenceNumber = eventRecord.sequence_number;
  }

  if (typeof eventRecord?.item_id === "string") {
    summary.itemId = eventRecord.item_id;
  }

  switch (eventType) {
    case "response.created":
    case "response.completed": {
      const response = eventRecord?.response;

      if (response?.id) {
        summary.responseId = response.id;
      }

      if (typeof response?.status === "string") {
        summary.status = response.status;
      }

      if (Array.isArray(response?.output)) {
        summary.outputCount = response.output.length;
      }

      return summary;
    }
    case "response.output_item.added":
    case "response.output_item.done": {
      const itemEvent = event as { output_index?: number; item?: unknown };

      if (typeof itemEvent.output_index === "number") {
        summary.outputIndex = itemEvent.output_index;
      }

      return {
        ...summary,
        ...summarizeConversationItemForLog(itemEvent.item)
      };
    }
    case "response.reasoning_text.delta": {
      const reasoningEvent = event as ResponseReasoningTextDeltaEvent;

      return {
        ...summary,
        contentIndex: reasoningEvent.content_index,
        deltaLength: reasoningEvent.delta.length
      };
    }
    case "response.reasoning_text.done": {
      const reasoningEvent = event as ResponseReasoningTextDoneEvent;

      return {
        ...summary,
        contentIndex: reasoningEvent.content_index,
        textLength: reasoningEvent.text.length
      };
    }
    case "response.output_text.delta": {
      const textEvent = event as ResponseTextDeltaEvent;

      return {
        ...summary,
        contentIndex: textEvent.content_index,
        deltaLength: textEvent.delta.length
      };
    }
    case "response.output_text.done": {
      const textEvent = event as ResponseTextDoneEvent;

      return {
        ...summary,
        contentIndex: textEvent.content_index,
        textLength: textEvent.text.length
      };
    }
    case "tool_call.created": {
      const toolCallEvent = event as {
        tool_call_id?: string;
        name?: string;
        arguments?: string | Record<string, unknown>;
      };

      return {
        ...summary,
        toolCallId: toolCallEvent.tool_call_id,
        name: toolCallEvent.name,
        ...(typeof toolCallEvent.arguments === "string"
          ? { argumentsLength: toolCallEvent.arguments.length }
          : toolCallEvent.arguments && typeof toolCallEvent.arguments === "object"
            ? { argumentKeys: Object.keys(toolCallEvent.arguments) }
            : {})
      };
    }
    case "tool_output.created": {
      const toolOutputEvent = event as {
        tool_output_id?: string;
        tool_call_id?: string;
        output?: string;
      };

      return {
        ...summary,
        toolOutputId: toolOutputEvent.tool_output_id,
        toolCallId: toolOutputEvent.tool_call_id,
        outputLength: toolOutputEvent.output?.length ?? 0
      };
    }
    default:
      return summary;
  }
}

function updateActiveItemStatuses(messages: Message[], status: "error" | "incomplete"): Message[] {
  const updatedMessages = messages
    .filter((message) => {
      const currentStatus = (message as { status?: string }).status;
      return (
        currentStatus === "in_progress" ||
        currentStatus === "streaming" ||
        currentStatus === "searching"
      );
    })
    .map((message) => ({ ...message, status }) as Message);

  return updatedMessages.length > 0 ? mergeMessagesById(messages, updatedMessages) : messages;
}

function mergeStreamingConversationItem(messages: Message[], item: Message): Message[] {
  if (item.type === "reasoning") {
    const existingReasoning = messages.find(
      (message): message is ReasoningItem => message.id === item.id && message.type === "reasoning"
    );

    if (!existingReasoning) {
      return mergeMessagesById(messages, [item]);
    }

    const incomingReasoning = item as ReasoningItem;
    const existingContentLength = getReasoningContentLength(existingReasoning.content);
    const incomingContentLength = getReasoningContentLength(incomingReasoning.content);

    return mergeMessagesById(messages, [
      {
        ...existingReasoning,
        ...incomingReasoning,
        content:
          incomingContentLength >= existingContentLength
            ? incomingReasoning.content
            : existingReasoning.content,
        status: incomingReasoning.status ?? existingReasoning.status
      }
    ]);
  }

  if (item.type === "message") {
    const existingMessage = messages.find(
      (message): message is ExtendedMessage => message.id === item.id && message.type === "message"
    );

    if (!existingMessage) {
      return mergeMessagesById(messages, [item]);
    }

    const incomingMessage = item as ExtendedMessage;
    const existingContentLength = getMessageContentLength(existingMessage.content);
    const incomingContentLength = getMessageContentLength(incomingMessage.content);

    return mergeMessagesById(messages, [
      {
        ...existingMessage,
        ...incomingMessage,
        content:
          incomingContentLength >= existingContentLength
            ? incomingMessage.content
            : existingMessage.content,
        status: incomingMessage.status ?? existingMessage.status
      } as Message
    ]);
  }

  if (isToolCallItem(item)) {
    const existingToolCall = messages.find(
      (message): message is ToolCallItem => message.id === item.id && isToolCallItem(message)
    );

    return mergeMessagesById(messages, [
      {
        ...(existingToolCall ?? {}),
        ...item,
        arguments:
          item.arguments.length >= (existingToolCall?.arguments.length ?? 0)
            ? item.arguments
            : (existingToolCall?.arguments ?? item.arguments),
        status: item.status ?? existingToolCall?.status ?? "in_progress"
      }
    ]);
  }

  if (isToolOutputItem(item)) {
    const existingToolOutput = messages.find(
      (message): message is ToolOutputItem => message.id === item.id && isToolOutputItem(message)
    );

    return mergeMessagesById(messages, [
      {
        ...(existingToolOutput ?? {}),
        ...item,
        output:
          item.output.length >= (existingToolOutput?.output.length ?? 0)
            ? item.output
            : (existingToolOutput?.output ?? item.output),
        status: item.status ?? existingToolOutput?.status ?? "in_progress"
      }
    ]);
  }

  return mergeMessagesById(messages, [item]);
}

// Helper function to convert conversation items - just returns them as-is (flat, no grouping)
// The API already returns items in the correct format (ConversationItem union)
function convertItemsToMessages(items: Array<unknown>): Message[] {
  return items.flatMap((item) => {
    const normalizedItem = normalizeConversationItem(item);

    if (!normalizedItem && item != null) {
      console.warn("Invalid conversation item filtered from API response:", item);
    }

    return normalizedItem ? [normalizedItem] : [];
  });
}

// Custom hook for copy to clipboard functionality
function useCopyToClipboard(text: string) {
  const [isCopied, setIsCopied] = useState(false);

  const handleCopy = useCallback(async () => {
    try {
      await navigator.clipboard.writeText(text);
      setIsCopied(true);
      setTimeout(() => setIsCopied(false), 2000);
    } catch (error) {
      console.error("Failed to copy text:", error);
    }
  }, [text]);

  return { isCopied, handleCopy };
}

// Copy button component with cleaner design
function CopyButton({ text }: { text: string }) {
  const { isCopied, handleCopy } = useCopyToClipboard(text);

  return (
    <Button
      variant="ghost"
      size="sm"
      className="h-7 w-7 p-0 text-muted-foreground hover:text-foreground"
      onClick={handleCopy}
      aria-label={isCopied ? "Copied" : "Copy to clipboard"}
    >
      {isCopied ? <Check className="h-3.5 w-3.5" /> : <Copy className="h-3.5 w-3.5" />}
    </Button>
  );
}

// TTS play button component
function TTSButton({
  text,
  messageId,
  onNeedsSetup,
  onManage
}: {
  text: string;
  messageId: string;
  onNeedsSetup: () => void;
  onManage: () => void;
}) {
  const { status, isPlaying, currentPlayingId, speak, stop, isTauriEnv } = useTTS();
  const isThisPlaying = isPlaying && currentPlayingId === messageId;
  const longPressTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  // Cleanup timer on unmount
  useEffect(() => {
    return () => {
      if (longPressTimer.current) {
        clearTimeout(longPressTimer.current);
      }
    };
  }, []);

  // Don't render the button at all if not in Tauri environment
  if (!isTauriEnv) {
    return null;
  }

  const handleClick = async () => {
    if (status === "not_downloaded" || status === "error") {
      onNeedsSetup();
      return;
    }

    if (status === "ready") {
      if (isThisPlaying) {
        stop();
      } else {
        await speak(text, messageId);
      }
    }
  };

  const handlePointerDown = () => {
    longPressTimer.current = setTimeout(() => {
      onManage();
    }, 500);
  };

  const handlePointerUp = () => {
    if (longPressTimer.current) {
      clearTimeout(longPressTimer.current);
      longPressTimer.current = null;
    }
  };

  const isDisabled =
    status === "checking" ||
    status === "downloading" ||
    status === "loading" ||
    status === "deleting";
  const showSpinner = isDisabled;

  return (
    <Button
      variant="ghost"
      size="sm"
      className="h-7 w-7 p-0 text-muted-foreground hover:text-foreground"
      onClick={handleClick}
      onPointerDown={handlePointerDown}
      onPointerUp={handlePointerUp}
      onPointerLeave={handlePointerUp}
      disabled={isDisabled}
      aria-label={isThisPlaying ? "Stop speaking" : "Read aloud"}
    >
      {showSpinner ? (
        <Loader2 className="h-3.5 w-3.5 animate-spin" />
      ) : isThisPlaying ? (
        <Square className="h-3.5 w-3.5" />
      ) : (
        <Volume2 className="h-3.5 w-3.5" />
      )}
    </Button>
  );
}

interface Conversation {
  id: string;
  object: "conversation";
  created_at: number;
  metadata?: {
    title?: string;
    [key: string]: unknown;
  };
}

function getToolCallQuery(functionCall: ToolCallItem): string {
  try {
    const args = JSON.parse(functionCall.arguments);
    return args.query || "";
  } catch {
    return "";
  }
}

// Component to render tool calls
function ToolCallRenderer({
  tool,
  toolOutputs,
  relatedCall
}: {
  tool: ConversationContent;
  toolOutputs?: ToolOutputItem[];
  relatedCall?: ToolCallItem;
}) {
  const [isExpanded, setIsExpanded] = useState(false);

  if (tool.type === "web_search_call") {
    const webSearch = tool as ResponseFunctionWebSearch;
    const statusText =
      webSearch.status === "in_progress"
        ? "Searching the web..."
        : webSearch.status === "searching"
          ? "Searching..."
          : webSearch.status === "completed"
            ? "Searched the web"
            : "Search failed";

    const isActive = webSearch.status === "in_progress" || webSearch.status === "searching";

    return (
      <div className="mb-2 flex items-center gap-2 rounded-2xl bg-muted/30 px-3 py-2 text-sm text-muted-foreground">
        {isActive ? (
          <Loader2 className="h-3.5 w-3.5 animate-spin" />
        ) : (
          <Search className="h-3.5 w-3.5" />
        )}
        <span>{statusText}</span>
      </div>
    );
  }

  if (tool.type === "tool_call") {
    const functionCall = tool as ToolCallItem;
    const isWebSearch = functionCall.name === "web_search";
    const query = getToolCallQuery(functionCall);
    const availableToolOutputs = toolOutputs ?? [];
    const combinedOutput = availableToolOutputs
      .map((toolOutput) => toolOutput.output || "")
      .filter(Boolean)
      .join("\n\n");
    const hasToolOutput = combinedOutput.length > 0;
    const isCompleted = availableToolOutputs.some(toolOutputHasResult);
    const isError = functionCall.status === "error";
    const isIncomplete = functionCall.status === "incomplete";
    const isFailed = isError || isIncomplete;
    const isActive = !isCompleted && !isFailed;

    if (hasToolOutput) {
      const preview = truncateMarkdownPreservingLinks(combinedOutput, 150);
      const hasMore = combinedOutput.length > 150;

      return (
        <div className="mb-2 rounded-3xl border border-muted/40 bg-muted/20 px-4 py-3 text-sm">
          <div className="mb-2 flex items-center gap-2">
            {isWebSearch ? (
              <Search className="h-4 w-4 flex-shrink-0 text-[hsl(var(--maple-primary))]" />
            ) : (
              <Check className="h-4 w-4 flex-shrink-0 text-maple-success" />
            )}
            <span className="font-medium text-foreground">
              {isWebSearch
                ? query
                  ? `Searched: "${query}"`
                  : "Web Search"
                : query
                  ? `Ran "${functionCall.name}" for "${query}"`
                  : `Tool "${functionCall.name}" completed`}
            </span>
          </div>
          <div className="pl-6 text-foreground/80">
            <Markdown content={isExpanded ? combinedOutput : preview} />
            {hasMore && (
              <button
                onClick={() => setIsExpanded(!isExpanded)}
                className="ml-2 text-xs text-primary hover:text-primary/80 font-medium"
              >
                {isExpanded ? "Show less" : "Show more"}
              </button>
            )}
          </div>
        </div>
      );
    }

    if (isWebSearch) {
      return (
        <div className="mb-2 flex items-center gap-2 rounded-2xl bg-muted/30 px-3 py-2 text-sm">
          {isActive ? (
            <Loader2 className="h-4 w-4 animate-spin flex-shrink-0 text-[hsl(var(--maple-primary))]" />
          ) : isFailed ? (
            <X
              className={`h-4 w-4 flex-shrink-0 ${
                isError ? "text-maple-error" : "text-maple-warning"
              }`}
            />
          ) : (
            <Search className="h-4 w-4 flex-shrink-0 text-[hsl(var(--maple-primary))]" />
          )}
          <span className="text-foreground">
            {isFailed
              ? isError
                ? query
                  ? `Search for "${query}" failed`
                  : "Web search failed"
                : query
                  ? `Search for "${query}" incomplete`
                  : "Web search incomplete"
              : isCompleted
                ? query
                  ? `Searched: "${query}"`
                  : "Web search completed"
                : query
                  ? `Searching for "${query}"...`
                  : "Searching the web..."}
          </span>
        </div>
      );
    }

    return (
      <div
        className={`mb-2 flex items-center gap-2 rounded-2xl bg-muted/30 px-3 py-2 text-sm ${
          isActive ? "text-muted-foreground" : "text-foreground"
        }`}
      >
        {isActive ? (
          <Loader2 className="h-3.5 w-3.5 animate-spin" />
        ) : isFailed ? (
          <X className={`h-3.5 w-3.5 ${isError ? "text-maple-error" : "text-maple-warning"}`} />
        ) : (
          <Check className="h-3.5 w-3.5 text-maple-success" />
        )}
        <span>
          {isCompleted
            ? query
              ? `Ran "${functionCall.name}" for "${query}"`
              : `Ran "${functionCall.name}"`
            : isFailed
              ? isError
                ? `Tool "${functionCall.name}" failed`
                : `Tool "${functionCall.name}" incomplete`
              : query
                ? `Running "${functionCall.name}" for "${query}"...`
                : `Running "${functionCall.name}"...`}
        </span>
      </div>
    );
  }

  if (tool.type === "tool_output") {
    const toolOutput = tool as ToolOutputItem;
    const output = toolOutput.output || "";
    const query = relatedCall ? getToolCallQuery(relatedCall) : "";
    const isWebSearch = relatedCall?.name === "web_search";

    // Show preview (first 150 chars to match grouped rendering)
    const preview = truncateMarkdownPreservingLinks(output, 150);
    const hasMore = output.length > 150;

    return (
      <div className="mb-2 rounded-3xl border border-muted/40 bg-muted/20 px-4 py-3 text-sm">
        <div className="mb-2 flex items-center gap-2">
          {isWebSearch ? (
            <Search className="h-4 w-4 flex-shrink-0 text-[hsl(var(--maple-primary))]" />
          ) : (
            <Check className="h-4 w-4 flex-shrink-0 text-maple-success" />
          )}
          <span className="font-medium text-foreground">
            {isWebSearch
              ? query
                ? `Searched: "${query}"`
                : "Web Search"
              : relatedCall
                ? `Tool "${relatedCall.name}" result`
                : "Tool Result"}
          </span>
        </div>
        <div className="pl-6 text-foreground/80">
          <Markdown content={isExpanded ? output : preview} />
          {hasMore && (
            <button
              onClick={() => setIsExpanded(!isExpanded)}
              className="ml-2 text-xs text-primary hover:text-primary/80 font-medium"
            >
              {isExpanded ? "Show less" : "Show more"}
            </button>
          )}
        </div>
      </div>
    );
  }

  return null;
}

// Types for grouping messages into turns
type MessageGroup =
  | { type: "user"; message: ExtendedMessage; id: string }
  | { type: "assistant"; items: Message[]; id: string };

// Memoized message list component to prevent re-renders on input changes
const MessageList = memo(
  ({
    messages,
    isGenerating,
    chatId,
    firstMessageRef,
    isLoadingOlderMessages,
    onTTSSetupOpen,
    onTTSManage
  }: {
    messages: Message[];
    isGenerating: boolean;
    chatId?: string;
    firstMessageRef?: React.RefObject<HTMLDivElement>;
    isLoadingOlderMessages?: boolean;
    onTTSSetupOpen: () => void;
    onTTSManage: () => void;
  }) => {
    const toolCallsByCallId = useMemo(() => {
      const toolCalls = new Map<string, ToolCallItem>();

      messages.forEach((message) => {
        if (isToolCallItem(message)) {
          toolCalls.set(message.call_id, message);
        }
      });

      return toolCalls;
    }, [messages]);

    const completedToolCallIds = useMemo(() => {
      return new Set(
        messages
          .filter((message): message is ToolOutputItem => isToolOutputItem(message))
          .filter(toolOutputHasResult)
          .map((message) => message.call_id)
      );
    }, [messages]);

    // Group messages into user turns and assistant turns
    // Assistant turns include: reasoning, tool calls, tool outputs, web search, and assistant messages
    const groupedMessages = useMemo(() => {
      const groups: MessageGroup[] = [];
      let currentAssistantItems: Message[] = [];

      for (const item of messages) {
        // Check if this is a user message
        if (item.type === "message" && (item as unknown as ExtendedMessage).role === "user") {
          // Flush any pending assistant items first
          if (currentAssistantItems.length > 0) {
            groups.push({
              type: "assistant",
              items: [...currentAssistantItems],
              id: `assistant-${currentAssistantItems[0].id}`
            });
            currentAssistantItems = [];
          }
          groups.push({
            type: "user",
            message: item as unknown as ExtendedMessage,
            id: item.id
          });
        } else {
          // This is an assistant-related item (reasoning, tool calls, assistant message, etc.)
          currentAssistantItems.push(item);
        }
      }

      // Don't forget trailing assistant items
      if (currentAssistantItems.length > 0) {
        groups.push({
          type: "assistant",
          items: [...currentAssistantItems],
          id: `assistant-${currentAssistantItems[0].id}`
        });
      }

      return groups;
    }, [messages]);

    const renderAssistantItems = (items: Message[]) => {
      const renderedItems: JSX.Element[] = [];

      for (let index = 0; index < items.length; index++) {
        const item = items[index];
        const itemType = item.type;

        if (itemType === "tool_call") {
          const toolCall = item as ToolCallItem;
          const renderedToolCall =
            completedToolCallIds.has(toolCall.call_id) && toolCall.status !== "completed"
              ? ({ ...toolCall, status: "completed" } as ToolCallItem)
              : toolCall;

          const matchedOutputs: ToolOutputItem[] = [];
          let nextIndex = index + 1;

          while (nextIndex < items.length) {
            const nextItem = items[nextIndex];
            if (isToolOutputItem(nextItem) && nextItem.call_id === toolCall.call_id) {
              matchedOutputs.push(nextItem);
              nextIndex++;
            } else {
              break;
            }
          }

          renderedItems.push(
            <div key={item.id} className="mb-2">
              <ToolCallRenderer tool={renderedToolCall} toolOutputs={matchedOutputs} />
            </div>
          );

          if (matchedOutputs.length > 0) {
            index = nextIndex - 1;
          }

          continue;
        }

        if (itemType === "tool_output") {
          const output = item as ToolOutputItem;
          renderedItems.push(
            <div key={item.id} className="mb-2">
              <ToolCallRenderer tool={output} relatedCall={toolCallsByCallId.get(output.call_id)} />
            </div>
          );
          continue;
        }

        if (itemType === "web_search_call") {
          const webSearch = item as unknown as ResponseFunctionWebSearch;
          renderedItems.push(
            <div key={item.id} className="mb-2">
              <ToolCallRenderer tool={webSearch} />
            </div>
          );
          continue;
        }

        if (itemType === "reasoning") {
          const reasoning = item as ReasoningItem;
          const text = (reasoning.content ?? [])
            .filter((c) => c.type === "reasoning_text")
            .map((c) => c.text)
            .join("");
          const isThinking = reasoning.status === "in_progress" || reasoning.status === "streaming";

          renderedItems.push(
            <div key={item.id} className="mb-2">
              <ThinkingBlock content={text} isThinking={isThinking} />
            </div>
          );
          continue;
        }

        if (itemType === "message") {
          const message = item as unknown as ExtendedMessage;
          if (message.role !== "assistant") continue;

          const isAssistantLoading = message.status === "in_progress";
          if ((!message.content || message.content.length === 0) && !isAssistantLoading) {
            continue;
          }

          renderedItems.push(
            <div key={item.id}>
              <div className="prose prose-sm dark:prose-invert max-w-none">
                <div className="space-y-3">
                  {message.content?.map((part, partIdx) => {
                    if (
                      (part.type === "input_text" ||
                        part.type === "output_text" ||
                        part.type === "text") &&
                      "text" in part &&
                      part.text
                    ) {
                      return (
                        <div key={partIdx}>
                          <Markdown
                            content={part.text}
                            loading={(message as { status?: string }).status === "streaming"}
                            chatId={chatId || ""}
                          />
                        </div>
                      );
                    }
                    if (part.type === "input_image" && "image_url" in part && part.image_url) {
                      return (
                        <div key={partIdx}>
                          <img
                            src={part.image_url}
                            alt={`Image ${partIdx + 1}`}
                            className="max-w-full rounded-2xl"
                            style={{ maxHeight: "400px", objectFit: "contain" }}
                          />
                        </div>
                      );
                    }
                    return null;
                  })}
                </div>
              </div>

              {message.status === "in_progress" && (
                <div className="mt-2 flex items-center gap-1 text-muted-foreground">
                  <div className="h-2 w-2 animate-pulse rounded-full bg-foreground/60" />
                  <div className="h-2 w-2 animate-pulse rounded-full bg-foreground/60 delay-75" />
                  <div className="h-2 w-2 animate-pulse rounded-full bg-foreground/60 delay-150" />
                </div>
              )}
              {message.status === "incomplete" && (
                <div className="mt-2 inline-flex items-center gap-2 rounded-2xl bg-muted/50 px-3 py-1.5 text-sm text-muted-foreground">
                  <div className="h-1.5 w-1.5 rounded-full bg-maple-warning" />
                  <span>Chat Canceled</span>
                </div>
              )}
            </div>
          );
        }
      }

      return renderedItems;
    };

    // Get all text content from an assistant group for the copy button
    const getAssistantGroupText = (items: Message[]) => {
      return items
        .filter((item) => item.type === "message")
        .flatMap((item) => {
          const message = item as unknown as ExtendedMessage;
          return (
            message.content
              ?.filter((p) => "text" in p && p.text)
              .map((p) => ("text" in p ? p.text : "")) || []
          );
        })
        .join("");
    };

    const shouldShowInitialAssistantLoader =
      isGenerating && groupedMessages[groupedMessages.length - 1]?.type !== "assistant";

    return (
      <>
        {/* Loading indicator for older messages */}
        {isLoadingOlderMessages && (
          <div className="flex items-center justify-center py-4">
            <div className="flex items-center gap-2 text-sm text-muted-foreground">
              <div className="w-2 h-2 bg-foreground/60 rounded-full animate-pulse" />
              <div className="w-2 h-2 bg-foreground/60 rounded-full animate-pulse delay-75" />
              <div className="w-2 h-2 bg-foreground/60 rounded-full animate-pulse delay-150" />
            </div>
          </div>
        )}

        {groupedMessages.map((group, groupIndex) => {
          if (group.type === "user") {
            const message = group.message;
            if (!message.content || message.content.length === 0) return null;

            return (
              <div
                key={group.id}
                ref={groupIndex === 0 ? firstMessageRef : undefined}
                className="group flex justify-end py-4 landscape-short:py-1.5"
              >
                <div className="max-w-[min(100%,42rem)] rounded-2xl border border-border bg-muted px-4 py-3 landscape-short:px-3 landscape-short:py-2 backdrop-blur-lg dark:bg-card">
                  <div className="prose prose-sm max-w-none text-left dark:prose-invert">
                    <div className="space-y-3">
                      {message.content.map((part, partIdx) => {
                        if (
                          (part.type === "input_text" ||
                            part.type === "output_text" ||
                            part.type === "text") &&
                          "text" in part &&
                          part.text
                        ) {
                          return (
                            <div key={partIdx}>
                              <Markdown content={part.text} chatId={chatId || ""} />
                            </div>
                          );
                        }
                        if (part.type === "input_image" && "image_url" in part && part.image_url) {
                          return (
                            <div key={partIdx}>
                              <img
                                src={part.image_url}
                                alt={`Image ${partIdx + 1}`}
                                className="max-w-full rounded-2xl"
                                style={{ maxHeight: "400px", objectFit: "contain" }}
                              />
                            </div>
                          );
                        }
                        return null;
                      })}
                    </div>
                  </div>
                </div>
              </div>
            );
          }

          // Assistant group - render all items in one Maple box
          if (group.type === "assistant") {
            const hasContent = group.items.some((item) => {
              if (item.type === "message") {
                const msg = item as unknown as ExtendedMessage;
                return (
                  msg.role === "assistant" &&
                  (msg.content?.length > 0 || msg.status === "in_progress")
                );
              }
              return true; // reasoning, tool calls always count
            });

            if (!hasContent) return null;

            const textContent = getAssistantGroupText(group.items);

            return (
              <div
                key={group.id}
                ref={groupIndex === 0 ? firstMessageRef : undefined}
                className="group py-4 px-0 md:p-4 landscape-short:py-1.5 landscape-short:px-2"
              >
                <div className="mx-auto flex w-full max-w-4xl flex-col gap-2 md:flex-row md:items-start md:gap-3">
                  <div className="flex h-8 shrink-0 items-center gap-2 px-0 md:h-auto md:flex-col md:items-start md:gap-3 landscape-short:h-6">
                    <MapleChatAvatar />
                    <div className="text-sm font-semibold leading-none md:hidden">Maple</div>
                  </div>
                  <div className="flex min-w-0 w-full flex-1 flex-col overflow-hidden px-2 md:gap-2 md:px-0">
                    <div className="hidden md:block">
                      <div className="text-left text-sm font-semibold leading-none">Maple</div>
                    </div>
                    <div className="space-y-2">
                      {renderAssistantItems(group.items)}
                      {/* Copy and TTS buttons for the assistant's text content */}
                      {textContent && (
                        <div className="flex gap-1">
                          <CopyButton text={textContent} />
                          <TTSButton
                            text={textContent}
                            messageId={group.id}
                            onNeedsSetup={onTTSSetupOpen}
                            onManage={onTTSManage}
                          />
                        </div>
                      )}
                    </div>
                  </div>
                </div>
              </div>
            );
          }

          return null;
        })}

        {/* Loading indicator - only show while waiting for the first assistant item (TTFT) */}
        {shouldShowInitialAssistantLoader && (
          <div className="group py-4 px-0 md:p-4 landscape-short:py-1.5 landscape-short:px-2">
            <div className="mx-auto flex w-full max-w-4xl flex-col gap-2 md:flex-row md:items-start md:gap-3">
              <div className="flex h-8 shrink-0 items-center gap-2 px-0 md:h-auto md:flex-col md:items-start md:gap-3 landscape-short:h-6">
                <MapleChatAvatar />
                <div className="text-sm font-semibold leading-none md:hidden">Maple</div>
              </div>
              <div className="flex min-w-0 w-full flex-1 flex-col px-2 md:gap-2 md:px-0">
                <div className="hidden md:block">
                  <div className="text-left text-sm font-semibold leading-none">Maple</div>
                </div>
                <div className="flex items-center gap-1">
                  <div className="h-2 w-2 animate-pulse rounded-full bg-foreground/60" />
                  <div className="h-2 w-2 animate-pulse rounded-full bg-foreground/60 delay-75" />
                  <div className="h-2 w-2 animate-pulse rounded-full bg-foreground/60 delay-150" />
                </div>
              </div>
            </div>
          </div>
        )}
      </>
    );
  }
);

MessageList.displayName = "MessageList";

export function UnifiedChat() {
  const isMobile = useIsMobile();
  const isLandscapeMobile = useIsLandscapeMobile();
  const isCompactLayout = isMobile || isLandscapeMobile;
  const openai = useOpenAI();
  const localState = useLocalState();
  const { selectedProjectId, setSelectedProjectId } = localState;
  const os = useOpenSecret();
  const isTauriEnv = isTauri();
  const queryClient = useQueryClient();
  const { playbackError, clearPlaybackError } = useTTS();

  // Track chatId from URL - use state so we can update it
  const [chatId, setChatId] = useState<string | undefined>(() => {
    const params = new URLSearchParams(window.location.search);
    return params.get("conversation_id") || undefined;
  });

  // State
  const [conversation, setConversation] = useState<Conversation | null>(null);
  const [messages, setMessages] = useState<Message[]>([]);
  const [input, setInput] = useState("");
  const [draftProjectId, setDraftProjectId] = useState<string | null>(() => selectedProjectId);
  const [isGenerating, setIsGenerating] = useState(false);
  const [isSidebarOpen, setIsSidebarOpen] = useState(!isCompactLayout);
  const [error, setError] = useState<string | null>(null);
  const [lastSeenItemId, setLastSeenItemId] = useState<string | undefined>();
  const [isNewConversationJustCreated, setIsNewConversationJustCreated] = useState(false);
  const [currentResponseId, setCurrentResponseId] = useState<string | undefined>();
  const [titleJustUpdated, setTitleJustUpdated] = useState(false);

  // Pagination states
  const [oldestItemId, setOldestItemId] = useState<string | undefined>();
  const [isLoadingOlderMessages, setIsLoadingOlderMessages] = useState(false);
  const [hasMoreOlderMessages, setHasMoreOlderMessages] = useState(false);

  // Attachment states
  const [draftImages, setDraftImages] = useState<File[]>([]);
  const [imageUrls, setImageUrls] = useState<Map<File, string>>(new Map());
  const [documentText, setDocumentText] = useState<string>("");
  const [documentName, setDocumentName] = useState<string>("");
  const [isProcessingDocument, setIsProcessingDocument] = useState(false);
  const [attachmentError, setAttachmentError] = useState<string | null>(null);
  const [upgradeDialogOpen, setUpgradeDialogOpen] = useState(false);
  const [upgradeFeature, setUpgradeFeature] = useState<
    "image" | "document" | "voice" | "usage" | "tokens"
  >("image");
  const [documentPlatformDialogOpen, setDocumentPlatformDialogOpen] = useState(false);
  const [contextLimitDialogOpen, setContextLimitDialogOpen] = useState(false);
  const [ttsSetupDialogOpen, setTtsSetupDialogOpen] = useState(false);

  // Audio recording states
  const [isRecording, setIsRecording] = useState(false);
  const [isTranscribing, setIsTranscribing] = useState(false);
  const [isProcessingSend, setIsProcessingSend] = useState(false);
  const [audioError, setAudioError] = useState<string | null>(null);

  // Web search toggle state - persisted in localStorage, billing-aware initial default
  const [isWebSearchEnabled, setIsWebSearchEnabled] = useState(getInitialWebSearchEnabled);

  // Fullscreen mode for power users - persisted in localStorage
  const [isFullscreen, setIsFullscreen] = useState(() => {
    return localStorage.getItem("chatFullscreen") === "true";
  });
  const [isFullscreenAnimating, setIsFullscreenAnimating] = useState(false);

  // Close sidebar when rotating to landscape on a short screen
  useEffect(() => {
    if (isLandscapeMobile && isSidebarOpen) {
      setIsSidebarOpen(false);
    }
  }, [isLandscapeMobile]); // eslint-disable-line react-hooks/exhaustive-deps

  // Save fullscreen preference to localStorage when it changes
  useEffect(() => {
    localStorage.setItem("chatFullscreen", isFullscreen.toString());
  }, [isFullscreen]);

  // Toggle fullscreen with animation
  const toggleFullscreen = useCallback(() => {
    setIsFullscreenAnimating(true);
    setIsFullscreen((prev) => !prev);
    // Reset animation state after transition completes
    setTimeout(() => setIsFullscreenAnimating(false), 300);
  }, []);

  // Scroll state
  const [isUserScrolling, setIsUserScrolling] = useState(false);
  const isUserScrollingRef = useRef(false);
  const prevMessageCountRef = useRef(0);
  const prevStreamingRef = useRef(false);
  const [hasNewPolledMessages, setHasNewPolledMessages] = useState(false);
  const recorderRef = useRef<RecordRTC | null>(null);
  const streamRef = useRef<MediaStream | null>(null);
  const assistantStreamingRef = useRef(false);

  const abortControllerRef = useRef<AbortController | null>(null);
  const billingRefreshTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const documentInputRef = useRef<HTMLInputElement>(null);

  // Refs
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const chatContainerRef = useRef<HTMLDivElement>(null);
  const firstMessageRef = useRef<HTMLDivElement>(null);
  const activeConversationLoadRef = useRef(0);

  // Attachment cleanup function - defined early to avoid reference errors
  const clearAllAttachments = useCallback(() => {
    // Clean up image URLs
    imageUrls.forEach((url) => URL.revokeObjectURL(url));
    setImageUrls(new Map());
    setDraftImages([]);
    setDocumentText("");
    setDocumentName("");
    setAttachmentError(null);
  }, [imageUrls]);

  // Auto-resize textarea
  useEffect(() => {
    if (textareaRef.current) {
      textareaRef.current.style.height = "auto";
      const scrollHeight = textareaRef.current.scrollHeight;
      textareaRef.current.style.height = `${Math.min(scrollHeight, 200)}px`;
    }
  }, [input]);

  // Cleanup billing refresh timeout on unmount
  useEffect(() => {
    return () => {
      if (billingRefreshTimeoutRef.current) {
        clearTimeout(billingRefreshTimeoutRef.current);
      }
    };
  }, []);

  // Auto-focus textbox on desktop (not mobile/landscape-mobile to avoid keyboard popup interrupting reading)
  // Focus when: app launches, new chat, conversation loads, or assistant finishes streaming
  useEffect(() => {
    // Skip on compact layouts (mobile + landscape mobile) to avoid keyboard popup
    if (isCompactLayout) return;

    // Focus when not generating and textbox is not disabled
    if (!isGenerating && textareaRef.current && !textareaRef.current.disabled) {
      // Small delay to ensure DOM is ready
      setTimeout(() => {
        textareaRef.current?.focus();
      }, 100);
    }
  }, [isCompactLayout, isGenerating, messages.length, chatId]);

  // Improved scroll detection - track if user is near bottom
  const handleScroll = useCallback(() => {
    const container = chatContainerRef.current;
    if (!container) return;

    const { scrollTop, scrollHeight, clientHeight } = container;
    const isNearBottom = scrollHeight - scrollTop - clientHeight < 100;
    setIsUserScrolling(!isNearBottom);
  }, []);

  useEffect(() => {
    isUserScrollingRef.current = isUserScrolling;
  }, [isUserScrolling]);

  // Scroll to bottom helper
  const scrollToBottom = useCallback((behavior: ScrollBehavior = "smooth") => {
    if (chatContainerRef.current) {
      chatContainerRef.current.scrollTo({
        top: chatContainerRef.current.scrollHeight,
        behavior
      });
    }
  }, []);

  const getScrollDebugSnapshot = useCallback(() => {
    const container = chatContainerRef.current;

    if (!container) {
      return null;
    }

    const { scrollTop, scrollHeight, clientHeight } = container;

    return {
      scrollTop: Math.round(scrollTop),
      scrollHeight: Math.round(scrollHeight),
      clientHeight: Math.round(clientHeight),
      distanceFromBottom: Math.max(0, Math.round(scrollHeight - scrollTop - clientHeight))
    };
  }, []);

  const logStreamEvent = useCallback(
    (streamStartedAt: number, eventIndex: number, eventType: string, event: unknown) => {
      const includeScrollSnapshot =
        eventType === "response.output_item.added" ||
        eventType === "response.output_item.done" ||
        eventType === "tool_call.created" ||
        eventType === "tool_output.created";

      console.log("🔵 SSE Event", {
        at: new Date().toISOString(),
        elapsedMs: Math.round(performance.now() - streamStartedAt),
        eventIndex,
        eventType,
        ...summarizeStreamEventForLog(eventType, event),
        ...(includeScrollSnapshot
          ? {
              isUserScrolling: isUserScrollingRef.current,
              scroll: getScrollDebugSnapshot()
            }
          : {})
      });
    },
    [getScrollDebugSnapshot]
  );

  // Attach scroll listener
  useEffect(() => {
    const container = chatContainerRef.current;
    if (!container) return;

    container.addEventListener("scroll", handleScroll);
    // Initial check
    handleScroll();

    return () => container.removeEventListener("scroll", handleScroll);
  }, [handleScroll]);

  // Initial load - scroll to bottom instantly
  useEffect(() => {
    if (messages.length > 0 && prevMessageCountRef.current === 0 && !isLoadingOlderMessages) {
      // First load of messages - scroll instantly to bottom
      setTimeout(() => {
        scrollToBottom("instant");
      }, 0);
    }
    // Don't update count when loading older messages, to avoid triggering scroll
    if (!isLoadingOlderMessages) {
      prevMessageCountRef.current = messages.length;
    }
  }, [messages.length, scrollToBottom, isLoadingOlderMessages]);

  // Auto-scroll when user sends a message
  // Track the LAST message ID (at the end of the array), not the count
  const lastMessageId = messages.length > 0 ? messages[messages.length - 1].id : null;
  const prevLastMessageId = useRef(lastMessageId);

  useEffect(() => {
    // Only scroll if the LAST message changed, which means an item was added to the END
    // (not prepended while loading older history).
    if (lastMessageId !== prevLastMessageId.current && messages.length > 0) {
      const lastItem = messages[messages.length - 1];
      const isUserMessage =
        lastItem.type === "message" && (lastItem as ExtendedMessage).role === "user";
      const shouldAutoScrollAssistantItem =
        isAssistantConversationItem(lastItem) && !isUserScrolling;

      if (isUserMessage || shouldAutoScrollAssistantItem) {
        setTimeout(
          () => {
            scrollToBottom("smooth");
          },
          isUserMessage ? 50 : 0
        );
      }
    }
    prevLastMessageId.current = lastMessageId;
  }, [isUserScrolling, lastMessageId, messages, scrollToBottom]);

  // Auto-scroll when assistant starts streaming (but not while streaming)
  useEffect(() => {
    const hasStreamingMessage = messages.some(
      (m) => m.type === "message" && (m as { status?: string }).status === "streaming"
    );

    if (hasStreamingMessage && !prevStreamingRef.current && !isUserScrolling) {
      // Just started streaming - scroll slightly to show the loading indicator
      setTimeout(() => {
        const container = chatContainerRef.current;
        if (container) {
          // Scroll just enough to show the streaming message started
          const currentScroll = container.scrollTop;
          const maxScroll = container.scrollHeight - container.clientHeight;
          // Scroll down 100px or to bottom, whichever is less
          const targetScroll = Math.min(currentScroll + 100, maxScroll);
          container.scrollTo({
            top: targetScroll,
            behavior: "smooth"
          });
        }
      }, 100);
    }

    prevStreamingRef.current = hasStreamingMessage;
  }, [messages, isUserScrolling]);

  // Auto-scroll when new messages arrive from polling
  useEffect(() => {
    if (hasNewPolledMessages) {
      // New messages arrived from polling - scroll to bottom to show them
      setTimeout(() => {
        scrollToBottom("smooth");
      }, 100);

      // Reset the flag
      setHasNewPolledMessages(false);
    }
  }, [hasNewPolledMessages, scrollToBottom]);

  // Unified event handling for conversation changes
  useEffect(() => {
    // Handle new chat event
    const handleNewChat = (event?: Event) => {
      const nextProjectId =
        event instanceof CustomEvent && event.detail && "projectId" in event.detail
          ? (event.detail.projectId ?? null)
          : (selectedProjectId ?? null);

      activeConversationLoadRef.current += 1;
      setChatId(undefined);
      setConversation(null);
      setMessages([]);
      setInput("");
      setDraftProjectId(nextProjectId);
      setError(null);
      setLastSeenItemId(undefined);
      // Clear pagination state
      setOldestItemId(undefined);
      setHasMoreOlderMessages(false);
      setIsLoadingOlderMessages(false);
      // Clear attachments
      clearAllAttachments();
      // Reset scroll tracking
      prevMessageCountRef.current = 0;
    };

    // Handle conversation selection from sidebar
    const handleConversationSelected = (event: CustomEvent) => {
      const { conversationId } = event.detail;
      if (conversationId && conversationId !== chatId) {
        // Reset scroll tracking for new conversation
        prevMessageCountRef.current = 0;
        activeConversationLoadRef.current += 1;
        // Update our local chatId state to trigger load
        setChatId(conversationId);
        setError(null);
      }
    };

    // Handle browser back/forward navigation
    const handlePopState = () => {
      const params = new URLSearchParams(window.location.search);
      const newChatId = params.get("conversation_id") || undefined;
      if (newChatId !== chatId) {
        // Reset scroll tracking for navigation
        prevMessageCountRef.current = 0;
        activeConversationLoadRef.current += 1;
        setChatId(newChatId);
      }
    };

    window.addEventListener("newchat", handleNewChat);
    window.addEventListener("conversationselected", handleConversationSelected as EventListener);
    window.addEventListener("popstate", handlePopState);

    return () => {
      window.removeEventListener("newchat", handleNewChat);
      window.removeEventListener(
        "conversationselected",
        handleConversationSelected as EventListener
      );
      window.removeEventListener("popstate", handlePopState);
    };
  }, [chatId, clearAllAttachments, selectedProjectId]);

  // Cancel the current response
  const handleCancelResponse = useCallback(async () => {
    if (!currentResponseId || !openai) return;

    try {
      await (openai.responses as { cancel: (id: string) => Promise<unknown> }).cancel(
        currentResponseId
      );
      setIsGenerating(false);
      setCurrentResponseId(undefined);
      abortControllerRef.current?.abort();
      abortControllerRef.current = null;
      assistantStreamingRef.current = false;
    } catch (error) {
      console.error("Failed to cancel response:", error);
      setError("Failed to cancel response. Please try again.");
    }
  }, [currentResponseId, openai]);

  // Load conversation from API
  const loadConversation = useCallback(
    async (conversationId: string) => {
      if (!openai) return;

      const requestId = ++activeConversationLoadRef.current;
      const isStaleRequest = () => requestId !== activeConversationLoadRef.current;

      // Reset message count before loading new conversation
      prevMessageCountRef.current = 0;

      try {
        // Start both fetches immediately in parallel
        const convPromise = openai.conversations.retrieve(conversationId).then(
          (conv) => ({ ok: true as const, value: conv }),
          (error) => ({ ok: false as const, error })
        );
        const itemsPromise = openai.conversations.items.list(conversationId, {
          limit: 10,
          order: "desc"
        });

        // Process items as soon as they're ready (don't wait for metadata)
        const itemsResponse = await itemsPromise;
        if (isStaleRequest()) return;

        // Convert items to messages, grouping tool calls with their messages
        const loadedMessages = convertItemsToMessages(
          itemsResponse.data as Array<{
            id: string;
            type: string;
            role?: string;
            content?: unknown;
            name?: string;
            arguments?: string;
            call_id?: string;
            output?: string;
            status?: string;
            created_at?: number;
          }>
        );

        // Reverse the array for display (we want oldest first/at top, newest last/at bottom)
        // API returns desc (newest first), but chat UI needs chronological order
        const messagesInChronologicalOrder = loadedMessages.reverse();
        setMessages(messagesInChronologicalOrder);

        // Set pagination state
        // Before reversal, last item was the oldest. After reversal, it's now first item.
        // But for the API "after" parameter, we still need the chronologically oldest ID
        if (loadedMessages.length > 0) {
          // After reversal, the FIRST message is the oldest (chronologically earliest)
          const oldestId = messagesInChronologicalOrder[0].id;
          setOldestItemId(oldestId);
          // If we got a full page, there might be more
          const hasMore = loadedMessages.length === 10;
          setHasMoreOlderMessages(hasMore);
        } else {
          setOldestItemId(undefined);
          setHasMoreOlderMessages(false);
        }

        // Set last seen ID for polling - use the NEWEST message (first in desc response)
        // Skip in_progress messages by finding the first completed one
        const newestCompletedItem = itemsResponse.data.find(
          (item) => (item as Message).status !== "in_progress"
        );
        if (newestCompletedItem) {
          setLastSeenItemId(newestCompletedItem.id);
        }

        // Then handle conversation metadata when it arrives
        const conv = await convPromise;
        if (isStaleRequest()) return;
        if (!conv.ok) {
          throw conv.error;
        }
        setConversation(conv.value as Conversation);
      } catch (error) {
        if (isStaleRequest()) return;

        const err = error as { status?: number; message?: string };
        if (err.status === 404) {
          // Conversation doesn't exist - clear and start fresh
          // Conversation not found, starting new
          setConversation(null);
          setMessages([]);
          setError(null);
          setOldestItemId(undefined);
          setHasMoreOlderMessages(false);
          // Clear the invalid conversation_id from URL
          const params = new URLSearchParams(window.location.search);
          params.delete("conversation_id");
          window.history.replaceState({}, "", params.toString() ? `/?${params}` : "/");
        } else {
          console.error("Failed to load conversation:", error);
          setError(err.message || "Failed to load conversation");
        }
      }
    },
    [openai]
  );

  // Load older messages for pagination
  const loadOlderMessages = useCallback(async () => {
    if (!conversation?.id || !openai || !oldestItemId || isLoadingOlderMessages) return;

    setIsLoadingOlderMessages(true);

    try {
      // Fetch next 10 older items using the oldest item ID we have
      const itemsResponse = await openai.conversations.items.list(conversation.id, {
        limit: 10,
        order: "desc",
        after: oldestItemId
      });

      // Convert items to messages, grouping tool calls with their messages
      const olderMessages = convertItemsToMessages(
        itemsResponse.data as Array<{
          id: string;
          type: string;
          role?: string;
          content?: unknown;
          name?: string;
          arguments?: string;
          call_id?: string;
          output?: string;
          status?: string;
          created_at?: number;
        }>
      );

      if (olderMessages.length > 0) {
        // Reverse for chronological order (API returns desc, we need asc for display)
        const olderMessagesInChronologicalOrder = olderMessages.reverse();

        // Prepend older messages to the existing messages using merge helper
        // This ensures no duplicates if a message was already loaded
        setMessages((prev) => mergeMessagesById(olderMessagesInChronologicalOrder, prev));

        // Update pagination state
        // After reversal, the FIRST message is the chronologically oldest
        const newOldestId = olderMessagesInChronologicalOrder[0].id;
        const hasMore = olderMessages.length === 10;
        setOldestItemId(newOldestId);
        setHasMoreOlderMessages(hasMore);
      } else {
        // No more messages to load
        setHasMoreOlderMessages(false);
      }
    } catch (error) {
      console.error("Failed to load older messages:", error);
    } finally {
      setIsLoadingOlderMessages(false);
    }
  }, [conversation?.id, openai, oldestItemId, isLoadingOlderMessages]);

  // Polling mechanism for conversation updates
  const pollForNewItems = useCallback(async () => {
    if (!conversation?.id || !openai) return;
    if (assistantStreamingRef.current) return;

    try {
      // Fetch NEW items that came after the last seen ID
      // Use order=asc to get items chronologically after the lastSeenItemId
      const response = await openai.conversations.items.list(conversation.id, {
        ...(lastSeenItemId ? { after: lastSeenItemId, order: "asc" } : {}),
        limit: 20 // Smaller limit since we only expect a few new messages
      });

      if (response.data.length > 0) {
        // Convert API items to UI messages, grouping tool calls with their messages
        const newMessages = convertItemsToMessages(
          response.data as Array<{
            id: string;
            type: string;
            role?: string;
            content?: unknown;
            name?: string;
            arguments?: string;
            call_id?: string;
            output?: string;
            status?: string;
            created_at?: number;
          }>
        );

        if (newMessages.length > 0) {
          // Merge new messages with deduplication using helper
          setMessages((prev) => {
            // Check if there are truly new messages (not already in prev)
            const prevIds = new Set(prev.map((m) => m.id));
            const trulyNewMessages = newMessages.filter((m) => !prevIds.has(m.id));

            // Mark that we have new polled messages for scrolling
            if (trulyNewMessages.length > 0) {
              setHasNewPolledMessages(true);
            }

            // Use merge helper to combine, which will deduplicate and update existing messages
            return mergeMessagesById(prev, newMessages);
          });

          // Update last seen item ID for next poll
          // Since we're using order=asc, the LAST item is the newest
          // Skip in_progress messages by finding the last completed one
          const newestCompletedItem = [...response.data]
            .reverse()
            .find((item) => (item as Message).status !== "in_progress");
          if (newestCompletedItem) {
            setLastSeenItemId(newestCompletedItem.id);
          }

          // Check if we're no longer generating
          // Only stop if assistant message is completed or incomplete, not if still in_progress
          if (
            isGenerating &&
            newMessages.some(
              (m) =>
                m.type === "message" &&
                (m as ExtendedMessage).role === "assistant" &&
                ((m as ExtendedMessage).status === "completed" ||
                  (m as ExtendedMessage).status === "incomplete")
            )
          ) {
            setIsGenerating(false);
          }
        }
      }
    } catch (error) {
      console.error("Polling error:", error);
      // Don't throw - polling should fail silently
    }
  }, [conversation?.id, lastSeenItemId, isGenerating, openai]);

  // Load conversation when URL changes or on mount
  useEffect(() => {
    // Skip if we just created a new conversation
    if (isNewConversationJustCreated) {
      // Reset the flag for next time
      setIsNewConversationJustCreated(false);
      return;
    }

    if (chatId && openai) {
      // Load the conversation from URL
      loadConversation(chatId);
    } else if (!chatId) {
      // Clear if no conversation ID
      setConversation(null);
      setMessages([]);
      setDraftProjectId(selectedProjectId ?? null);
      setLastSeenItemId(undefined);
      // Clear pagination state
      setOldestItemId(undefined);
      setHasMoreOlderMessages(false);
      setIsLoadingOlderMessages(false);
      // Reset scroll tracking
      prevMessageCountRef.current = 0;
    }
  }, [chatId, openai, loadConversation, selectedProjectId]);

  // Set up progressive polling interval
  useEffect(() => {
    if (!conversation?.id || !openai) return;

    // Progressive intervals: 2s, 5s, 10s, 15s, 20s, 30s, 60s (then 60s forever)
    const intervals = [2000, 5000, 10000, 15000, 20000, 30000, 60000];
    let currentIntervalIndex = 0;
    let timeoutId: ReturnType<typeof setTimeout>;

    const scheduleNextPoll = () => {
      // Get current interval (use last interval if we've reached the end)
      const currentInterval = intervals[Math.min(currentIntervalIndex, intervals.length - 1)];

      timeoutId = setTimeout(() => {
        pollForNewItems();

        // Move to next interval if not at the end
        if (currentIntervalIndex < intervals.length - 1) {
          currentIntervalIndex++;
        }

        // Schedule the next poll
        scheduleNextPoll();
      }, currentInterval);
    };

    // Start the progressive polling (first poll after 2s)
    scheduleNextPoll();

    return () => {
      if (timeoutId) clearTimeout(timeoutId);
    };
  }, [conversation?.id, openai, pollForNewItems]);

  // Poll for title updates when it's "New Conversation" with exponential backoff
  useEffect(() => {
    const currentTitle = conversation?.metadata?.title;

    // Only poll if we have a conversation and the title is "New Conversation"
    if (!conversation?.id || !openai || currentTitle !== "New Conversation") {
      return;
    }

    // Exponential backoff: 0.5s, 1s, 2s, 4s, 8s, 10s (max)
    let currentDelay = 500; // Start at 0.5s
    const maxDelay = 10000; // Cap at 10s
    let timeoutId: ReturnType<typeof setTimeout>;

    const checkTitle = async () => {
      try {
        // Fetch updated conversation metadata
        const updatedConv = await openai.conversations.retrieve(conversation.id);
        const newTitle = (updatedConv as Conversation).metadata?.title;

        // If title changed from "New Conversation", update local state and sidebar
        if (newTitle && newTitle !== "New Conversation") {
          setConversation(updatedConv as Conversation);
          // Trigger title animation
          setTitleJustUpdated(true);
          // Remove animation class after animation completes (800ms for flash animation)
          setTimeout(() => setTitleJustUpdated(false), 850);
          // Refresh all sidebar conversation lists
          await Promise.all([
            queryClient.invalidateQueries({ queryKey: ["conversations"] }),
            queryClient.invalidateQueries({ queryKey: ["pinnedConversations"] }),
            queryClient.invalidateQueries({ queryKey: ["projectConversations"] })
          ]);
          return; // Stop polling once title is updated
        }

        // Schedule next check with exponential backoff
        currentDelay = Math.min(currentDelay * 2, maxDelay);
        timeoutId = setTimeout(checkTitle, currentDelay);
      } catch (error) {
        console.error("Failed to check title update:", error);
        // Continue polling even on error
        currentDelay = Math.min(currentDelay * 2, maxDelay);
        timeoutId = setTimeout(checkTitle, currentDelay);
      }
    };

    // Start the first check after 0.5s
    timeoutId = setTimeout(checkTitle, currentDelay);

    return () => {
      if (timeoutId) clearTimeout(timeoutId);
    };
  }, [conversation?.id, conversation?.metadata?.title, openai, queryClient]);

  // Set up IntersectionObserver for loading older messages
  useEffect(() => {
    if (!firstMessageRef.current || !hasMoreOlderMessages) return;

    const observer = new IntersectionObserver(
      (entries) => {
        // When the first message comes into view, load older messages
        if (entries[0].isIntersecting && hasMoreOlderMessages && !isLoadingOlderMessages) {
          loadOlderMessages();
        }
      },
      {
        root: chatContainerRef.current,
        rootMargin: "100px", // Start loading a bit before the message is visible
        threshold: 0.1
      }
    );

    observer.observe(firstMessageRef.current);

    return () => {
      observer.disconnect();
    };
  }, [hasMoreOlderMessages, isLoadingOlderMessages, loadOlderMessages]);

  // Auto-clear error after 3 seconds
  useEffect(() => {
    if (error) {
      const timeoutId = setTimeout(() => {
        setError(null);
      }, 3000);

      return () => clearTimeout(timeoutId);
    }
  }, [error]);

  // Toggle sidebar
  const toggleSidebar = useCallback(() => setIsSidebarOpen((prev) => !prev), []);

  const handleNewChatFromHeader = useCallback(() => {
    flushSync(() => {
      setSelectedProjectId(null);
    });

    const usp = new URLSearchParams(window.location.search);
    usp.delete("conversation_id");
    usp.delete("project_id");
    const newUrl = usp.toString()
      ? `${window.location.pathname}?${usp.toString()}`
      : window.location.pathname;
    window.history.replaceState(null, "", newUrl);
    window.dispatchEvent(new CustomEvent("newchat", { detail: { projectId: null } }));
    if (isSidebarOpen) {
      toggleSidebar();
    }
  }, [isSidebarOpen, setSelectedProjectId, toggleSidebar]);

  // Check user's billing access
  const billingStatus = localState.billingStatus;
  const hasProAccess =
    billingStatus &&
    (billingStatus.product_name?.toLowerCase().includes("pro") ||
      billingStatus.product_name?.toLowerCase().includes("max") ||
      billingStatus.product_name?.toLowerCase().includes("team"));

  const hasStarterAccess =
    billingStatus &&
    (billingStatus.product_name?.toLowerCase().includes("starter") || hasProAccess);

  const canUseImages = hasStarterAccess;
  const canUseDocuments = hasProAccess;
  const canUseVoice = hasProAccess && localState.hasWhisperModel;

  const handleAddImages = useCallback(
    (e: React.ChangeEvent<HTMLInputElement>) => {
      if (!e.target.files) return;

      const supportedTypes = ["image/jpeg", "image/jpg", "image/png", "image/webp"];
      const maxSizeInBytes = 10 * 1024 * 1024; // 10MB

      const validFiles = Array.from(e.target.files).filter((file) => {
        if (!supportedTypes.includes(file.type.toLowerCase())) {
          setAttachmentError("Only JPEG, PNG, and WebP images are supported");
          setTimeout(() => setAttachmentError(null), 5000);
          return false;
        }
        if (file.size > maxSizeInBytes) {
          setAttachmentError(`Image too large (max 10MB)`);
          setTimeout(() => setAttachmentError(null), 5000);
          return false;
        }
        return true;
      });

      // Create object URLs for previews
      const newUrlMap = new Map(imageUrls);
      validFiles.forEach((file) => {
        if (!newUrlMap.has(file)) {
          newUrlMap.set(file, URL.createObjectURL(file));
        }
      });
      setImageUrls(newUrlMap);
      setDraftImages((prev) => [...prev, ...validFiles]);

      // Clear input to allow re-uploading same file
      e.target.value = "";
    },
    [imageUrls, localState]
  );

  const removeImage = useCallback(
    (idx: number) => {
      setDraftImages((prev) => {
        const fileToRemove = prev[idx];
        const url = imageUrls.get(fileToRemove);
        if (url) {
          URL.revokeObjectURL(url);
          setImageUrls((prevUrls) => {
            const newUrls = new Map(prevUrls);
            newUrls.delete(fileToRemove);
            return newUrls;
          });
        }
        return prev.filter((_, i) => i !== idx);
      });
    },
    [imageUrls]
  );

  const handleDocumentUpload = useCallback(
    async (e: React.ChangeEvent<HTMLInputElement>) => {
      const file = e.target.files?.[0];
      if (!file) return;

      const maxSizeInBytes = 10 * 1024 * 1024; // 10MB
      if (file.size > maxSizeInBytes) {
        setAttachmentError("Document too large (max 10MB)");
        setTimeout(() => setAttachmentError(null), 5000);
        e.target.value = "";
        return;
      }

      setIsProcessingDocument(true);
      setAttachmentError(null);

      try {
        // For text files, read directly
        if (file.name.endsWith(".txt") || file.name.endsWith(".md")) {
          const text = await file.text();
          // Format as JSON for consistency with PDF handling
          const documentData = {
            document: {
              filename: file.name,
              text_content: text
            }
          };
          setDocumentText(JSON.stringify(documentData));
          setDocumentName(file.name);
        } else if (file.name.endsWith(".pdf") && isTauriEnv) {
          // For PDFs in Tauri, use the parseDocument API
          const reader = new FileReader();
          const base64Data = await new Promise<string>((resolve, reject) => {
            reader.onload = () => {
              const base64 = (reader.result as string).split(",")[1];
              resolve(base64);
            };
            reader.onerror = reject;
            reader.readAsDataURL(file);
          });

          // Use the Tauri API directly for parsing PDFs
          const { invoke } = await import("@tauri-apps/api/core");

          // Define the response type to match Rust
          interface RustDocumentResponse {
            document: {
              filename: string;
              text_content: string;
            };
            status: string;
          }

          const result = await invoke<RustDocumentResponse>("extract_document_content", {
            fileBase64: base64Data,
            filename: file.name,
            fileType: "pdf"
          });

          if (result.document?.text_content) {
            // Create a cleaned version with image references removed
            const cleanedParsed = {
              document: {
                filename: result.document.filename,
                text_content: result.document.text_content.replace(/!\[Image\]\([^)]+\)/g, "")
              }
            };

            // Store as JSON string for markdown.tsx to parse and display properly
            setDocumentText(JSON.stringify(cleanedParsed));
            setDocumentName(file.name);
          }
        } else if (file.name.endsWith(".pdf")) {
          setAttachmentError("PDF files can only be processed in the desktop app");
          setTimeout(() => setAttachmentError(null), 5000);
        }
      } catch (error) {
        console.error("Document processing error:", error);
        setAttachmentError("Failed to process document");
        setTimeout(() => setAttachmentError(null), 5000);
      } finally {
        setIsProcessingDocument(false);
        e.target.value = "";
      }
    },
    [isTauriEnv]
  );

  const removeDocument = useCallback(() => {
    setDocumentText("");
    setDocumentName("");
  }, []);

  // Audio recording functions
  const startRecording = async () => {
    // Prevent duplicate starts
    if (isRecording || isTranscribing) return;

    // Check if user has access
    if (!canUseVoice) {
      setUpgradeFeature("voice");
      setUpgradeDialogOpen(true);
      return;
    }

    try {
      // Check if getUserMedia is available
      if (!navigator.mediaDevices || !navigator.mediaDevices.getUserMedia) {
        setAudioError(
          "Microphone access is blocked. Please check your browser permissions or disable Lockdown Mode for this site (Settings > Safari > Advanced > Lockdown Mode)."
        );
        setTimeout(() => setAudioError(null), 8000);
        return;
      }

      const stream = await navigator.mediaDevices.getUserMedia({
        audio: {
          echoCancellation: false,
          noiseSuppression: true,
          autoGainControl: false,
          sampleRate: 16000
        }
      });

      streamRef.current = stream;

      // Create RecordRTC instance configured for WAV
      const recorder = new RecordRTC(stream, {
        type: "audio",
        mimeType: "audio/wav",
        recorderType: RecordRTC.StereoAudioRecorder,
        numberOfAudioChannels: 1,
        desiredSampRate: 16000
      });

      recorderRef.current = recorder;
      recorder.startRecording();
      setIsRecording(true);
      setAudioError(null);
    } catch (error) {
      console.error("Failed to start recording:", error);
      const err = error as Error & { name?: string };

      // Handle different error types
      if (err.name === "NotAllowedError" || err.name === "PermissionDeniedError") {
        setAudioError(
          "Microphone access denied. Please enable microphone permissions in Settings > Maple."
        );
      } else if (err.name === "NotFoundError" || err.name === "DevicesNotFoundError") {
        setAudioError("No microphone found. Please check your device.");
      } else if (err.name === "NotReadableError" || err.name === "TrackStartError") {
        setAudioError("Microphone is already in use by another app.");
      } else {
        setAudioError(
          `Failed to access microphone: ${err.name || "Unknown error"} - ${err.message || "Please try again"}`
        );
      }

      setTimeout(() => setAudioError(null), 5000);
    }
  };

  const stopRecording = (shouldSend: boolean = false) => {
    if (recorderRef.current && isRecording) {
      // Only hide immediately if canceling, keep visible if sending
      if (!shouldSend) {
        setIsRecording(false);
      } else {
        setIsProcessingSend(true);
      }

      recorderRef.current.stopRecording(async () => {
        const blob = recorderRef.current?.getBlob();

        if (!blob || blob.size === 0) {
          console.error("No audio recorded or empty recording");
          if (shouldSend) {
            setAudioError("No audio was recorded. Please try again.");
            setTimeout(() => setAudioError(null), 5000);
          }
          // Clean up
          if (streamRef.current) {
            streamRef.current.getTracks().forEach((track) => track.stop());
            streamRef.current = null;
          }
          recorderRef.current = null;
          setIsProcessingSend(false);
          setIsRecording(false);
          return;
        }

        // Create a proper WAV file
        const audioFile = new File([blob], "recording.wav", {
          type: "audio/wav"
        });

        if (shouldSend) {
          setIsTranscribing(true);
          try {
            const result = await os.transcribeAudio(audioFile, "whisper-large-v3");
            const transcribedText = result.text.trim();

            if (transcribedText) {
              // Combine with existing input if any
              const newValue = input ? `${input} ${transcribedText}` : transcribedText;

              // Clear states before sending
              setInput("");
              clearAllAttachments();
              setIsRecording(false);
              setIsTranscribing(false);
              setIsProcessingSend(false);

              // Send the message directly with the transcribed text
              await handleSendMessage(undefined, newValue);
            } else {
              setAudioError("No speech detected. Please try again.");
              setTimeout(() => setAudioError(null), 5000);
            }
          } catch (error) {
            console.error("Transcription failed:", error);
            setAudioError("Failed to transcribe audio. Please try again.");
            setTimeout(() => setAudioError(null), 5000);
          } finally {
            setIsTranscribing(false);
            setIsProcessingSend(false);
            setIsRecording(false);
          }
        }

        // Clean up resources
        if (streamRef.current) {
          streamRef.current.getTracks().forEach((track) => track.stop());
          streamRef.current = null;
        }
        recorderRef.current = null;
      });
    }
  };

  // Helper function to process streaming response - used by both initial request and retry
  const processStreamingResponse = useCallback(
    async (stream: AsyncIterable<unknown>) => {
      const messageTextBuffers = new Map<string, Map<number, string>>();
      const reasoningTextBuffers = new Map<string, Map<number, string>>();
      const streamStartedAt = performance.now();
      let streamEventCount = 0;

      const appendBufferedText = (
        buffers: Map<string, Map<number, string>>,
        itemId: string,
        contentIndex: number,
        delta: string
      ) => {
        const contentBuffers = buffers.get(itemId) ?? new Map<number, string>();
        const nextText = `${contentBuffers.get(contentIndex) ?? ""}${delta}`;
        contentBuffers.set(contentIndex, nextText);
        buffers.set(itemId, contentBuffers);
        return nextText;
      };

      const setBufferedText = (
        buffers: Map<string, Map<number, string>>,
        itemId: string,
        contentIndex: number,
        text: string
      ) => {
        const contentBuffers = buffers.get(itemId) ?? new Map<number, string>();
        contentBuffers.set(contentIndex, text);
        buffers.set(itemId, contentBuffers);
        return text;
      };

      for await (const event of stream) {
        const eventType = (event as { type: string }).type;
        streamEventCount += 1;
        logStreamEvent(streamStartedAt, streamEventCount, eventType, event);

        if (eventType === "response.created") {
          const eventWithResponse = event as { response?: { id?: string } };
          if (eventWithResponse.response?.id) {
            setCurrentResponseId(eventWithResponse.response.id);
          }
        } else if (eventType === "response.output_item.added") {
          const addedEvent = event as ResponseOutputItemAddedEvent;
          const item = normalizeConversationItem(addedEvent.item);

          if (item) {
            setMessages((prev) => mergeStreamingConversationItem(prev, item));
          }
        } else if (eventType === "response.web_search_call.in_progress") {
          const webSearchEvent = event as { item_id?: string };
          if (webSearchEvent.item_id) {
            setMessages((prev) =>
              updateMessageById(prev, webSearchEvent.item_id!, (message) =>
                message.type === "web_search_call"
                  ? ({ ...message, status: "in_progress" } as unknown as Message)
                  : message
              )
            );
          }
        } else if (eventType === "response.web_search_call.searching") {
          const webSearchEvent = event as { item_id?: string };
          if (webSearchEvent.item_id) {
            setMessages((prev) =>
              updateMessageById(prev, webSearchEvent.item_id!, (message) =>
                message.type === "web_search_call"
                  ? ({ ...message, status: "searching" } as unknown as Message)
                  : message
              )
            );
          }
        } else if (eventType === "response.web_search_call.completed") {
          const webSearchEvent = event as { item_id?: string };
          if (webSearchEvent.item_id) {
            setMessages((prev) =>
              updateMessageById(prev, webSearchEvent.item_id!, (message) =>
                message.type === "web_search_call"
                  ? ({ ...message, status: "completed" } as unknown as Message)
                  : message
              )
            );
          }
        } else if (eventType === "tool_call.created") {
          const toolCallEvent = event as {
            tool_call_id?: string;
            name?: string;
            arguments?: { query?: string };
          };
          if (toolCallEvent.tool_call_id) {
            const toolCallItem: ToolCallItem = {
              id: toolCallEvent.tool_call_id,
              call_id: toolCallEvent.tool_call_id,
              type: "tool_call",
              name: toolCallEvent.name || "function",
              arguments: JSON.stringify(toolCallEvent.arguments || {}),
              status: "in_progress"
            };

            setMessages((prev) => {
              const existingToolCall = prev.find(
                (message) => message.id === toolCallEvent.tool_call_id
              );

              if (existingToolCall && isToolCallItem(existingToolCall)) {
                return mergeMessagesById(prev, [
                  {
                    ...existingToolCall,
                    ...toolCallItem,
                    status: existingToolCall.status || toolCallItem.status
                  }
                ]);
              }

              return mergeMessagesById(prev, [toolCallItem]);
            });
          }
        } else if (eventType === "tool_output.created") {
          const toolOutputEvent = event as {
            tool_output_id?: string;
            tool_call_id?: string;
            output?: string;
          };
          if (toolOutputEvent.tool_output_id && toolOutputEvent.tool_call_id) {
            const toolOutputItem: ToolOutputItem = {
              id: toolOutputEvent.tool_output_id,
              call_id: toolOutputEvent.tool_call_id,
              type: "tool_output",
              output: toolOutputEvent.output || "",
              status: "completed"
            };

            setMessages((prev) => {
              const existingToolOutput = prev.find(
                (message) => message.id === toolOutputEvent.tool_output_id
              );
              const withOutput = mergeMessagesById(prev, [
                existingToolOutput && isToolOutputItem(existingToolOutput)
                  ? {
                      ...existingToolOutput,
                      ...toolOutputItem
                    }
                  : toolOutputItem
              ]);

              return updateMessageById(withOutput, toolOutputEvent.tool_call_id!, (message) =>
                isToolCallItem(message)
                  ? ({ ...message, status: "completed" } as unknown as Message)
                  : message
              );
            });
          }
        } else if (
          eventType === "response.reasoning_text.delta" &&
          (event as { delta?: string }).delta
        ) {
          const reasoningEvent = event as ResponseReasoningTextDeltaEvent;
          const nextText = appendBufferedText(
            reasoningTextBuffers,
            reasoningEvent.item_id,
            reasoningEvent.content_index,
            reasoningEvent.delta
          );

          setMessages((prev) =>
            updateMessageById(prev, reasoningEvent.item_id, (message) =>
              message.type === "reasoning"
                ? upsertReasoningTextContent(
                    message as ReasoningItem,
                    reasoningEvent.content_index,
                    nextText,
                    "streaming"
                  )
                : message
            )
          );
        } else if (eventType === "response.reasoning_text.done") {
          const reasoningEvent = event as ResponseReasoningTextDoneEvent;
          const nextText = setBufferedText(
            reasoningTextBuffers,
            reasoningEvent.item_id,
            reasoningEvent.content_index,
            reasoningEvent.text
          );

          setMessages((prev) =>
            updateMessageById(prev, reasoningEvent.item_id, (message) =>
              message.type === "reasoning"
                ? upsertReasoningTextContent(
                    message as ReasoningItem,
                    reasoningEvent.content_index,
                    nextText,
                    "completed"
                  )
                : message
            )
          );
        } else if (
          eventType === "response.output_text.delta" &&
          (event as { delta?: string }).delta
        ) {
          const textEvent = event as ResponseTextDeltaEvent;
          const nextText = appendBufferedText(
            messageTextBuffers,
            textEvent.item_id,
            textEvent.content_index,
            textEvent.delta
          );

          setMessages((prev) =>
            updateMessageById(prev, textEvent.item_id, (message) =>
              message.type === "message"
                ? (upsertAssistantTextContent(
                    message as ExtendedMessage,
                    textEvent.content_index,
                    nextText,
                    "streaming"
                  ) as unknown as Message)
                : message
            )
          );
        } else if (eventType === "response.output_text.done") {
          const textEvent = event as ResponseTextDoneEvent;
          const nextText = setBufferedText(
            messageTextBuffers,
            textEvent.item_id,
            textEvent.content_index,
            textEvent.text
          );

          setMessages((prev) =>
            updateMessageById(prev, textEvent.item_id, (message) =>
              message.type === "message"
                ? (upsertAssistantTextContent(
                    message as ExtendedMessage,
                    textEvent.content_index,
                    nextText,
                    "completed"
                  ) as unknown as Message)
                : message
            )
          );
        } else if (eventType === "response.output_item.done") {
          const doneEvent = event as ResponseOutputItemDoneEvent;
          const item = normalizeConversationItem(doneEvent.item);

          if (item) {
            setMessages((prev) => mergeStreamingConversationItem(prev, item));
            setLastSeenItemId(item.id);
          }
        } else if (eventType === "response.failed" || eventType === "error") {
          console.error("Streaming error:", event);
          setMessages((prev) => updateActiveItemStatuses(prev, "error"));
          setError("Failed to generate response. Please try again.");
        } else if (eventType === "response.cancelled") {
          setMessages((prev) => updateActiveItemStatuses(prev, "incomplete"));
          break;
        }
      }
    },
    [logStreamEvent]
  );

  // Send message handler - now accepts optional override text for voice input
  const handleSendMessage = useCallback(
    async (e?: React.FormEvent, overrideInput?: string) => {
      e?.preventDefault();

      // Use override input (from voice) or regular input
      const textToSend = overrideInput || input;
      const trimmedInput = textToSend.trim();
      const hasContent = trimmedInput || draftImages.length > 0 || documentText;
      if (!hasContent || isGenerating || !openai) return;

      // Clear any previous error
      setError(null);

      // Build the message content - always as an array
      // Using the input types since we're building user input
      const messageContent: (InputTextContent | InputImageContent)[] = [];

      // Combine document text with input if both exist
      let finalText = trimmedInput;
      if (documentText) {
        finalText = documentText + (trimmedInput ? `\n\n${trimmedInput}` : "");
      }

      // Add text part if exists
      if (finalText) {
        const textContent: InputTextContent = {
          type: "input_text",
          text: finalText
        };
        messageContent.push(textContent);
      }

      // Add image parts if we have images
      for (const file of draftImages) {
        try {
          const dataUrl = await fileToDataURL(file);
          const imageContent: InputImageContent = {
            type: "input_image",
            image_url: dataUrl,
            detail: "auto",
            file_id: null
          };
          messageContent.push(imageContent);
        } catch (error) {
          console.error("Failed to convert image:", error);
        }
      }

      // Add user message immediately with a local UUID
      const localMessageId = uuidv4();
      const userMessage = {
        id: localMessageId,
        type: "message",
        role: "user",
        content: messageContent,
        status: "completed"
      } as unknown as Message;

      // Use merge helper to add user message (prevents duplicates)
      setMessages((prev) => mergeMessagesById(prev, [userMessage]));
      // Set lastSeenItemId to our local message ID
      // The backend should map this via internal_message_id
      setLastSeenItemId(localMessageId);

      // Store the original input and attachments in case we need to restore them
      const originalInput = trimmedInput;
      const originalImages = [...draftImages];
      const originalDocumentText = documentText;
      const originalDocumentName = documentName;

      // Only clear input if not using override (voice already cleared it)
      if (!overrideInput) {
        setInput("");
        clearAllAttachments();
      }
      setIsGenerating(true);

      try {
        // Create conversation if we don't have one
        let conversationId = conversation?.id;
        if (!conversationId) {
          const newConv = draftProjectId
            ? await os.createConversation(
                {},
                {
                  project_id: draftProjectId
                }
              )
            : await openai.conversations.create({
                metadata: {}
              });
          conversationId = newConv.id;
          setConversation(newConv as Conversation);

          // Update URL with new conversation ID
          const usp = new URLSearchParams(window.location.search);
          usp.set("conversation_id", conversationId);
          window.history.replaceState(null, "", `${window.location.pathname}?${usp.toString()}`);

          // Update local state but flag that we just created it
          setIsNewConversationJustCreated(true);
          setChatId(conversationId);

          // Trigger sidebar refresh to show the new conversation
          window.dispatchEvent(new Event("conversationcreated"));
        }

        // Create abort controller for this request
        const abortController = new AbortController();
        abortControllerRef.current = abortController;

        // Create streaming response - the API expects the content directly as we built it
        const stream = await openai.responses.create(
          {
            conversation: conversationId,
            model: localState.model || DEFAULT_MODEL_ID, // Use selected model or default
            input: [{ role: "user", content: messageContent }],
            metadata: { internal_message_id: localMessageId }, // Pass our local ID
            stream: true,
            store: true, // Store in conversation history
            ...(isWebSearchEnabled && { tools: [{ type: "web_search" }] })
          },
          { signal: abortController.signal }
        );

        // Disable polling while streaming is active
        assistantStreamingRef.current = true;

        try {
          // Process the streaming response
          await processStreamingResponse(stream);
        } finally {
          // Re-enable polling after streaming completes
          assistantStreamingRef.current = false;
          setCurrentResponseId(undefined);

          // Invalidate billing status after a delay to allow backend processing
          billingRefreshTimeoutRef.current = setTimeout(() => {
            queryClient.invalidateQueries({ queryKey: ["billingStatus"] });
            billingRefreshTimeoutRef.current = null;
          }, 3000);
        }
      } catch (error) {
        console.error("Failed to send message:", error);

        // Handle usage limit errors with upsell dialogs
        // The SDK throws errors with the message "Request failed with status 403: {json}" or "Request failed with status 413: {json}"
        // We need to parse this to extract the actual error details
        let errorMessage = error instanceof Error ? error.message : "Something went wrong";

        // Also check the cause property if it exists
        const causeMessage = (error as Error & { cause?: { message?: string } })?.cause?.message;
        if (causeMessage && causeMessage.includes("Request failed with status")) {
          errorMessage = causeMessage;
        }

        // Check for 413 error (Message exceeds context limit)
        let status413Error: { status: number; message: string } | null = null;
        if (errorMessage.includes("Request failed with status 413:")) {
          try {
            // Extract the JSON part from the error message
            const jsonMatch = errorMessage.match(/Request failed with status 413:\s*({.*})/);
            if (jsonMatch && jsonMatch[1]) {
              status413Error = JSON.parse(jsonMatch[1]);
            }
          } catch (parseError) {
            console.error("Failed to parse 413 error:", parseError);
          }
        }

        if (status413Error && status413Error.message === "Message exceeds context limit") {
          // Remove the user message from history and restore input
          setMessages((prev) => prev.filter((msg) => msg.id !== localMessageId));

          // Restore the original input and attachments
          if (!overrideInput) {
            setInput(originalInput);
            setDraftImages(originalImages);
            // Re-create object URLs since originals were revoked by clearAllAttachments()
            const restoredUrlMap = new Map<File, string>();
            for (const file of originalImages) {
              restoredUrlMap.set(file, URL.createObjectURL(file));
            }
            setImageUrls(restoredUrlMap);
            setDocumentText(originalDocumentText);
            setDocumentName(originalDocumentName);
          }

          // Show the context limit dialog
          setContextLimitDialogOpen(true);
          setError("Your message exceeds the context limit for this model.");
          return; // Exit early, don't continue to other error handling
        }

        let status403Error: { status: number; message: string } | null = null;

        // Check if this is a 403 error from the SDK
        if (errorMessage.includes("Request failed with status 403:")) {
          try {
            // Extract the JSON part from the error message
            const jsonMatch = errorMessage.match(/Request failed with status 403:\s*({.*})/);
            if (jsonMatch && jsonMatch[1]) {
              status403Error = JSON.parse(jsonMatch[1]);
            }
          } catch (parseError) {
            console.error("Failed to parse 403 error:", parseError);
          }
        }

        if (status403Error) {
          // Remove the user message from history and restore input
          setMessages((prev) => prev.filter((msg) => msg.id !== localMessageId));

          // Restore the original input and attachments
          if (!overrideInput) {
            setInput(originalInput);
            setDraftImages(originalImages);
            // Re-create object URLs since originals were revoked by clearAllAttachments()
            const restoredUrlMap = new Map<File, string>();
            for (const file of originalImages) {
              restoredUrlMap.set(file, URL.createObjectURL(file));
            }
            setImageUrls(restoredUrlMap);
            setDocumentText(originalDocumentText);
            setDocumentName(originalDocumentName);
          }

          if (status403Error.message === "Free tier token limit exceeded") {
            // Token limit exceeded - conversation too long for free tier
            setUpgradeFeature("tokens");
            setUpgradeDialogOpen(true);
            setError(
              "This conversation is too long for the free tier. Upgrade to Pro for longer conversations."
            );
          } else if (status403Error.message === "Usage limit reached") {
            // Usage limit reached - could be daily (free) or monthly (paid)
            const isFreeTier =
              !billingStatus?.product_name || billingStatus.product_name.toLowerCase() === "free";

            if (isFreeTier) {
              // Free tier hit daily limits
              setUpgradeFeature("usage");
              setUpgradeDialogOpen(true);
              setError("You've reached your daily usage limit. Upgrade to Pro for more chats.");
            } else {
              // Paid tier hit monthly limits - upsell to next tier
              setUpgradeFeature("usage");
              setUpgradeDialogOpen(true);
              const isPro =
                billingStatus.product_name?.toLowerCase().includes("pro") &&
                !billingStatus.product_name?.toLowerCase().includes("max");
              setError(
                isPro
                  ? "You've reached your monthly Pro limit. Upgrade to Max for 10x more usage."
                  : "You've reached your monthly usage limit. Please wait for the next billing cycle."
              );
            }
          } else {
            setError(status403Error.message || "Access denied. Please check your subscription.");
          }
        } else if (error instanceof Error && error.name !== "AbortError") {
          // Retry logic for non-rate-limit errors on follow-up conversations
          // Only retry if this is a follow-up (conversation already existed before this request)
          const isFollowUpConversation = conversation?.id && messages.length > 1;

          if (isFollowUpConversation && conversation?.id) {
            try {
              // Wait 1 second before retrying
              console.log("Waiting 1s before retry...");
              await new Promise((resolve) => setTimeout(resolve, 1000));

              console.log("Retrying request once...");
              // TODO: Consider calling os.getAttestation() here if needed for attestation refresh

              // Create new abort controller for retry
              const retryAbortController = new AbortController();
              abortControllerRef.current = retryAbortController;

              const retryStream = await openai.responses.create(
                {
                  conversation: conversation.id,
                  model: localState.model || DEFAULT_MODEL_ID,
                  input: [{ role: "user", content: messageContent }],
                  metadata: { internal_message_id: localMessageId }, // Server prevents duplicate IDs
                  stream: true,
                  store: true,
                  ...(isWebSearchEnabled && { tools: [{ type: "web_search" }] })
                },
                { signal: retryAbortController.signal }
              );

              // Disable polling while streaming is active
              assistantStreamingRef.current = true;

              try {
                // Process the retry stream using the same helper function
                await processStreamingResponse(retryStream);
                console.log("Retry completed successfully");
              } finally {
                // Re-enable polling after streaming completes
                assistantStreamingRef.current = false;
              }
              return;
            } catch (retryError) {
              // Retry failed - check one last time if message actually went through
              console.error("Retry failed:", retryError);

              try {
                // Check the last 5 items to see if our message is there
                const finalCheckResponse = await openai.conversations.items.list(conversation.id, {
                  limit: 5,
                  order: "desc"
                });

                // Look for our message by ID - server uses our internal_message_id as the item's ID
                const foundMessage = finalCheckResponse.data.find(
                  (item) => item.id === localMessageId
                );

                if (!foundMessage) {
                  // Message definitely didn't go through - restore input and remove from UI
                  console.log("Message not found after retry - restoring input");

                  // Remove the user message from the messages list
                  setMessages((prev) => prev.filter((msg) => msg.id !== localMessageId));

                  // Restore the original input and attachments
                  if (!overrideInput) {
                    setInput(originalInput);
                    setDraftImages(originalImages);
                    // Re-create object URLs since originals were revoked by clearAllAttachments()
                    const restoredUrlMap = new Map<File, string>();
                    for (const file of originalImages) {
                      restoredUrlMap.set(file, URL.createObjectURL(file));
                    }
                    setImageUrls(restoredUrlMap);
                    setDocumentText(originalDocumentText);
                    setDocumentName(originalDocumentName);
                  }

                  setError("Failed to send message. Please try again.");
                } else {
                  // Message actually went through! Just log it
                  console.log("Message found after retry failure - it actually went through");
                }
              } catch (finalCheckError) {
                // If we can't even check, assume message failed and restore input
                console.error("Final check failed:", finalCheckError);

                // Remove the user message from the messages list
                setMessages((prev) => prev.filter((msg) => msg.id !== localMessageId));

                // Restore the original input and attachments
                if (!overrideInput) {
                  setInput(originalInput);
                  setDraftImages(originalImages);
                  // Re-create object URLs since originals were revoked by clearAllAttachments()
                  const restoredUrlMap = new Map<File, string>();
                  for (const file of originalImages) {
                    restoredUrlMap.set(file, URL.createObjectURL(file));
                  }
                  setImageUrls(restoredUrlMap);
                  setDocumentText(originalDocumentText);
                  setDocumentName(originalDocumentName);
                }

                setError("Failed to send message. Please try again.");
              }
            }
          } else {
            // Not a follow-up conversation or other non-retryable error
            setError(errorMessage + ". Please try again.");
          }
        }
      } finally {
        setIsGenerating(false);
        setCurrentResponseId(undefined);
        abortControllerRef.current = null;
        assistantStreamingRef.current = false;
      }
    },
    [
      input,
      isGenerating,
      openai,
      os,
      conversation,
      draftProjectId,
      localState.model,
      draftImages,
      documentText,
      clearAllAttachments,
      processStreamingResponse,
      isWebSearchEnabled
    ]
  );

  const handleKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    // On desktop: Enter submits, Shift+Enter for new line
    // On mobile: Enter for new line, no keyboard shortcut to submit (use button)
    if (e.key === "Enter" && !e.shiftKey && !isCompactLayout) {
      e.preventDefault();
      handleSendMessage();
    }
  };

  const sidebarLayoutStyle = getSidebarLayoutStyle({ offsetContent: isSidebarOpen });

  return (
    <div
      style={sidebarLayoutStyle}
      className={`grid h-dvh min-h-0 w-full grid-cols-1 overflow-hidden ${isSidebarOpen ? SIDEBAR_GRID_COLUMNS_CLASS : ""}`}
    >
      {/* Use the existing Sidebar component */}
      <Sidebar chatId={chatId} isOpen={isSidebarOpen} onToggle={toggleSidebar} />

      {/* Main Content */}
      <div className="flex flex-col flex-1 min-w-0 min-h-0 bg-background overflow-hidden relative">
        {/* Error message - fixed at top below header, always visible */}
        {error && (
          <div className={CHAT_ALERT_CLASS}>
            <Alert variant="destructive" className="bg-background">
              <AlertCircle className="h-4 w-4" />
              <AlertDescription>{error}</AlertDescription>
            </Alert>
          </div>
        )}

        {/* TTS playback error - shows when audio context is unavailable (e.g., Lockdown Mode) */}
        {playbackError && (
          <div className={CHAT_ALERT_CLASS}>
            <Alert variant="destructive" className="bg-background">
              <AlertCircle className="h-4 w-4" />
              <AlertDescription className="flex items-center justify-between">
                <span>{playbackError}</span>
                <Button
                  variant="ghost"
                  size="sm"
                  className="h-6 px-2 ml-2"
                  onClick={clearPlaybackError}
                  aria-label="Dismiss TTS error"
                >
                  <X className="h-4 w-4" />
                </Button>
              </AlertDescription>
            </Alert>
          </div>
        )}

        {/* Sidebar toggle + wordmark — fixed except on compact layouts while chatting (two-row header below) */}
        {!isSidebarOpen && !(isCompactLayout && messages.length > 0) && (
          <div className="fixed left-4 top-[9.5px] z-20 flex items-center gap-1.5">
            <SidebarToggle onToggle={toggleSidebar} />
            <MapleWordmark
              className="h-4 w-auto animate-in fade-in-0 slide-in-from-left-1 duration-300"
              aria-hidden
            />
          </div>
        )}

        {/* Only show header when there are messages (conversation exists) */}
        {messages.length > 0 &&
          (isLandscapeMobile && !isSidebarOpen ? (
            <div className="z-10 flex shrink-0 items-center gap-2 bg-background px-1 py-1 pr-4">
              <SidebarToggle onToggle={toggleSidebar} />
              <div className="min-w-0 overflow-hidden">
                <MapleWordmark className="h-4 w-auto max-w-full" aria-hidden />
              </div>
              <h1
                className={`min-w-0 flex-1 truncate px-1 text-center text-base font-medium text-foreground transition-colors duration-300 ${
                  titleJustUpdated ? "title-update-animation" : ""
                }`}
              >
                {conversation?.metadata?.title || "Chat"}
              </h1>
              <Button
                variant="outline"
                size="icon"
                className="h-9 w-9 shrink-0 border-0"
                onClick={handleNewChatFromHeader}
                aria-label="New chat"
              >
                <SquarePen className="h-4 w-4" />
              </Button>
            </div>
          ) : isMobile && !isSidebarOpen ? (
            <div className="z-10 flex shrink-0 flex-col gap-2 bg-background pb-2 pl-1 pr-4 pt-2">
              <div className="flex items-center justify-between gap-3">
                <div className="flex min-w-0 flex-1 items-center gap-1.5">
                  <SidebarToggle onToggle={toggleSidebar} />
                  <div className="min-w-0 overflow-hidden">
                    <MapleWordmark className="h-4 w-auto max-w-full" aria-hidden />
                  </div>
                </div>
                <Button
                  variant="outline"
                  size="icon"
                  className="h-9 w-9 shrink-0 border-0"
                  onClick={handleNewChatFromHeader}
                  aria-label="New chat"
                >
                  <SquarePen className="h-4 w-4" />
                </Button>
              </div>
              <h1
                className={`w-full truncate px-1 text-center text-base font-medium text-foreground transition-colors duration-300 ${
                  titleJustUpdated ? "title-update-animation" : ""
                }`}
              >
                {conversation?.metadata?.title || "Chat"}
              </h1>
            </div>
          ) : (
            <div className="h-14 flex items-center px-4">
              <div className="relative flex flex-1 items-center justify-center">
                <h1
                  className={`max-w-[20rem] truncate text-base font-medium text-foreground transition-colors duration-300 ${
                    titleJustUpdated ? "title-update-animation" : ""
                  }`}
                >
                  {conversation?.metadata?.title || "Chat"}
                </h1>
                {!isSidebarOpen && (
                  <Button
                    variant="outline"
                    size="icon"
                    className="absolute right-0 h-9 w-9 border-0"
                    onClick={handleNewChatFromHeader}
                    aria-label="New chat"
                  >
                    <SquarePen className="h-4 w-4" />
                  </Button>
                )}
              </div>
            </div>
          ))}

        {/* Messages Area */}
        <div
          ref={chatContainerRef}
          className="flex-1 min-h-0 overflow-y-auto overscroll-y-contain flex flex-col relative"
        >
          {/* Only show messages when there are messages */}
          {messages.length > 0 && (
            <div className="mx-auto w-full max-w-4xl p-4 md:p-6 landscape-short:p-2">
              {/* Message list with modern ChatGPT/Claude style */}
              <div className="space-y-1">
                <MessageList
                  messages={messages}
                  isGenerating={isGenerating}
                  chatId={chatId}
                  firstMessageRef={firstMessageRef}
                  isLoadingOlderMessages={isLoadingOlderMessages}
                  onTTSSetupOpen={() => setTtsSetupDialogOpen(true)}
                  onTTSManage={() => setTtsSetupDialogOpen(true)}
                />
              </div>

              <div ref={messagesEndRef} />
            </div>
          )}
        </div>

        {/* Input Area - centered when no messages, fixed at bottom when chatting */}
        {messages.length === 0 && !chatId ? (
          // Centered input for new chat
          <div
            className={`absolute inset-0 flex flex-col px-4 ${
              isFullscreenAnimating ? "transition-all duration-300" : ""
            } ${isFullscreen ? "justify-start pt-8" : "justify-center"}`}
          >
            <div
              className={`mx-auto w-full ${
                isFullscreenAnimating ? "transition-all duration-300" : ""
              } ${isFullscreen ? "flex h-full max-w-6xl flex-col" : "max-w-4xl"}`}
            >
              {!isFullscreen && <div className="mb-16 landscape-short:mb-4" />}

              <div
                className={`flex flex-col items-center gap-6 landscape-short:gap-3 ${isFullscreen ? "flex-1 justify-center" : ""}`}
              >
                {!isFullscreen && (
                  <h1 className="mb-6 landscape-short:mb-2 w-full overflow-visible pb-1 text-center font-displayWide text-4xl landscape-short:text-2xl font-normal leading-tight brand-gradient-text sm:leading-relaxed">
                    Research anything...
                  </h1>
                )}

                <form onSubmit={handleSendMessage} className="relative w-full">
                  <div className="space-y-2">
                    {(draftImages.length > 0 || documentName) && (
                      <div className="space-y-2">
                        {draftImages.length > 0 && (
                          <div className="flex flex-wrap gap-2">
                            {draftImages.map((file, i) => (
                              <div key={i} className="group relative">
                                <img
                                  src={imageUrls.get(file) || ""}
                                  alt={`Attachment ${i + 1}`}
                                  className="h-16 w-16 rounded-xl border object-cover"
                                />
                                <button
                                  type="button"
                                  onClick={() => removeImage(i)}
                                  className="absolute -right-1 -top-1 rounded-full border bg-background p-0.5 opacity-0 transition-opacity group-hover:opacity-100"
                                >
                                  <X className="h-3 w-3" />
                                </button>
                              </div>
                            ))}
                          </div>
                        )}

                        {documentName && (
                          <div className="flex items-center gap-2 rounded-2xl bg-muted/50 p-2">
                            <FileText className="h-4 w-4 text-muted-foreground" />
                            <span className="flex-1 truncate text-sm">{documentName}</span>
                            <button
                              type="button"
                              onClick={removeDocument}
                              className="text-muted-foreground hover:text-foreground"
                            >
                              <X className="h-3 w-3" />
                            </button>
                          </div>
                        )}
                      </div>
                    )}

                    {(attachmentError || audioError) && (
                      <div className="px-2 text-sm text-maple-error">
                        {attachmentError || audioError}
                      </div>
                    )}

                    <div
                      className={`relative flex flex-col overflow-hidden rounded-3xl border border-[hsl(var(--maple-secondary-container))] bg-background focus-within:border-[hsl(var(--maple-primary))] ${
                        isFullscreenAnimating ? "transition-all duration-300" : "transition-colors"
                      } ${isFullscreen ? "h-[70vh] max-h-[800px] min-h-0" : ""}`}
                    >
                      <button
                        type="button"
                        onClick={toggleFullscreen}
                        className="absolute right-2 top-2 z-10 rounded-full p-1.5 text-muted-foreground/60 transition-colors hover:bg-muted/50 hover:text-foreground"
                        aria-label={isFullscreen ? "Exit fullscreen" : "Enter fullscreen"}
                      >
                        {isFullscreen ? (
                          <Shrink className="h-4 w-4" />
                        ) : (
                          <Expand className="h-4 w-4" />
                        )}
                      </button>
                      <Textarea
                        ref={textareaRef}
                        value={input}
                        onChange={(e) => setInput(e.target.value)}
                        onKeyDown={handleKeyDown}
                        placeholder="Message Maple..."
                        disabled={isGenerating || isRecording}
                        className={`resize-none border-0 bg-transparent pl-4 pr-8 text-base leading-6 focus-visible:ring-0 focus-visible:ring-offset-0 placeholder:text-muted-foreground/60 ${
                          isFullscreen
                            ? "flex-1 min-h-0 pt-3 pb-2"
                            : "min-h-[52px] max-h-[200px] pt-3 pb-2"
                        }`}
                        rows={isFullscreen ? undefined : 1}
                        id="message"
                      />

                      <div className="grid shrink-0 grid-cols-[minmax(0,1fr)_auto] items-end gap-x-2 gap-y-2 px-2 pb-2 pt-1">
                        <div className="flex min-w-0 flex-wrap items-center gap-1.5 sm:gap-2">
                          <ModelSelector
                            hasImages={
                              draftImages.length > 0 ||
                              messages.some(
                                (msg) =>
                                  msg.type === "message" &&
                                  (msg as ExtendedMessage).content?.some(
                                    (part: ConversationContent) => part.type === "input_image"
                                  )
                              )
                            }
                          />

                          <ConversationProjectPicker
                            selectedProjectId={draftProjectId}
                            onSelect={setDraftProjectId}
                            disabled={isGenerating}
                          />

                          <Button
                            type="button"
                            variant="ghost"
                            size="sm"
                            className="h-8 w-8 p-0 text-[hsl(var(--maple-secondary-700))] hover:bg-[hsl(var(--maple-primary-container))] hover:text-[hsl(var(--maple-secondary-700))]"
                            onClick={() => {
                              const newValue = !isWebSearchEnabled;
                              setIsWebSearchEnabled(newValue);
                              localStorage.setItem("webSearchEnabled", newValue.toString());
                            }}
                            aria-label={
                              isWebSearchEnabled ? "Disable web search" : "Enable web search"
                            }
                          >
                            <Globe
                              className={`h-4 w-4 ${
                                isWebSearchEnabled
                                  ? "text-[hsl(var(--maple-primary))]"
                                  : "text-[hsl(var(--maple-secondary-700))]"
                              }`}
                            />
                          </Button>

                          <DropdownMenu>
                            <DropdownMenuTrigger asChild>
                              <Button
                                type="button"
                                variant="ghost"
                                size="sm"
                                className="h-8 w-8 p-0 text-[hsl(var(--maple-secondary-700))] hover:bg-[hsl(var(--maple-primary-container))] hover:text-[hsl(var(--maple-secondary-700))]"
                                disabled={isProcessingDocument}
                              >
                                <Plus className="h-4 w-4 text-[hsl(var(--maple-secondary-700))]" />
                              </Button>
                            </DropdownMenuTrigger>
                            <DropdownMenuContent align="start">
                              <DropdownMenuItem
                                onClick={() => {
                                  if (!canUseImages) {
                                    setUpgradeFeature("image");
                                    setUpgradeDialogOpen(true);
                                  } else {
                                    fileInputRef.current?.click();
                                  }
                                }}
                              >
                                <Image className="mr-2 h-4 w-4" />
                                <span>Add Images</span>
                              </DropdownMenuItem>
                              <DropdownMenuItem
                                onClick={() => {
                                  if (!isTauriEnv) {
                                    setDocumentPlatformDialogOpen(true);
                                  } else if (!canUseDocuments) {
                                    setUpgradeFeature("document");
                                    setUpgradeDialogOpen(true);
                                  } else {
                                    documentInputRef.current?.click();
                                  }
                                }}
                              >
                                <FileText className="mr-2 h-4 w-4" />
                                <span>Add Document</span>
                              </DropdownMenuItem>
                            </DropdownMenuContent>
                          </DropdownMenu>
                        </div>

                        <div className="flex shrink-0 items-center self-end gap-1.5 sm:gap-2">
                          <Button
                            type="button"
                            onClick={startRecording}
                            disabled={isGenerating || isRecording || !canUseVoice}
                            size="icon"
                            variant="ghost"
                            className="h-8 w-8 rounded-xl hover:bg-muted sm:h-9 sm:w-9"
                          >
                            <Mic className="h-4 w-4" />
                          </Button>
                          {isGenerating ? (
                            <Button
                              type="button"
                              onClick={handleCancelResponse}
                              size="icon"
                              variant="destructive"
                              className="h-8 w-8 rounded-xl sm:h-9 sm:w-9"
                            >
                              <div className="h-3 w-3 rounded-md bg-current" />
                            </Button>
                          ) : (
                            <button
                              type="submit"
                              disabled={!input.trim() && !draftImages.length && !documentText}
                              className="flex h-8 w-8 items-center justify-center rounded-full bg-gradient-to-b from-[hsl(var(--maple-primary))] to-[hsl(var(--maple-primary-strong))] text-[hsl(var(--maple-on-primary))]/90 transition-all duration-200 ease-out active:scale-[0.95] disabled:pointer-events-none disabled:opacity-40 sm:h-9 sm:w-9"
                            >
                              <ArrowUp className="h-3.5 w-3.5 sm:h-4 sm:w-4" />
                            </button>
                          )}
                        </div>
                      </div>

                      {isRecording && (
                        <RecordingOverlay
                          isRecording={isRecording}
                          isProcessing={isProcessingSend || isTranscribing}
                          onSend={() => stopRecording(true)}
                          onCancel={() => stopRecording(false)}
                          isCompact={false}
                          className="absolute inset-0 rounded-3xl"
                        />
                      )}
                    </div>
                  </div>
                </form>

                {!isFullscreen && (
                  <p className="flex items-center justify-center gap-1 text-center text-xs text-muted-foreground/60">
                    <LockKeyhole className="h-3 w-3" />
                    Encrypted and private at every step
                  </p>
                )}
              </div>
            </div>
          </div>
        ) : (
          // Fixed at bottom when there are messages
          <div className="bg-background pb-[env(safe-area-inset-bottom)]">
            <div className="mx-auto max-w-4xl px-4 landscape-short:px-3">
              <form onSubmit={handleSendMessage} className="relative">
                <div className="space-y-2 landscape-short:space-y-1">
                  {(draftImages.length > 0 || documentName) && (
                    <div className="space-y-2 landscape-short:space-y-1">
                      {draftImages.length > 0 && (
                        <div className="flex flex-wrap gap-2">
                          {draftImages.map((file, i) => (
                            <div key={i} className="group relative">
                              <img
                                src={imageUrls.get(file) || ""}
                                alt={`Attachment ${i + 1}`}
                                className="h-12 w-12 rounded-xl border object-cover"
                              />
                              <button
                                type="button"
                                onClick={() => removeImage(i)}
                                className="absolute -right-1 -top-1 rounded-full border bg-background p-0.5 opacity-0 transition-opacity group-hover:opacity-100"
                              >
                                <X className="h-2.5 w-2.5" />
                              </button>
                            </div>
                          ))}
                        </div>
                      )}

                      {documentName && (
                        <div className="flex items-center gap-2 rounded-2xl bg-muted/50 p-1.5 text-xs">
                          <FileText className="h-3 w-3 text-muted-foreground" />
                          <span className="flex-1 truncate">{documentName}</span>
                          <button
                            type="button"
                            onClick={removeDocument}
                            className="text-muted-foreground hover:text-foreground"
                          >
                            <X className="h-2.5 w-2.5" />
                          </button>
                        </div>
                      )}
                    </div>
                  )}

                  {(attachmentError || audioError) && (
                    <div className="px-2 text-xs text-maple-error">
                      {attachmentError || audioError}
                    </div>
                  )}

                  <div className="relative overflow-hidden rounded-3xl border border-[hsl(var(--maple-secondary-container))] bg-background transition-colors focus-within:border-[hsl(var(--maple-primary))]">
                    <Textarea
                      ref={textareaRef}
                      value={input}
                      onChange={(e) => setInput(e.target.value)}
                      onKeyDown={handleKeyDown}
                      placeholder="Message Maple..."
                      disabled={isGenerating || isRecording}
                      className="w-full min-h-[52px] landscape-short:min-h-[40px] max-h-[200px] landscape-short:max-h-[100px] resize-none border-0 bg-transparent py-3.5 landscape-short:py-2 pl-4 pr-2 leading-6 focus-visible:ring-0 focus-visible:ring-offset-0 placeholder:text-muted-foreground/60"
                      rows={1}
                      id="message"
                    />

                    <div className="grid grid-cols-[minmax(0,1fr)_auto] items-end gap-x-2 gap-y-2 px-2 pb-2 landscape-short:pb-1.5 pt-1">
                      <div className="flex min-w-0 flex-wrap items-center gap-1.5 sm:gap-2">
                        <ModelSelector
                          hasImages={
                            draftImages.length > 0 ||
                            messages.some(
                              (msg) =>
                                msg.type === "message" &&
                                (msg as ExtendedMessage).content?.some(
                                  (part: ConversationContent) => part.type === "input_image"
                                )
                            )
                          }
                        />

                        <Button
                          type="button"
                          variant="ghost"
                          size="sm"
                          className="h-8 w-8 p-0 text-[hsl(var(--maple-secondary-700))] hover:bg-[hsl(var(--maple-primary-container))] hover:text-[hsl(var(--maple-secondary-700))]"
                          onClick={() => {
                            const newValue = !isWebSearchEnabled;
                            setIsWebSearchEnabled(newValue);
                            localStorage.setItem("webSearchEnabled", newValue.toString());
                          }}
                          aria-label={
                            isWebSearchEnabled ? "Disable web search" : "Enable web search"
                          }
                        >
                          <Globe
                            className={`h-4 w-4 ${
                              isWebSearchEnabled
                                ? "text-[hsl(var(--maple-primary))]"
                                : "text-[hsl(var(--maple-secondary-700))]"
                            }`}
                          />
                        </Button>

                        <DropdownMenu>
                          <DropdownMenuTrigger asChild>
                            <Button
                              type="button"
                              variant="ghost"
                              size="sm"
                              className="h-8 w-8 p-0 text-[hsl(var(--maple-secondary-700))] hover:bg-[hsl(var(--maple-primary-container))] hover:text-[hsl(var(--maple-secondary-700))]"
                              disabled={isProcessingDocument}
                            >
                              <Plus className="h-4 w-4 text-[hsl(var(--maple-secondary-700))]" />
                            </Button>
                          </DropdownMenuTrigger>
                          <DropdownMenuContent align="start">
                            <DropdownMenuItem
                              onClick={() => {
                                if (!canUseImages) {
                                  setUpgradeFeature("image");
                                  setUpgradeDialogOpen(true);
                                } else {
                                  fileInputRef.current?.click();
                                }
                              }}
                            >
                              <Image className="mr-2 h-4 w-4" />
                              <span>Add Images</span>
                            </DropdownMenuItem>
                            <DropdownMenuItem
                              onClick={() => {
                                if (!isTauriEnv) {
                                  setDocumentPlatformDialogOpen(true);
                                } else if (!canUseDocuments) {
                                  setUpgradeFeature("document");
                                  setUpgradeDialogOpen(true);
                                } else {
                                  documentInputRef.current?.click();
                                }
                              }}
                            >
                              <FileText className="mr-2 h-4 w-4" />
                              <span>Add Document</span>
                            </DropdownMenuItem>
                          </DropdownMenuContent>
                        </DropdownMenu>
                      </div>

                      <div className="flex shrink-0 items-center self-end gap-1.5 sm:gap-2">
                        <Button
                          type="button"
                          onClick={startRecording}
                          disabled={isGenerating || isRecording || !canUseVoice}
                          size="icon"
                          variant="ghost"
                          className="h-8 w-8 rounded-xl text-[hsl(var(--maple-secondary-700))] hover:bg-[hsl(var(--maple-primary-container))] hover:text-[hsl(var(--maple-secondary-700))]"
                        >
                          <Mic className="h-4 w-4 text-[hsl(var(--maple-secondary-700))]" />
                        </Button>
                        {isGenerating ? (
                          <Button
                            type="button"
                            onClick={handleCancelResponse}
                            size="icon"
                            variant="destructive"
                            className="h-8 w-8 rounded-xl"
                          >
                            <div className="h-3 w-3 rounded-md bg-current" />
                          </Button>
                        ) : (
                          <button
                            type="submit"
                            disabled={!input.trim() && !draftImages.length && !documentText}
                            className="flex h-8 w-8 items-center justify-center rounded-full bg-gradient-to-b from-[hsl(var(--maple-primary))] to-[hsl(var(--maple-primary-strong))] text-[hsl(var(--maple-on-primary))]/90 transition-all duration-200 ease-out active:scale-[0.95] disabled:pointer-events-none disabled:opacity-40"
                          >
                            <ArrowUp className="h-4 w-4" />
                          </button>
                        )}
                      </div>
                    </div>

                    {isRecording && (
                      <RecordingOverlay
                        isRecording={isRecording}
                        isProcessing={isProcessingSend || isTranscribing}
                        onSend={() => stopRecording(true)}
                        onCancel={() => stopRecording(false)}
                        isCompact={true}
                        className="absolute inset-0 rounded-3xl"
                      />
                    )}
                  </div>
                </div>
              </form>
              <p className="mb-2 mt-1 landscape-short:mb-1 text-center text-[10px] text-muted-foreground/50">
                AI can make mistakes. Check important info.
              </p>
            </div>
          </div>
        )}

        {/* Upgrade dialog for attachments and usage limits */}
        <UpgradePromptDialog
          open={upgradeDialogOpen}
          onOpenChange={setUpgradeDialogOpen}
          feature={
            upgradeFeature === "document"
              ? "document"
              : upgradeFeature === "voice"
                ? "voice"
                : upgradeFeature === "usage"
                  ? "usage"
                  : upgradeFeature === "tokens"
                    ? "tokens"
                    : "image"
          }
        />

        {/* Document platform dialog for web users */}
        <DocumentPlatformDialog
          open={documentPlatformDialogOpen}
          onOpenChange={setDocumentPlatformDialogOpen}
          hasProAccess={canUseDocuments || false}
        />

        {/* Context limit dialog for 413 errors */}
        <ContextLimitDialog
          open={contextLimitDialogOpen}
          onOpenChange={setContextLimitDialogOpen}
          currentModel={localState.model}
          hasDocument={!!documentName}
        />

        {/* TTS setup dialog */}
        <TTSDownloadDialog open={ttsSetupDialogOpen} onOpenChange={setTtsSetupDialogOpen} />

        {/* Hidden file inputs - must be outside conditional rendering to work in both views */}
        <input
          type="file"
          ref={fileInputRef}
          accept="image/jpeg,image/jpg,image/png,image/webp"
          multiple
          onChange={handleAddImages}
          className="hidden"
        />
        <input
          type="file"
          ref={documentInputRef}
          accept=".pdf,.txt,.md"
          onChange={handleDocumentUpload}
          className="hidden"
        />
      </div>
    </div>
  );
}
