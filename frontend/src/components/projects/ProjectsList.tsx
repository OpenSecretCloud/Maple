import { useState, useMemo, useCallback } from "react";
import {
  ChevronDown,
  ChevronRight,
  Folder,
  FolderPlus,
  MoreHorizontal
} from "lucide-react";
import { useNavigate } from "@tanstack/react-router";
import { useOpenAI } from "@/ai/useOpenAi";
import { useProjects } from "@/state/useProjects";
import type { Project } from "@/state/ProjectsContext";
import { CreateProjectDialog } from "./CreateProjectDialog";
import { DeleteProjectDialog } from "./DeleteProjectDialog";
import { RenameChatDialog } from "@/components/RenameChatDialog";
import { DeleteChatDialog } from "@/components/DeleteChatDialog";
import { ChatContextMenu } from "@/components/ChatContextMenu";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger
} from "@/components/ui/dropdown-menu";

interface Conversation {
  id: string;
  object: "conversation";
  created_at: number;
  metadata?: {
    title?: string;
    [key: string]: unknown;
  };
}

interface ProjectsListProps {
  conversations: Conversation[];
  currentChatId?: string;
  searchQuery?: string;
  isMobile?: boolean;
}

const MAX_CHATS_PER_PROJECT = 3;

export function ProjectsList({
  conversations,
  currentChatId,
  searchQuery = "",
  isMobile = false
}: ProjectsListProps) {
  const {
    projects,
    createProject,
    renameProject,
    deleteProject,
    removeChatFromProject,
    assignChatToProject
  } = useProjects();
  const navigate = useNavigate();
  const openai = useOpenAI();

  const [isExpanded, setIsExpanded] = useState(() => {
    try {
      const stored = localStorage.getItem("maple_projects_expanded");
      if (stored !== null) return stored === "true";
      // Default open so user can discover the feature
      return true;
    } catch {
      return true;
    }
  });

  const [isCreateDialogOpen, setIsCreateDialogOpen] = useState(false);
  const [renameTarget, setRenameTarget] = useState<{ id: string; name: string } | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<{ id: string; name: string } | null>(null);

  // Chat rename/delete state
  const [selectedChat, setSelectedChat] = useState<{ id: string; title: string } | null>(null);
  const [isRenameChatOpen, setIsRenameChatOpen] = useState(false);
  const [isDeleteChatOpen, setIsDeleteChatOpen] = useState(false);

  // Persist collapsed state
  const toggleExpanded = useCallback(() => {
    setIsExpanded((prev) => {
      const next = !prev;
      localStorage.setItem("maple_projects_expanded", String(next));
      return next;
    });
  }, []);

  // Build a map of conversation ID → conversation for quick lookup
  const conversationMap = useMemo(() => {
    const map = new Map<string, Conversation>();
    for (const conv of conversations) {
      map.set(conv.id, conv);
    }
    return map;
  }, [conversations]);

  // Filter projects by search query
  const filteredProjects = useMemo(() => {
    if (!searchQuery.trim()) return projects;
    const query = searchQuery.toLowerCase();
    return projects.filter((project) => {
      // Match project name
      if (project.name.toLowerCase().includes(query)) return true;
      // Match any chat title within the project
      return project.chatIds.some((chatId) => {
        const conv = conversationMap.get(chatId);
        return conv?.metadata?.title?.toLowerCase().includes(query);
      });
    });
  }, [projects, searchQuery, conversationMap]);

  // Auto-expand when search finds matches
  const shouldAutoExpand = searchQuery.trim() && filteredProjects.length > 0;

  const handleCreateProject = useCallback(
    (name: string) => {
      const id = createProject(name);
      navigate({ to: "/project/$projectId", params: { projectId: id } });
    },
    [createProject, navigate]
  );

  const handleRenameProject = useCallback(
    (newName: string) => {
      if (renameTarget) {
        renameProject(renameTarget.id, newName);
        setRenameTarget(null);
      }
    },
    [renameTarget, renameProject]
  );

  const handleDeleteProject = useCallback(() => {
    if (deleteTarget) {
      deleteProject(deleteTarget.id);
      setDeleteTarget(null);
    }
  }, [deleteTarget, deleteProject]);

  const handleOpenRenameChat = useCallback((chatId: string, title: string) => {
    setSelectedChat({ id: chatId, title });
    setIsRenameChatOpen(true);
  }, []);

  const handleOpenDeleteChat = useCallback((chatId: string, title: string) => {
    setSelectedChat({ id: chatId, title });
    setIsDeleteChatOpen(true);
  }, []);

  const handleRenameChat = useCallback(
    async (chatId: string, newTitle: string) => {
      if (!openai) return;
      await openai.conversations.update(chatId, {
        metadata: { title: newTitle }
      });
      // Dispatch event so ChatHistoryList and other consumers refresh
      window.dispatchEvent(
        new CustomEvent("conversationrenamed", {
          detail: { conversationId: chatId, title: newTitle }
        })
      );
    },
    [openai]
  );

  const handleDeleteChat = useCallback(
    async (chatId: string) => {
      if (!openai) return;
      try {
        await openai.conversations.delete(chatId);
        // Remove from project
        removeChatFromProject(chatId);
        // If deleting the current chat, navigate to home
        if (chatId === currentChatId) {
          const params = new URLSearchParams(window.location.search);
          params.delete("conversation_id");
          window.history.replaceState({}, "", params.toString() ? `/?${params}` : "/");
          window.dispatchEvent(new Event("newchat"));
        }
      } catch (error) {
        console.error("Error deleting conversation:", error);
      }
    },
    [openai, currentChatId, removeChatFromProject]
  );

  const handleSelectConversation = useCallback(
    (conversationId: string) => {
      const params = new URLSearchParams(window.location.search);
      params.set("conversation_id", conversationId);
      window.history.pushState({}, "", `/?${params}`);
      window.dispatchEvent(
        new CustomEvent("conversationselected", {
          detail: { conversationId }
        })
      );
    },
    []
  );

  const handleProjectClick = useCallback(
    (projectId: string) => {
      navigate({ to: "/project/$projectId", params: { projectId } });
    },
    [navigate]
  );

  const getProjectChats = useCallback(
    (project: Project): Conversation[] => {
      const chats: Conversation[] = [];
      for (const chatId of project.chatIds) {
        const conv = conversationMap.get(chatId);
        if (conv) chats.push(conv);
        if (chats.length >= MAX_CHATS_PER_PROJECT) break;
      }
      return chats;
    },
    [conversationMap]
  );

  const showExpanded = isExpanded || shouldAutoExpand;

  return (
    <>
      <div className="mt-2 mb-2">
        {/* Projects header */}
        <button
          onClick={toggleExpanded}
          className="group/header flex items-center gap-1 w-full text-sm text-muted-foreground hover:text-foreground transition-colors py-1"
        >
          <span className="font-medium">Projects</span>
          {/* Chevron: always visible when collapsed or on mobile; hover-only on desktop when expanded */}
          <span
            className={
              !showExpanded || isMobile
                ? ""
                : "opacity-0 group-hover/header:opacity-100 transition-opacity"
            }
          >
            {showExpanded ? (
              <ChevronDown className="h-3.5 w-3.5" />
            ) : (
              <ChevronRight className="h-3.5 w-3.5" />
            )}
          </span>
        </button>

        {showExpanded && (
          <div className="mt-0.5 flex flex-col">
            {/* + New project button */}
            <button
              onClick={() => setIsCreateDialogOpen(true)}
              className="flex items-center gap-2 py-1.5 w-full text-muted-foreground hover:text-foreground transition-colors rounded-md hover:bg-muted/50 px-1"
            >
              <FolderPlus className="h-4 w-4 flex-shrink-0" />
              <span>New project</span>
            </button>

            {/* Project list */}
            {filteredProjects.map((project) => (
              <ProjectEntry
                key={project.id}
                project={project}
                allProjects={projects}
                chats={getProjectChats(project)}
                currentChatId={currentChatId}
                isMobile={isMobile}
                onProjectClick={handleProjectClick}
                onChatClick={handleSelectConversation}
                onRename={(id, name) => setRenameTarget({ id, name })}
                onDelete={(id, name) => setDeleteTarget({ id, name })}
                onRemoveChatFromProject={removeChatFromProject}
                onMoveChatToProject={assignChatToProject}
                onRenameChat={handleOpenRenameChat}
                onDeleteChat={handleOpenDeleteChat}
              />
            ))}
          </div>
        )}
      </div>

      <CreateProjectDialog
        open={isCreateDialogOpen}
        onOpenChange={setIsCreateDialogOpen}
        onSubmit={handleCreateProject}
      />
      <CreateProjectDialog
        open={!!renameTarget}
        onOpenChange={(open) => !open && setRenameTarget(null)}
        onSubmit={handleRenameProject}
        mode="rename"
        initialName={renameTarget?.name || ""}
      />
      <DeleteProjectDialog
        open={!!deleteTarget}
        onOpenChange={(open) => !open && setDeleteTarget(null)}
        onConfirm={handleDeleteProject}
        projectName={deleteTarget?.name || ""}
      />
      {selectedChat && (
        <>
          <RenameChatDialog
            open={isRenameChatOpen}
            onOpenChange={setIsRenameChatOpen}
            chatId={selectedChat.id}
            currentTitle={selectedChat.title}
            onRename={handleRenameChat}
          />
          <DeleteChatDialog
            open={isDeleteChatOpen}
            onOpenChange={setIsDeleteChatOpen}
            chatTitle={selectedChat.title}
            onConfirm={() => handleDeleteChat(selectedChat.id)}
          />
        </>
      )}
    </>
  );
}

