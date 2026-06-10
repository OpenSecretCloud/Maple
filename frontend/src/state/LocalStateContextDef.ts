import { createContext } from "react";
import { BillingStatus } from "@/billing/billingApi";
import type { Model } from "openai/resources/models.js";

// Extended Model type for OpenSecret API which includes additional properties
export type ModelAccessTier = "free" | "starter" | "pro";

export type ModelCapabilities = {
  chat?: boolean;
  vision?: boolean;
  reasoning?: boolean;
  tool_use?: boolean;
};

export interface OpenSecretModel extends Model {
  tasks?: string[];
  provider?: string;
  provider_id?: string;
  display_name?: string;
  short_name?: string;
  description?: string;
  context_window?: number;
  max_context_tokens?: number;
  access?: ModelAccessTier;
  capabilities?: ModelCapabilities;
  badges?: string[];
  enabled?: boolean;
  deprecated?: boolean;
  sort_order?: number;
}

export type OpenSecretModelAlias = {
  id: "auto:quick" | "auto:powerful";
  label: string;
  short_name: string;
  description: string;
  target_model: string;
  access?: ModelAccessTier;
  capabilities?: ModelCapabilities;
};

export type OpenSecretModelCatalog = {
  object: "list";
  data: OpenSecretModel[];
  aliases: OpenSecretModelAlias[];
  defaults?: {
    quick: "auto:quick";
    powerful: "auto:powerful";
  };
  audio?: {
    transcription?: {
      available: boolean;
      model: string;
      display_name?: string;
    };
    speech?: {
      available: boolean;
      model: string;
      display_name?: string;
    };
  };
};

export type LocalState = {
  model: string;
  availableModels: OpenSecretModel[];
  modelAliases: OpenSecretModelAlias[];
  setModel: (model: string, modelMetadata?: OpenSecretModel | null) => void;
  setAvailableModels: (models: OpenSecretModel[]) => void;
  setModelAliases: (aliases: OpenSecretModelAlias[]) => void;
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
  /** Currently selected conversation project for sidebar/composer context */
  selectedProjectId: string | null;
  /** Updates the selected conversation project context */
  setSelectedProjectId: (projectId: string | null) => void;
  setBillingStatus: (status: BillingStatus) => void;
  setUserPrompt: (prompt: string) => void;
  setSystemPrompt: (prompt: string | null) => void;
  setUserImages: (images: File[]) => void;
  setSentViaVoice: (sentViaVoice: boolean) => void;
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
  modelAliases: [],
  setModel: () => void 0,
  setAvailableModels: () => void 0,
  setModelAliases: () => void 0,
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
  selectedProjectId: null,
  setSelectedProjectId: () => void 0,
  setBillingStatus: () => void 0,
  setUserPrompt: () => void 0,
  setSystemPrompt: () => void 0,
  setUserImages: () => void 0,
  setSentViaVoice: () => void 0,
  draftMessages: new Map(),
  setDraftMessage: () => void 0,
  clearDraftMessage: () => void 0
});
