import { createContext } from "react";
import { BillingStatus } from "@/billing/billingApi";

export type ChatMessage = {
  role: "user" | "assistant";
  content: string;
};

export type Chat = {
  id: string;
  title: string;
  messages: ChatMessage[];
};

export type HistoryItem = {
  id: string;
  title: string;
  updated_at: number;
  created_at: number;
};

export type LocalState = {
  model: string;
  userPrompt: string;
  billingStatus: BillingStatus | null;
  searchQuery: string;
  setSearchQuery: (query: string) => void;
  isSearchVisible: boolean;
  setIsSearchVisible: (visible: boolean) => void;
  setBillingStatus: (status: BillingStatus) => void;
  setUserPrompt: (prompt: string) => void;
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
  userPrompt: "",
  billingStatus: null,
  searchQuery: "",
  setSearchQuery: () => void 0,
  isSearchVisible: false,
  setIsSearchVisible: () => void 0,
  setBillingStatus: () => void 0,
  setUserPrompt: () => void 0,
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
