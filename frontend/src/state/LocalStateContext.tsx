import { useOpenSecret } from "@opensecret/react";
import { useCallback, useState } from "react";
import { BillingStatus } from "@/billing/billingApi";
import {
  LocalStateContext,
  Chat,
  HistoryItem,
  OpenSecretModel,
  OpenSecretModelAlias
} from "./LocalStateContextDef";
import { aliasModelName, migrateStickyModelName } from "@/utils/utils";

export {
  LocalStateContext,
  type Chat,
  type HistoryItem,
  type LocalState
} from "./LocalStateContextDef";

export const QUICK_MODEL_ALIAS = "auto:quick";
export const POWERFUL_MODEL_ALIAS = "auto:powerful";
export const DEFAULT_MODEL_ID = QUICK_MODEL_ALIAS;
export const PAID_DEFAULT_MODEL_ID = POWERFUL_MODEL_ALIAS;
const SELECTED_MODEL_METADATA_KEY = "selectedModelMetadata";

const DEFAULT_MODEL_ALIASES: OpenSecretModelAlias[] = [
  {
    id: QUICK_MODEL_ALIAS,
    label: "Quick",
    short_name: "Quick",
    description: "Fast, everyday responses",
    target_model: "",
    access: "free",
    capabilities: { chat: true, vision: false, reasoning: true, tool_use: true }
  },
  {
    id: POWERFUL_MODEL_ALIAS,
    label: "Powerful",
    short_name: "Powerful",
    description: "Deeper thinking & analysis",
    target_model: "",
    access: "pro",
    capabilities: { chat: true, vision: true, reasoning: true, tool_use: true }
  }
];

// Check if a plan name corresponds to a pro/max/team plan
function isProMaxOrTeamPlan(planName: string): boolean {
  return planName.includes("pro") || planName.includes("max") || planName.includes("team");
}

// Check if paid defaults have already been applied for this user.
// The value is an ISO date string indicating when defaults were last applied.
function hasPaidDefaultsBeenApplied(): boolean {
  return localStorage.getItem("paidDefaultsApplied") !== null;
}

// Helper to get the initial web search state from localStorage.
// Web search is on by default for all users, but respects the user's explicit preference.
export function getInitialWebSearchEnabled(): boolean {
  try {
    // If user has explicitly toggled web search before, respect that
    const webSearchSetting = localStorage.getItem("webSearchEnabled");
    if (webSearchSetting !== null) {
      return webSearchSetting === "true";
    }
  } catch (error) {
    console.error("Failed to get initial web search state:", error);
  }
  // Default to enabled for all users
  return true;
}

// Helper to get default model based on cached billing status
function getInitialModel(): string {
  // Check for dev override first
  if (import.meta.env.VITE_DEV_MODEL_OVERRIDE) {
    return aliasModelName(import.meta.env.VITE_DEV_MODEL_OVERRIDE);
  }

  try {
    // Priority 1: Check local storage for user's explicit model choice
    const selectedModel = localStorage.getItem("selectedModel");
    if (selectedModel) {
      if (getCachedSelectedModelMetadata(selectedModel)) {
        return selectedModel;
      }

      return migrateStickyModelName(selectedModel);
    }

    // Priority 2: Check if paid defaults were already applied
    // (user is returning paid user who got the one-time flip but then
    // cleared selectedModel somehow — unlikely but safe fallback)
    if (hasPaidDefaultsBeenApplied()) {
      return PAID_DEFAULT_MODEL_ID;
    }

    // Priority 3: Check cached billing status for pro/max/team users
    const cachedBillingStr = localStorage.getItem("cachedBillingStatus");
    if (cachedBillingStr) {
      const cachedBilling = JSON.parse(cachedBillingStr) as BillingStatus;
      const planName = cachedBilling.product_name?.toLowerCase() || "";

      // Pro, Max, or Team users get the powerful reasoning model
      if (isProMaxOrTeamPlan(planName)) {
        return PAID_DEFAULT_MODEL_ID;
      }
    }
  } catch (error) {
    console.error("Failed to load initial model:", error);
  }

  // Priority 4: Default to free model
  return DEFAULT_MODEL_ID;
}

