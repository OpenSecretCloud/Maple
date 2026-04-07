import { createContext, useState, useEffect, useCallback, type ReactNode } from "react";

// --- Types ---

export interface ProjectFile {
  id: string;
  name: string;
  type: string; // e.g. "pdf", "txt", "md"
  size: number;
  addedAt: number;
}

export interface Project {
  id: string;
  name: string;
  createdAt: number;
  updatedAt: number;
  customInstructions: string;
  files: ProjectFile[];
  chatIds: string[];
}

export interface ProjectsContextType {
  projects: Project[];

  // CRUD
  createProject: (name: string) => string;
  renameProject: (projectId: string, newName: string) => void;
  deleteProject: (projectId: string) => void;
  updateCustomInstructions: (projectId: string, instructions: string) => void;

  // Files
  addFile: (projectId: string, file: { name: string; type: string; size: number }) => void;
  removeFile: (projectId: string, fileId: string) => void;

  // Chat assignment
  assignChatToProject: (chatId: string, projectId: string) => void;
  removeChatFromProject: (chatId: string) => void;
  moveChatToProject: (chatId: string, targetProjectId: string) => void;
  getProjectForChat: (chatId: string) => Project | undefined;
  getAllAssignedChatIds: () => Set<string>;

  // Utility
  getProjectById: (projectId: string) => Project | undefined;
  getDeletedChatIds: () => Set<string>;
}

// --- Context ---

export const ProjectsContext = createContext<ProjectsContextType | null>(null);

// --- Helpers ---

const STORAGE_KEY = "maple_projects";
const DELETED_CHATS_KEY = "maple_deleted_chat_ids";

function generateId(): string {
  return crypto.randomUUID();
}

function loadProjects(): Project[] {
  try {
    const stored = localStorage.getItem(STORAGE_KEY);
    return stored ? JSON.parse(stored) : [];
  } catch {
    return [];
  }
}

function saveProjects(projects: Project[]) {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(projects));
}

// --- Provider ---

export function ProjectsProvider({ children }: { children: ReactNode }) {
  const [projects, setProjects] = useState<Project[]>(loadProjects);

  // Persist on every change
  useEffect(() => {
    saveProjects(projects);
  }, [projects]);

  // Listen for assignchattoproject custom events (fired by UnifiedChat)
  useEffect(() => {
    const handler = (e: Event) => {
      const { chatId, projectId } = (e as CustomEvent).detail;
      if (chatId && projectId) {
        setProjects((prev) => {
          // Remove chat from any current project
          const updated = prev.map((p) => ({
            ...p,
            chatIds: p.chatIds.filter((id) => id !== chatId)
          }));
          // Add to target project
          return updated.map((p) =>
            p.id === projectId
              ? { ...p, chatIds: [chatId, ...p.chatIds], updatedAt: Date.now() }
              : p
          );
        });
      }
    };
    window.addEventListener("assignchattoproject", handler);
    return () => window.removeEventListener("assignchattoproject", handler);
  }, []);

  const createProject = useCallback((name: string): string => {
    const id = generateId();
    const now = Date.now();
    const project: Project = {
      id,
      name,
      createdAt: now,
      updatedAt: now,
      customInstructions: "",
      files: [],
      chatIds: []
    };
    setProjects((prev) => [project, ...prev]);
    return id;
  }, []);

  const renameProject = useCallback((projectId: string, newName: string) => {
    setProjects((prev) =>
      prev.map((p) => (p.id === projectId ? { ...p, name: newName, updatedAt: Date.now() } : p))
    );
  }, []);

  const deleteProject = useCallback((projectId: string) => {
    setProjects((prev) => {
      const project = prev.find((p) => p.id === projectId);
      if (project && project.chatIds.length > 0) {
        // Mark project's chats as deleted in localStorage
        try {
          const existing = JSON.parse(localStorage.getItem(DELETED_CHATS_KEY) || "[]");
          const updated = [...new Set([...existing, ...project.chatIds])];
          localStorage.setItem(DELETED_CHATS_KEY, JSON.stringify(updated));
        } catch {
          // ignore
        }
      }
      return prev.filter((p) => p.id !== projectId);
    });
  }, []);

  const updateCustomInstructions = useCallback((projectId: string, instructions: string) => {
    setProjects((prev) =>
      prev.map((p) =>
        p.id === projectId ? { ...p, customInstructions: instructions, updatedAt: Date.now() } : p
      )
    );
  }, []);

  const addFile = useCallback(
    (projectId: string, file: { name: string; type: string; size: number }) => {
      const projectFile: ProjectFile = {
        id: generateId(),
        name: file.name,
        type: file.type,
        size: file.size,
        addedAt: Date.now()
      };
      setProjects((prev) =>
        prev.map((p) =>
          p.id === projectId
            ? { ...p, files: [...p.files, projectFile], updatedAt: Date.now() }
            : p
        )
      );
    },
    []
  );

  const removeFile = useCallback((projectId: string, fileId: string) => {
    setProjects((prev) =>
      prev.map((p) =>
        p.id === projectId
          ? { ...p, files: p.files.filter((f) => f.id !== fileId), updatedAt: Date.now() }
          : p
      )
    );
  }, []);

  const assignChatToProject = useCallback((chatId: string, projectId: string) => {
    setProjects((prev) => {
      // Remove from any current project first
      const cleaned = prev.map((p) => ({
        ...p,
        chatIds: p.chatIds.filter((id) => id !== chatId)
      }));
      // Add to target
      return cleaned.map((p) =>
        p.id === projectId
          ? { ...p, chatIds: [chatId, ...p.chatIds], updatedAt: Date.now() }
          : p
      );
    });
  }, []);

  const removeChatFromProject = useCallback((chatId: string) => {
    setProjects((prev) =>
      prev.map((p) => ({
        ...p,
        chatIds: p.chatIds.filter((id) => id !== chatId),
        updatedAt: p.chatIds.includes(chatId) ? Date.now() : p.updatedAt
      }))
    );
  }, []);

  const moveChatToProject = useCallback(
    (chatId: string, targetProjectId: string) => {
      assignChatToProject(chatId, targetProjectId);
    },
    [assignChatToProject]
  );

  const getProjectForChat = useCallback(
    (chatId: string): Project | undefined => {
      return projects.find((p) => p.chatIds.includes(chatId));
    },
    [projects]
  );

  const getAllAssignedChatIds = useCallback((): Set<string> => {
    const ids = new Set<string>();
    for (const p of projects) {
      for (const chatId of p.chatIds) {
        ids.add(chatId);
      }
    }
    return ids;
  }, [projects]);

  const getProjectById = useCallback(
    (projectId: string): Project | undefined => {
      return projects.find((p) => p.id === projectId);
    },
    [projects]
  );

  const getDeletedChatIds = useCallback((): Set<string> => {
    try {
      const stored = JSON.parse(localStorage.getItem(DELETED_CHATS_KEY) || "[]");
      return new Set<string>(stored);
    } catch {
      return new Set<string>();
    }
  }, []);

  return (
    <ProjectsContext.Provider
      value={{
        projects,
        createProject,
        renameProject,
        deleteProject,
        updateCustomInstructions,
        addFile,
        removeFile,
        assignChatToProject,
        removeChatFromProject,
        moveChatToProject,
        getProjectForChat,
        getAllAssignedChatIds,
        getProjectById,
        getDeletedChatIds
      }}
    >
      {children}
    </ProjectsContext.Provider>
  );
}