// --- Individual project entry in sidebar ---

function ProjectEntry({
  project,
  allProjects,
  chats,
  currentChatId,
  isMobile,
  onProjectClick,
  onChatClick,
  onRename,
  onDelete,
  onRemoveChatFromProject,
  onMoveChatToProject,
  onRenameChat,
  onDeleteChat
}: {
  project: Project;
  allProjects: Project[];
  chats: { id: string; metadata?: { title?: string } }[];
  currentChatId?: string;
  isMobile: boolean;
  onProjectClick: (projectId: string) => void;
  onChatClick: (conversationId: string) => void;
  onRename: (projectId: string, name: string) => void;
  onDelete: (projectId: string, name: string) => void;
  onRemoveChatFromProject: (chatId: string) => void;
  onMoveChatToProject: (chatId: string, projectId: string) => void;
  onRenameChat: (chatId: string, title: string) => void;
  onDeleteChat: (chatId: string, title: string) => void;
}) {
  const otherProjects = allProjects.filter((p) => p.id !== project.id);

  return (
    <div className="mt-0.5">
      {/* Project name row */}
      <div className="group/project flex items-center w-full rounded-md hover:bg-muted/50 transition-colors">
        <button
          onClick={() => onProjectClick(project.id)}
          className="flex items-center gap-2 py-1.5 px-1 flex-1 min-w-0 font-medium hover:text-primary transition-colors truncate"
        >
          <Folder className="h-4 w-4 flex-shrink-0 text-muted-foreground" />
          <span className="truncate">{project.name}</span>
        </button>
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <button
              className="opacity-0 group-hover/project:opacity-100 focus:opacity-100 transition-opacity p-1 mr-1 flex-shrink-0"
              onClick={(e) => {
                e.preventDefault();
                e.stopPropagation();
              }}
            >
              <MoreHorizontal className="h-4 w-4" />
            </button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end">
            <DropdownMenuItem onClick={() => onRename(project.id, project.name)}>
              Rename project
            </DropdownMenuItem>
            <DropdownMenuItem
              onClick={() => onDelete(project.id, project.name)}
              className="text-destructive focus:text-destructive"
            >
              Delete project
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>

      {/* Recent chats under this project — styled like Recents items */}
      {chats.length > 0 && (
        <div className="flex flex-col -mt-1">
          {chats.map((chat) => {
            const title = chat.metadata?.title || "Untitled";
            const isActive = chat.id === currentChatId;
            return (
              <div
                key={chat.id}
                className="relative group select-none"
              >
                <div
                  onClick={() => onChatClick(chat.id)}
                  className={`rounded-lg py-2 pl-7 pr-8 transition-all hover:text-primary cursor-pointer ${
                    isActive ? "text-primary" : "text-muted-foreground"
                  }`}
                >
                  <div className="overflow-hidden whitespace-nowrap hover:underline">
                    {title}
                  </div>
                </div>
                {/* Three-dot context menu */}
                <ChatContextMenu
                  chatId={chat.id}
                  isMobile={isMobile}
                  projects={otherProjects}
                  currentProjectName={project.name}
                  onRename={() => onRenameChat(chat.id, title)}
                  onMoveToProject={(targetId) => onMoveChatToProject(chat.id, targetId)}
                  onRemoveFromProject={() => onRemoveChatFromProject(chat.id)}
                  onDelete={() => onDeleteChat(chat.id, title)}
                />
                <div className="absolute inset-y-0 right-0 w-[3rem] bg-gradient-to-l from-background to-transparent pointer-events-none group-hover:opacity-0 transition-opacity"></div>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}