function normalizeAvailableModels(models: OpenSecretModel[]): OpenSecretModel[] {
  const normalizedModels = new Map<string, OpenSecretModel>();

  for (const model of models) {
    if (!normalizedModels.has(model.id)) {
      normalizedModels.set(model.id, model);
    }
  }

  return Array.from(normalizedModels.values());
}

function isAutoModelAlias(modelId: string): boolean {
  return modelId === QUICK_MODEL_ALIAS || modelId === POWERFUL_MODEL_ALIAS;
}

function getCachedSelectedModelMetadata(modelId: string): OpenSecretModel | null {
  if (!modelId || isAutoModelAlias(modelId)) return null;

  try {
    const cachedMetadata = localStorage.getItem(SELECTED_MODEL_METADATA_KEY);
    if (!cachedMetadata) return null;

    const parsedMetadata = JSON.parse(cachedMetadata) as OpenSecretModel;
    if (parsedMetadata.id !== modelId) return null;

    return {
      ...parsedMetadata,
      object: "model",
      created: parsedMetadata.created || Date.now(),
      owned_by: parsedMetadata.owned_by || "opensecret"
    };
  } catch (error) {
    console.error("Failed to load selected model metadata:", error);
    return null;
  }
}

function cacheSelectedModelMetadata(modelId: string, modelMetadata?: OpenSecretModel | null) {
  try {
    if (!modelMetadata || isAutoModelAlias(modelId)) {
      localStorage.removeItem(SELECTED_MODEL_METADATA_KEY);
      return;
    }

    const cacheableMetadata: OpenSecretModel = {
      ...modelMetadata,
      id: modelId,
      object: "model",
      created: modelMetadata.created || Date.now(),
      owned_by: modelMetadata.owned_by || "opensecret"
    };

    localStorage.setItem(SELECTED_MODEL_METADATA_KEY, JSON.stringify(cacheableMetadata));
  } catch (error) {
    console.error("Failed to cache selected model metadata:", error);
  }
}

