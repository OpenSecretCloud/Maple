import { useOpenSecret } from "@opensecret/react";
import { useState } from "react";
import { BillingStatus } from "@/billing/billingApi";
import { LocalStateContext, Chat, HistoryItem, Project } from "./LocalStateContextDef";

export {
  LocalStateContext,
  type Chat,
  type ChatMessage,
  type HistoryItem,
  type Project,
  type LocalState
} from "./LocalStateContextDef";

export const LocalStateProvider = ({ children }: { children: React.ReactNode }) => {
  const [localState, setLocalState] = useState({
    userPrompt: "",
    model:
      import.meta.env.VITE_DEV_MODEL_OVERRIDE || "ibnzterrell/Meta-Llama-3.3-70B-Instruct-AWQ-INT4",
    billingStatus: null as BillingStatus | null,
    searchQuery: "",
    isSearchVisible: false,
    draftMessages: new Map<string, string>()
  });

  const { get, put, list, del } = useOpenSecret();

  async function persistChat(chat: Chat) {
    console.log("Persisting chat:", chat);
    try {
      // Save the chat to storage
      await put(`chat_${chat.id}`, JSON.stringify(chat));

      // Now we need to update the history_list
      const historyList = await fetchOrCreateHistoryList();

      // If the item already exists, update it, otherwise add it
      if (historyList.some((item) => item.id === chat.id)) {
        const updatedHistory = historyList.map((item) => {
          if (item.id === chat.id) {
            return {
              id: chat.id,
              title: chat.title,
              updated_at: Date.now(),
              created_at: Date.now(),
              projectId: chat.projectId
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
            created_at: Date.now(),
            projectId: chat.projectId
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

  function setBillingStatus(status: BillingStatus) {
    setLocalState((prev) => ({ ...prev, billingStatus: status }));
  }

  function setSearchQuery(query: string) {
    setLocalState((prev) => ({ ...prev, searchQuery: query }));
  }

  function setIsSearchVisible(visible: boolean) {
    setLocalState((prev) => ({ ...prev, isSearchVisible: visible }));
  }

  async function addChat(title: string = "New Chat", projectId?: string) {
    const newChat = {
      id: window.crypto.randomUUID(),
      title,
      messages: [],
      projectId
    };
    await persistChat(newChat);
    return newChat.id;
  }

  async function getChatById(id: string) {
    try {
      const chat = await get(`chat_${id}`);
      if (!chat) throw new Error("Chat not found");
      return JSON.parse(chat) as Chat;
    } catch (error) {
      console.error("Error fetching chat:", error);
      throw new Error("Error fetching chat.");
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
              created_at: item.created_at,
              projectId: chat.projectId
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
      // Get the current chat (getChatById already throws if chat not found)
      const chat = await getChatById(chatId);

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

  // Project-related functions
  async function getProjects(): Promise<Project[]> {
    try {
      const projectsStr = await get("projects");
      if (!projectsStr) return [];
      return JSON.parse(projectsStr) as Project[];
    } catch (error) {
      console.error("Error fetching projects:", error);
      return [];
    }
  }

  async function getProjectById(projectId: string): Promise<Project | undefined> {
    try {
      const projects = await getProjects();
      return projects.find((p) => p.id === projectId);
    } catch (error) {
      console.error("Error fetching project:", error);
      return undefined;
    }
  }

  async function createProject(
    name: string,
    description?: string,
    systemPrompt?: string
  ): Promise<Project> {
    try {
      const projects = await getProjects();
      const newProject: Project = {
        id: window.crypto.randomUUID(),
        name,
        description,
        systemPrompt,
        created_at: Date.now(),
        updated_at: Date.now()
      };
      await put("projects", JSON.stringify([...projects, newProject]));
      return newProject;
    } catch (error) {
      console.error("Error creating project:", error);
      throw new Error("Error creating project");
    }
  }

  async function updateProject(project: Project): Promise<void> {
    try {
      const projects = await getProjects();
      const updatedProjects = projects.map((p) =>
        p.id === project.id ? { ...project, updated_at: Date.now() } : p
      );
      await put("projects", JSON.stringify(updatedProjects));
    } catch (error) {
      console.error("Error updating project:", error);
      throw new Error("Error updating project");
    }
  }

  async function deleteProject(projectId: string): Promise<void> {
    try {
      const projects = await getProjects();
      const updatedProjects = projects.filter((p) => p.id !== projectId);
      await put("projects", JSON.stringify(updatedProjects));

      // Remove project reference from all chats in this project
      const historyList = await fetchOrCreateHistoryList();
      const updatedHistory = historyList.map((item) => {
        if (item.projectId === projectId) {
          return { ...item, projectId: undefined };
        }
        return item;
      });
      await put("history_list", JSON.stringify(updatedHistory));

      // Update all chat objects that were in this project
      for (const item of historyList) {
        if (item.projectId === projectId) {
          try {
            const chat = await getChatById(item.id);
            if (chat) {
              await persistChat({ ...chat, projectId: undefined });
            }
          } catch (error) {
            console.error(`Error updating chat ${item.id}:`, error);
          }
        }
      }
    } catch (error) {
      console.error("Error deleting project:", error);
      throw new Error("Error deleting project");
    }
  }

  async function addChatToProject(chatId: string, projectId: string): Promise<void> {
    try {
      const chat = await getChatById(chatId);
      if (!chat) throw new Error("Chat not found");

      const projects = await getProjects();
      if (!projects.some((p) => p.id === projectId)) {
        throw new Error("Project not found");
      }

      await persistChat({ ...chat, projectId });
    } catch (error) {
      console.error("Error adding chat to project:", error);
      throw new Error("Error adding chat to project");
    }
  }

  async function removeChatFromProject(chatId: string): Promise<void> {
    try {
      const chat = await getChatById(chatId);
      if (!chat) throw new Error("Chat not found");
      await persistChat({ ...chat, projectId: undefined });
    } catch (error) {
      console.error("Error removing chat from project:", error);
      throw new Error("Error removing chat from project");
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

  return (
    <LocalStateContext.Provider
      value={{
        model: localState.model,
        userPrompt: localState.userPrompt,
        billingStatus: localState.billingStatus,
        searchQuery: localState.searchQuery,
        setSearchQuery,
        isSearchVisible: localState.isSearchVisible,
        setIsSearchVisible,
        setBillingStatus,
        setUserPrompt,
        addChat,
        getChatById,
        persistChat,
        fetchOrCreateHistoryList,
        clearHistory,
        deleteChat,
        renameChat,
        // Project functions
        getProjects,
        getProjectById,
        createProject,
        updateProject,
        deleteProject,
        addChatToProject,
        removeChatFromProject,
        // Draft messages
        draftMessages: localState.draftMessages,
        setDraftMessage,
        clearDraftMessage
      }}
    >
      {children}
    </LocalStateContext.Provider>
  );
};
