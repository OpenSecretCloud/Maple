import { useOpenSecret } from "@opensecret/react";
import { useState, useEffect } from "react";
import { BillingStatus } from "@/billing/billingApi";
import { LocalStateContext, Chat, HistoryItem, OpenSecretModel } from "./LocalStateContextDef";
import { aliasModelName } from "@/utils/utils";
import { getDefaultModelForUser, hasAccessToModel } from "@/utils/modelDefaults";
import { MODEL_CONFIG } from "@/utils/modelConfig";

export {
  LocalStateContext,
  type Chat,
  type ChatMessage,
  type HistoryItem,
  type LocalState
} from "./LocalStateContextDef";

export const DEFAULT_MODEL_ID = "llama3-3-70b";

export const LocalStateProvider = ({ children }: { children: React.ReactNode }) => {
  /** The model that should be assumed when a chat doesn't yet have one */
  const llamaModel: OpenSecretModel = {
    id: DEFAULT_MODEL_ID,
    object: "model",
    created: Date.now(),
    owned_by: "meta",
    tasks: ["generate"]
  };

  const [localState, setLocalState] = useState({
    userPrompt: "",
    systemPrompt: null as string | null,
    userImages: [] as File[],
    model: aliasModelName(import.meta.env.VITE_DEV_MODEL_OVERRIDE) || DEFAULT_MODEL_ID,
    availableModels: [llamaModel] as OpenSecretModel[],
    billingStatus: null as BillingStatus | null,
    searchQuery: "",
    isSearchVisible: false,
    draftMessages: new Map<string, string>(),
    previousPlanTier: null as "free" | "starter" | "pro" | null
  });

  const { get, put, list, del } = useOpenSecret();

  // Helper function to extract plan tier from billing status
  function getPlanTier(billingStatus: BillingStatus | null): "free" | "starter" | "pro" {
    if (!billingStatus) return "free";

    const planName = billingStatus.product_name?.toLowerCase() || "";

    if (planName.includes("pro") || planName.includes("max") || planName.includes("team")) {
      return "pro";
    }

    if (planName.includes("starter")) {
      return "starter";
    }

    return "free";
  }

  // Load last used model when component mounts
  useEffect(() => {
    async function loadLastUsedModel() {
      try {
        const [lastUsedModel, storedPlanTier] = await Promise.all([
          get("last_used_model"),
          get("previous_plan_tier")
        ]);

        if (lastUsedModel) {
          const aliasedModel = aliasModelName(lastUsedModel);
          // Validate stored model before using it
          if (
            MODEL_CONFIG[aliasedModel] &&
            hasAccessToModel(aliasedModel, localState.billingStatus)
          ) {
            setLocalState((prev) => ({
              ...prev,
              model: aliasedModel,
              previousPlanTier: storedPlanTier || null
            }));
          }
        } else if (storedPlanTier) {
          setLocalState((prev) => ({
            ...prev,
            previousPlanTier: storedPlanTier
          }));
        }
      } catch (error) {
        console.error("Error loading last used model:", error);
      }
    }

    loadLastUsedModel();
  }, [get, localState.billingStatus]); // Add billingStatus as dependency for validation

  // Update default model when billing status changes
  useEffect(() => {
    async function updateDefaultModel() {
      if (localState.billingStatus) {
        try {
          const lastUsedModel = await get("last_used_model");
          const currentPlanTier = getPlanTier(localState.billingStatus);

          // Store current plan tier for future upgrade detection
          if (currentPlanTier !== localState.previousPlanTier) {
            await put("previous_plan_tier", currentPlanTier);
          }

          const defaultModel = getDefaultModelForUser(
            localState.billingStatus,
            lastUsedModel || null,
            localState.previousPlanTier || undefined
          );

          // Update model if:
          // 1. Current model is the initial default, OR
          // 2. No last used model, OR
          // 3. User upgraded from Free to Pro (override Llama with GPT-OSS)
          const shouldUpdate =
            localState.model === DEFAULT_MODEL_ID ||
            !lastUsedModel ||
            (localState.previousPlanTier === "free" && currentPlanTier === "pro");

          if (shouldUpdate) {
            const aliasedModel = aliasModelName(defaultModel);
            setLocalState((prev) => ({
              ...prev,
              model: aliasedModel,
              previousPlanTier: currentPlanTier
            }));
          } else {
            // Just update the plan tier tracking
            setLocalState((prev) => ({
              ...prev,
              previousPlanTier: currentPlanTier
            }));
          }
        } catch (error) {
          console.error("Error updating default model:", error);
        }
      }
    }

    updateDefaultModel();
  }, [localState.billingStatus, get, put, localState.previousPlanTier]);

  async function persistChat(chat: Chat) {
    const chatToSave = {
      /** If a model is missing, assume the default Llama and write it now */
      model: aliasModelName(chat.model) || DEFAULT_MODEL_ID,
      ...chat
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

  function setBillingStatus(status: BillingStatus) {
    setLocalState((prev) => ({ ...prev, billingStatus: status }));
  }

  function setSearchQuery(query: string) {
    setLocalState((prev) => ({ ...prev, searchQuery: query }));
  }

  function setIsSearchVisible(visible: boolean) {
    setLocalState((prev) => ({ ...prev, isSearchVisible: visible }));
  }

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
    const items = await list();
    await del("history_list");
    await Promise.all(
      items.filter((item) => item.key.startsWith("chat_")).map(async (chat) => del(chat.key))
    );
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

  async function setModel(model: string) {
    const aliasedModel = aliasModelName(model);
    setLocalState((prev) =>
      prev.model === aliasedModel ? prev : { ...prev, model: aliasedModel }
    );

    // Persist the last used model
    await put("last_used_model", aliasedModel).catch((err) => {
      console.error("Error saving last used model:", err);
      throw err; // Propagate error to caller
    });
  }

  function setAvailableModels(models: OpenSecretModel[]) {
    setLocalState((prev) => ({ ...prev, availableModels: models }));
  }

  return (
    <LocalStateContext.Provider
      value={{
        model: localState.model,
        availableModels: localState.availableModels,
        setModel,
        setAvailableModels,
        userPrompt: localState.userPrompt,
        systemPrompt: localState.systemPrompt,
        userImages: localState.userImages,
        billingStatus: localState.billingStatus,
        searchQuery: localState.searchQuery,
        setSearchQuery,
        isSearchVisible: localState.isSearchVisible,
        setIsSearchVisible,
        setBillingStatus,
        setUserPrompt,
        setSystemPrompt,
        setUserImages,
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