export const LocalStateProvider = ({ children }: { children: React.ReactNode }) => {
  const initialModel = getInitialModel();
  const cachedSelectedModel = getCachedSelectedModelMetadata(initialModel);

  const [localState, setLocalState] = useState({
    userPrompt: "",
    systemPrompt: null as string | null,
    userImages: [] as File[],
    sentViaVoice: false,
    model: initialModel,
    availableModels: cachedSelectedModel ? [cachedSelectedModel] : ([] as OpenSecretModel[]),
    modelAliases: DEFAULT_MODEL_ALIASES,
    hasWhisperModel: true, // Default to true to avoid hiding button during loading
    billingStatus: null as BillingStatus | null,
    searchQuery: "",
    isSearchVisible: false,
    selectedProjectId: null as string | null,
    draftMessages: new Map<string, string>()
  });

  const { get, put, list, del, delAll } = useOpenSecret();

  async function persistChat(chat: Chat) {
    const chatToSave = {
      ...chat,

      /** If a model is missing, assume the default model and write it now */
      model: aliasModelName(chat.model) || DEFAULT_MODEL_ID
    };

    console.log("Persisting chat:", chatToSave);
    try {
      // Save the chat to storage
      await put(`chat_${chat.id}`, JSON.stringify(chatToSave));

      // Now we need to update the history_list
      const historyList = await fetchOrCreateHistoryList();

      // If the item already exists, update it, otherwise add it
      if (historyList.some((item) => item.id === chat.id)) {
        const updatedHistory = historyList.map((item) => {
          if (item.id === chat.id) {
            return {
              id: chat.id,
              title: chatToSave.title,
              updated_at: Date.now(),
              created_at: item.created_at
            };
          } else {
            return item;
          }
        });
        await put("history_list", JSON.stringify(updatedHistory));
      } else {
        const updatedHistory = [
          {
            id: chat.id,
            title: chat.title,
            updated_at: Date.now(),
            created_at: Date.now()
          },
          ...historyList
        ];
        await put("history_list", JSON.stringify(updatedHistory));
      }
    } catch (error) {
      console.error("Failed to persist chat:", error);
    }
  }

  function setUserPrompt(prompt: string) {
    setLocalState((prev) => ({ ...prev, userPrompt: prompt }));
  }

  function setSystemPrompt(prompt: string | null) {
    setLocalState((prev) => ({ ...prev, systemPrompt: prompt }));
  }

  function setUserImages(images: File[]) {
    setLocalState((prev) => ({ ...prev, userImages: images }));
  }

  function setSentViaVoice(sentViaVoice: boolean) {
    setLocalState((prev) => ({ ...prev, sentViaVoice }));
  }

  function setBillingStatus(status: BillingStatus) {
    setLocalState((prev) => ({ ...prev, billingStatus: status }));

    const planName = status.product_name?.toLowerCase() || "";
    const isPaidPlan =
      planName.includes("pro") ||
      planName.includes("max") ||
      planName.includes("team") ||
      planName.includes("starter");

    const isProMaxOrTeam = isProMaxOrTeamPlan(planName);

    // Check if billing plan changed from cached version
    let billingChanged = false;
    try {
      const cachedBillingStr = localStorage.getItem("cachedBillingStatus");
      if (cachedBillingStr) {
        const cachedBilling = JSON.parse(cachedBillingStr) as BillingStatus;
        const cachedPlan = cachedBilling.product_name?.toLowerCase() || "";
        billingChanged = cachedPlan !== planName;
      }
    } catch (error) {
      console.error("Failed to check cached billing:", error);
    }

    // Cache billing status to localStorage only for paid users
    try {
      if (isPaidPlan) {
        localStorage.setItem("cachedBillingStatus", JSON.stringify(status));
      } else {
        // Clear cache for free users
        localStorage.removeItem("cachedBillingStatus");
      }
    } catch (error) {
      console.error("Failed to cache billing status:", error);
    }

    // One-time paid defaults: when a user is on pro/max/team and we haven't
    // applied paid defaults yet, flip model to "Powerful" and web search ON.
    // This handles both new signups and free-to-paid upgrades.
    try {
      if (isProMaxOrTeam && !hasPaidDefaultsBeenApplied()) {
        // Apply paid defaults — set model to Powerful reasoning model
        setModelInternal(PAID_DEFAULT_MODEL_ID, true);

        // Mark when we applied paid defaults (ISO date) so we never override again.
        // Future defaults can check this date to decide whether to re-apply newer defaults.
        localStorage.setItem("paidDefaultsApplied", new Date().toISOString());

        return;
      }
    } catch (error) {
      console.error("Failed to apply paid defaults:", error);
    }

    // For users who already had defaults applied: handle plan changes
    try {
      if (billingChanged) {
        if (isProMaxOrTeam) {
          // Plan changed but still pro-tier — only update model if user
          // hasn't manually chosen one (selectedModel not in localStorage)
          const selectedModel = localStorage.getItem("selectedModel");
          if (!selectedModel) {
            setModelInternal(PAID_DEFAULT_MODEL_ID, true);
          }
        } else {
          // User downgraded to free/starter — switch back to free model
          // and clear paid defaults so they get re-applied if they upgrade again
          setModelInternal(DEFAULT_MODEL_ID);
          localStorage.removeItem("paidDefaultsApplied");
          localStorage.removeItem("selectedModel");
        }
      }
    } catch (error) {
      console.error("Failed to update model based on billing status:", error);
    }
  }

  function setSearchQuery(query: string) {
    setLocalState((prev) => ({ ...prev, searchQuery: query }));
  }

  function setIsSearchVisible(visible: boolean) {
    setLocalState((prev) => ({ ...prev, isSearchVisible: visible }));
  }

  const setSelectedProjectId = useCallback((projectId: string | null) => {
    setLocalState((prev) => ({ ...prev, selectedProjectId: projectId }));
  }, []);

  async function addChat(title: string = "New Chat") {
    const newChat = {
      id: window.crypto.randomUUID(),
      title,
      messages: [],
      model: localState.model
    };
    await persistChat(newChat);
    return newChat.id;
  }

  async function getChatById(id: string): Promise<Chat | undefined> {
    try {
      const chat = await get(`chat_${id}`);
      if (!chat) return undefined;
      const parsedChat = JSON.parse(chat) as Chat;
      // Alias the model name for backward compatibility
      if (parsedChat.model) {
        parsedChat.model = aliasModelName(parsedChat.model);
      }
      return parsedChat;
    } catch (error) {
      console.error("Error fetching chat:", error);
      return undefined;
    }
  }

  async function fetchOrCreateHistoryList() {
    let historyList = "[]";
    try {
      const existingHistory = await get("history_list");
      if (existingHistory) {
        historyList = existingHistory;
      }
    } catch (error) {
      console.error("Error fetching history_list:", error);
    }

    // Parse the history_list item
    let parsedHistory: HistoryItem[];
    try {
      parsedHistory = JSON.parse(historyList) as HistoryItem[];
      if (!Array.isArray(parsedHistory)) {
        throw new Error("Parsed history is not an array");
      }
    } catch (error) {
      console.error("Error parsing history_list:", error);
      console.log("Raw history_list content:", historyList);
      parsedHistory = [];
    }

    // TODO REMOVE: this fallback is because we didn't always have a history_list
    if (parsedHistory.length === 0) {
      try {
        const allKeys = await list();
        const chatKeys = allKeys.filter((item) => item.key.startsWith("chat_"));

        const newHistoryList = await Promise.all(
          chatKeys.map(async (item) => {
            const chat = JSON.parse(item.value) as Chat;
            return {
              id: chat.id,
              title: chat.title,
              updated_at: item.updated_at,
              created_at: item.created_at
            };
          })
        );

        // Sort by updated_at timestamp
        newHistoryList.sort((a, b) => b.updated_at - a.updated_at);

        await put("history_list", JSON.stringify(newHistoryList));
        return newHistoryList;
      } catch (error) {
        console.error("Error creating new history list:", error);
        return [];
      }
    }

    return parsedHistory;
  }

  async function clearHistory() {
    try {
      await delAll();
    } catch (error) {
      console.error("Failed to clear history:", error);
      // Fallback to manual deletion if bulk delete fails (e.g. old SDK version)
      const items = await list();
      await del("history_list");
      await Promise.all(
        items.filter((item) => item.key.startsWith("chat_")).map(async (chat) => del(chat.key))
      );
    }
  }

  async function deleteChat(chatId: string) {
    // Delete the chat contents
    await del(`chat_${chatId}`);
    // Update the chat history
    const chatHistory = await fetchOrCreateHistoryList();
    const updatedChatHistory = chatHistory.filter((item) => item.id !== chatId);
    await put("history_list", JSON.stringify(updatedChatHistory));
  }

  async function renameChat(chatId: string, newTitle: string) {
    try {
      // Get the current chat
      const chat = await getChatById(chatId);
      if (!chat) {
        console.error("Chat not found for renaming:", chatId);
        throw new Error("Chat not found");
      }

      // Update the chat title
      chat.title = newTitle;

      // Save the updated chat
      await persistChat(chat);

      // The persistChat function already updates the history list
      return;
    } catch (error) {
      console.error("Error renaming chat:", error);
      throw new Error("Error renaming chat");
    }
  }

  function setDraftMessage(chatId: string, draft: string) {
    if (!chatId?.trim()) {
      console.error("Invalid chatId provided to setDraftMessage");
      return;
    }
    setLocalState((prev) => ({
      ...prev,
      draftMessages: new Map(prev.draftMessages).set(chatId, draft)
    }));
  }

  function clearDraftMessage(chatId: string) {
    if (!chatId?.trim()) {
      console.error("Invalid chatId provided to clearDraftMessage");
      return;
    }
    setLocalState((prev) => {
      const newDrafts = new Map(prev.draftMessages);
      if (!newDrafts.has(chatId)) {
        return prev; // No state update needed if draft doesn't exist
      }
      newDrafts.delete(chatId);
      return { ...prev, draftMessages: newDrafts };
    });
  }

  // Internal model setter — updates state and localStorage but does NOT mark as
  // a user's explicit choice. Used by billing/system logic.
  function setModelInternal(modelId: string, persist = false) {
    const aliasedModel = aliasModelName(modelId);
    setLocalState((prev) => {
      if (prev.model === aliasedModel) return prev;
      return { ...prev, model: aliasedModel };
    });
    if (persist) {
      try {
        localStorage.setItem("selectedModel", aliasedModel);
        cacheSelectedModelMetadata(aliasedModel);
      } catch (error) {
        console.error("Failed to save model to localStorage:", error);
      }
    }
  }

  // Public model setter — records the choice as a user-initiated selection.
  // After this, we won't auto-override their model choice.
  function setModel(model: string, modelMetadata?: OpenSecretModel | null) {
    const nextModel = modelMetadata ? model : aliasModelName(model);
    setLocalState((prev) => {
      if (prev.model === nextModel && !modelMetadata) return prev;

      // Save to localStorage as user's explicit choice
      try {
        localStorage.setItem("selectedModel", nextModel);
        cacheSelectedModelMetadata(nextModel, modelMetadata);
      } catch (error) {
        console.error("Failed to save model to localStorage:", error);
      }

      const availableModels =
        modelMetadata && !isAutoModelAlias(nextModel)
          ? normalizeAvailableModels([modelMetadata, ...prev.availableModels])
          : prev.availableModels;

      return { ...prev, model: nextModel, availableModels };
    });
  }

  function setAvailableModels(models: OpenSecretModel[]) {
    setLocalState((prev) => ({
      ...prev,
      availableModels: normalizeAvailableModels(models)
    }));
  }

  function setModelAliases(aliases: OpenSecretModelAlias[]) {
    setLocalState((prev) => ({
      ...prev,
      modelAliases: aliases.length > 0 ? aliases : DEFAULT_MODEL_ALIASES
    }));
  }

  function setHasWhisperModel(hasWhisper: boolean) {
    setLocalState((prev) => ({ ...prev, hasWhisperModel: hasWhisper }));
  }

  return (
    <LocalStateContext.Provider
      value={{
        model: localState.model,
        availableModels: localState.availableModels,
        modelAliases: localState.modelAliases,
        setModel,
        setAvailableModels,
        setModelAliases,
        hasWhisperModel: localState.hasWhisperModel,
        setHasWhisperModel,
        userPrompt: localState.userPrompt,
        systemPrompt: localState.systemPrompt,
        userImages: localState.userImages,
        sentViaVoice: localState.sentViaVoice,
        billingStatus: localState.billingStatus,
        searchQuery: localState.searchQuery,
        setSearchQuery,
        isSearchVisible: localState.isSearchVisible,
        setIsSearchVisible,
        selectedProjectId: localState.selectedProjectId,
        setSelectedProjectId,
        setBillingStatus,
        setUserPrompt,
        setSystemPrompt,
        setUserImages,
        setSentViaVoice,
        addChat,
        getChatById,
        persistChat,
        fetchOrCreateHistoryList,
        clearHistory,
        deleteChat,
        renameChat,
        draftMessages: localState.draftMessages,
        setDraftMessage,
        clearDraftMessage
      }}
    >
      {children}
    </LocalStateContext.Provider>
  );
};
