import { createContext } from "react";
import { BillingStatus } from "@/billing/billingApi";

export type ChatMessage = {
  role: "user" | "assistant" | "system";
  content: string;
};

export type Project = {
  id: string;
  name: string;
  description?: string;
  systemPrompt?: string;
  created_at: number;
  updated_at: number;
};

export type Chat = {
  id: string;
  title: string;
  messages: ChatMessage[];
  projectId?: string;
};

export type HistoryItem = {
  id: string;
  title: string;
  updated_at: number;
  created_at: number;
  projectId?: string;
};

export type LocalState = {
  model: string;
  userPrompt: string;
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
  addChat: (title?: string, projectId?: string) => Promise<string>;
  getChatById: (id: string) => Promise<Chat | undefined>;
  persistChat: (chat: Chat) => Promise<void>;
  fetchOrCreateHistoryList: () => Promise<HistoryItem[]>;
  clearHistory: () => Promise<void>;
  deleteChat: (chatId: string) => Promise<void>;
  renameChat: (chatId: string, newTitle: string) => Promise<void>;
  /** Project-related functions */
  getProjects: () => Promise<Project[]>;
  getProjectById: (projectId: string) => Promise<Project | undefined>;
  createProject: (name: string, description?: string, systemPrompt?: string) => Promise<Project>;
  updateProject: (project: Project) => Promise<void>;
  deleteProject: (projectId: string) => Promise<void>;
  addChatToProject: (chatId: string, projectId: string) => Promise<void>;
  removeChatFromProject: (chatId: string) => Promise<void>;
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
  getProjects: async () => [],
  getProjectById: async () => undefined,
  createProject: async () => ({ id: "", name: "", created_at: 0, updated_at: 0 }),
  updateProject: async () => void 0,
  deleteProject: async () => void 0,
  addChatToProject: async () => void 0,
  removeChatFromProject: async () => void 0,
  draftMessages: new Map(),
  setDraftMessage: () => void 0,
  clearDraftMessage: () => void 0
});
