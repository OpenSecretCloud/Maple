import { createContext } from "react";
import { BillingStatus } from "@/billing/billingApi";
import type { Model } from "openai/resources/models.js";

// Extended Model type for OpenSecret API which includes additional properties
export interface OpenSecretModel extends Model {
  tasks?: string[];
}

export type ChatContentPart =
  | { type: "text"; text: string }
  | { type: "image_url"; image_url: { url: string } };

export type ChatMessage = {
  role: "user" | "assistant" | "system";
  /** plain text for normal models, or multimodal array for multimodal models */
  content: string | ChatContentPart[];
  /** Optional document attachment for user messages */
  document?: {
    filename: string;
    fullContent: string;
  };
};

export type Chat = {
  id: string;
  title: string;
  messages: ChatMessage[];
  model?: string;
};

export type HistoryItem = {
  id: string;
  title: string;
  updated_at: number;
  created_at: number;
};

export type LocalState = {
  model: string;
  availableModels: OpenSecretModel[];
  setModel: (model: string) => void;
  setAvailableModels: (models: OpenSecretModel[]) => void;
  /** Whether the whisper transcription model is available */
  hasWhisperModel: boolean;
  setHasWhisperModel: (hasWhisper: boolean) => void;
  userPrompt: string;
  systemPrompt: string | null;
  userImages: File[];
  sentViaVoice: boolean;
  billingStatus: BillingStatus | null;
  /** Current search query for filtering chat history */
  searchQuery: string;
  /** Updates the current search query */
  setSearchQuery: (query: string) => void;
  /** Whether the search input is currently visible */
  isSearchVisible: boolean;
  /** Controls the visibility of the search input */
  setIsSearchVisible: (visible: boolean) => void;
  setBillingStatus: (status: BillingStatus) => void;
  setUserPrompt: (prompt: string) => void;
  setSystemPrompt: (prompt: string | null) => void;
  setUserImages: (images: File[]) => void;
  setSentViaVoice: (sentViaVoice: boolean) => void;
  addChat: (title?: string) => Promise<string>;
  getChatById: (id: string) => Promise<Chat | undefined>;
  persistChat: (chat: Chat) => Promise<void>;
  fetchOrCreateHistoryList: () => Promise<HistoryItem[]>;
  clearHistory: () => Promise<void>;
  deleteChat: (chatId: string) => Promise<void>;
  renameChat: (chatId: string, newTitle: string) => Promise<void>;
  /** Map of chat IDs to their draft messages */
  draftMessages: Map<string, string>;
  /** Sets a draft message for a specific chat */
  setDraftMessage: (chatId: string, draft: string) => void;
  /** Clears the draft message for a specific chat */
  clearDraftMessage: (chatId: string) => void;
};

export const LocalStateContext = createContext<LocalState>({
  model: "",
  availableModels: [],
  setModel: () => void 0,
  setAvailableModels: () => void 0,
  hasWhisperModel: true,
  setHasWhisperModel: () => void 0,
  userPrompt: "",
  systemPrompt: null,
  userImages: [],
  sentViaVoice: false,
  billingStatus: null,
  searchQuery: "",
  setSearchQuery: () => void 0,
  isSearchVisible: false,
  setIsSearchVisible: () => void 0,
  setBillingStatus: () => void 0,
  setUserPrompt: () => void 0,
  setSystemPrompt: () => void 0,
  setUserImages: () => void 0,
  setSentViaVoice: () => void 0,
  addChat: async () => "",
  getChatById: async () => undefined,
  persistChat: async () => void 0,
  fetchOrCreateHistoryList: async () => [],
  clearHistory: async () => void 0,
  deleteChat: async () => void 0,
  renameChat: async () => void 0,
  draftMessages: new Map(),
  setDraftMessage: () => void 0,
  clearDraftMessage: () => void 0
});
