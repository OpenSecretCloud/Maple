import { useState, useCallback, useRef, useEffect } from "react";
import { useNavigate } from "@tanstack/react-router";
import {
  Folder,
  MoreHorizontal,
  FileText,
  Plus,
  X,
  Send,
  Globe,
  Mic
} from "lucide-react";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger
} from "@/components/ui/dropdown-menu";
import { ChatContextMenu } from "@/components/ChatContextMenu";
import { RenameChatDialog } from "@/components/RenameChatDialog";
import { DeleteChatDialog } from "@/components/DeleteChatDialog";
import { useOpenAI } from "@/ai/useOpenAi";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import { Sidebar, SidebarToggle } from "@/components/Sidebar";
import { useProjects } from "@/state/useProjects";
import { useIsMobile } from "@/utils/utils";
import { useOpenSecret } from "@opensecret/react";
import { ModelSelector } from "@/components/ModelSelector";
import { CreateProjectDialog } from "./CreateProjectDialog";
import { DeleteProjectDialog } from "./DeleteProjectDialog";
import { CustomInstructionsDialog } from "./CustomInstructionsDialog";
import { RemoveFileDialog } from "./RemoveFileDialog";
import type { Project } from "@/state/ProjectsContext";

interface ProjectDetailPageProps {
  projectId: string;
}

interface ConversationData {
  id: string;
  metadata?: {
    title?: string;
    [key: string]: unknown;
  };
}

export function ProjectDetailPage({ projectId }: ProjectDetailPageProps) {
  const navigate = useNavigate();
  const isMobile = useIsMobile();
  const opensecret = useOpenSecret();
  const {
    projects,
    getProjectById,
    renameProject,
    deleteProject,
    updateCustomInstructions,
    addFile,
    removeFile,
    removeChatFromProject,
    assignChatToProject
  } = useProjects();

  const project = getProjectById(projectId);

  const [isSidebarOpen, setIsSidebarOpen] = useState(!isMobile);
  const [newChatInput, setNewChatInput] = useState("");
  const [isRenameOpen, setIsRenameOpen] = useState(false);
  const [isDeleteOpen, setIsDeleteOpen] = useState(false);
  const [isInstructionsOpen, setIsInstructionsOpen] = useState(false);
  const [fileToRemove, setFileToRemove] = useState<{ id: string; name: string } | null>(null);
  const [selectedChat, setSelectedChat] = useState<{ id: string; title: string } | null>(null);
  const [isRenameChatOpen, setIsRenameChatOpen] = useState(false);
  const [isDeleteChatOpen, setIsDeleteChatOpen] = useState(false);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const openai = useOpenAI();

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
      // Update local map
      setConversationMap((prev) => {
        const next = new Map(prev);
        const conv = next.get(chatId);
        if (conv) {
          next.set(chatId, { ...conv, metadata: { ...conv.metadata, title: newTitle } });
        }
        return next;
      });
    },
    [openai]
  );

  const handleDeleteChat = useCallback(
    async (chatId: string) => {
      if (!openai) return;
      try {
        await openai.conversations.delete(chatId);
        removeChatFromProject(chatId);
      } catch (error) {
        console.error("Error deleting conversation:", error);
      }
    },
    [openai, removeChatFromProject]
  );

  // Fetch conversation metadata for chat names
  const [conversationMap, setConversationMap] = useState<Map<string, ConversationData>>(new Map());

  useEffect(() => {
    if (!opensecret || !project?.chatIds.length) return;
    let cancelled = false;

    async function fetchConversations() {
      try {
        const response = await opensecret.listConversations({ limit: 100 });
        if (cancelled) return;
        const map = new Map<string, ConversationData>();
        for (const conv of response.data || []) {
          map.set(conv.id, conv);
        }
        setConversationMap(map);
      } catch {
        // Silently fail — names just won't show
      }
    }

    fetchConversations();
    return () => { cancelled = true; };
  }, [opensecret, project?.chatIds.length]);

  const toggleSidebar = useCallback(() => setIsSidebarOpen((prev) => !prev), []);

  const handleStartChat = useCallback(() => {
    const message = newChatInput.trim();
    if (!message) return;
    setNewChatInput("");
    // Navigate to home with project_id param so UnifiedChat can auto-assign
    navigate({ to: "/" });
    setTimeout(() => {
      window.history.replaceState(null, "", `/?project_id=${projectId}`);
      window.dispatchEvent(new Event("newchat"));
      // Set the message to be sent
      setTimeout(() => {
        const textarea = document.getElementById("message") as HTMLTextAreaElement;
        if (textarea) {
          const nativeInputValueSetter = Object.getOwnPropertyDescriptor(
            window.HTMLTextAreaElement.prototype,
            "value"
          )?.set;
          nativeInputValueSetter?.call(textarea, message);
          textarea.dispatchEvent(new Event("input", { bubbles: true }));
          textarea.focus();
          // Auto-submit after a brief delay
          setTimeout(() => {
            const form = textarea.closest("form");
            if (form) {
              form.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
            }
          }, 50);
        }
      }, 100);
    }, 0);
  }, [newChatInput, projectId, navigate]);

  const handleKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      handleStartChat();
    }
  };

  const handleRename = useCallback(
    (newName: string) => {
      renameProject(projectId, newName);
    },
    [projectId, renameProject]
  );

  const handleDelete = useCallback(() => {
    deleteProject(projectId);
    navigate({ to: "/" });
  }, [projectId, deleteProject, navigate]);

  const handleSaveInstructions = useCallback(
    (instructions: string) => {
      updateCustomInstructions(projectId, instructions);
    },
    [projectId, updateCustomInstructions]
  );

  const handleFileUpload = useCallback(
    (e: React.ChangeEvent<HTMLInputElement>) => {
      const files = e.target.files;
      if (!files) return;
      for (const file of Array.from(files)) {
        const ext = file.name.split(".").pop()?.toLowerCase() || "file";
        addFile(projectId, { name: file.name, type: ext, size: file.size });
      }
      e.target.value = "";
    },
    [projectId, addFile]
  );

  const handleRemoveFile = useCallback(() => {
    if (fileToRemove) {
      removeFile(projectId, fileToRemove.id);
      setFileToRemove(null);
    }
  }, [projectId, fileToRemove, removeFile]);

  const handleSelectConversation = useCallback((conversationId: string) => {
    const params = new URLSearchParams(window.location.search);
    params.set("conversation_id", conversationId);
    window.history.pushState({}, "", `/?${params}`);
    window.dispatchEvent(
      new CustomEvent("conversationselected", {
        detail: { conversationId }
      })
    );
  }, []);

  if (!project) {
    return (
      <div
        className={`grid h-dvh min-h-0 w-full grid-cols-1 overflow-hidden ${isSidebarOpen ? "md:grid-cols-[280px_1fr]" : ""}`}
      >
        <Sidebar isOpen={isSidebarOpen} onToggle={toggleSidebar} />
        <main className="flex h-dvh flex-col items-center justify-center">
          {!isSidebarOpen && (
            <div className="fixed top-[9.5px] left-4 z-20">
              <SidebarToggle onToggle={toggleSidebar} />
            </div>
          )}
          <p className="text-muted-foreground">Project not found</p>
          <Button variant="outline" className="mt-4" onClick={() => navigate({ to: "/" })}>
            Go home
          </Button>
        </main>
      </div>
    );
  }

  return (
    <div
      className={`grid h-dvh min-h-0 w-full grid-cols-1 overflow-hidden ${isSidebarOpen ? "md:grid-cols-[280px_1fr]" : ""}`}
    >
      <Sidebar isOpen={isSidebarOpen} onToggle={toggleSidebar} />
      <main className="flex h-dvh flex-col bg-card/90 backdrop-blur-lg overflow-hidden">
        {/* Sidebar toggle - visible when sidebar is closed */}
        {!isSidebarOpen && (
          <div className="fixed top-[9.5px] left-4 z-20">
            <SidebarToggle onToggle={toggleSidebar} />
          </div>
        )}

        {/* Header — centered title matching chat page */}
        <div className="h-14 flex items-center px-4">
          <div className="flex-1 flex items-center justify-center relative">
            <Folder className="h-4 w-4 text-muted-foreground mr-2 flex-shrink-0" />
            <h1 className="text-base font-medium truncate max-w-[20rem]">{project.name}</h1>
          </div>
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button variant="ghost" size="icon" className="h-9 w-9 flex-shrink-0">
                <MoreHorizontal className="h-4 w-4" />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
              <DropdownMenuItem onClick={() => setIsRenameOpen(true)}>
                Rename project
              </DropdownMenuItem>
              <DropdownMenuItem
                onClick={() => setIsDeleteOpen(true)}
                className="text-destructive focus:text-destructive"
              >
                Delete project
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </div>

        {/* Content — two columns on desktop, stacked on mobile */}
        <div className="flex-1 overflow-y-auto">
          <div className="flex flex-col md:flex-row gap-6 p-6 max-w-5xl mx-auto">
            {/* Left column: Chat input + chat list */}
            <div className="flex-1 min-w-0">
              {/* Chat input — styled like UnifiedChat */}
              <div className="mb-6">
                <div className="relative rounded-xl border-2 border-border focus-within:border-purple-500 transition-colors bg-background overflow-hidden">
                  <Textarea
                    ref={textareaRef}
                    value={newChatInput}
                    onChange={(e) => setNewChatInput(e.target.value)}
                    onKeyDown={handleKeyDown}
                    placeholder={`New chat in ${project.name}`}
                    className="w-full resize-none min-h-[52px] max-h-[200px] px-4 pt-3 pb-2 border-0 bg-transparent focus-visible:ring-0 focus-visible:ring-offset-0 placeholder:text-muted-foreground/60"
                    rows={1}
                  />
                  {/* Bottom toolbar — matches UnifiedChat layout */}
                  <div className="flex items-center justify-between px-3 pb-2 pt-1 border-t border-border/50">
                    <div className="flex items-center gap-2">
                      <ModelSelector />
                      <Button
                        type="button"
                        variant="ghost"
                        size="sm"
                        className="h-8 w-8 p-0"
                        aria-label="Web search"
                      >
                        <Globe className="h-4 w-4 text-muted-foreground" />
                      </Button>
                    </div>
                    <div className="flex items-center gap-2">
                      <Button
                        type="button"
                        variant="ghost"
                        size="icon"
                        className="h-9 w-9 rounded-lg hover:bg-muted"
                        aria-label="Voice input"
                      >
                        <Mic className="h-4 w-4" />
                      </Button>
                      <Button
                        size="icon"
                        className="h-9 w-9 rounded-lg"
                        onClick={handleStartChat}
                        disabled={!newChatInput.trim()}
                      >
                        <Send className="h-4 w-4" />
                      </Button>
                    </div>
                  </div>
                </div>
              </div>

              {/* Chat list */}
              <div>
                <h3 className="text-sm font-medium text-muted-foreground mb-3">Chats</h3>
                <ProjectChatList
                  project={project}
                  projects={projects}
                  conversationMap={conversationMap}
                  onSelectConversation={handleSelectConversation}
                  onRemoveFromProject={removeChatFromProject}
                  onMoveToProject={assignChatToProject}
                  onRenameChat={handleOpenRenameChat}
                  onDeleteChat={handleOpenDeleteChat}
                  isMobile={isMobile}
                />
              </div>
            </div>

            {/* Right column: Custom instructions + Files */}
            <div className="w-full md:w-80 space-y-6 flex-shrink-0">
              {/* Custom Instructions */}
              <div>
                <h3 className="text-sm font-medium mb-1">Custom instructions</h3>
                <p className="text-xs text-muted-foreground mb-2">
                  Set context and customize how Maple responds in this project.
                </p>
                <button
                  onClick={() => setIsInstructionsOpen(true)}
                  className="w-full text-left border border-input rounded-lg p-3 text-sm hover:border-primary transition-colors cursor-pointer min-h-[60px]"
                >
                  {project.customInstructions ? (
                    <span className="text-foreground line-clamp-3">
                      {project.customInstructions}
                    </span>
                  ) : (
                    <span className="text-muted-foreground italic">
                      e.g., &ldquo;Use concise bullet points. Focus on practical examples.&rdquo;
                    </span>
                  )}
                </button>
              </div>

              {/* Files */}
              <div>
                <div className="flex items-center justify-between mb-2">
                  <h3 className="text-sm font-medium">Files</h3>
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={() => fileInputRef.current?.click()}
                  >
                    <Plus className="h-4 w-4 mr-1" />
                    Add files
                  </Button>
                </div>
                {project.files.length > 0 ? (
                  <div className="space-y-2">
                    {project.files.map((file) => (
                      <div
                        key={file.id}
                        className="flex items-center gap-2 border border-input rounded-lg p-2"
                      >
                        <FileText className="h-4 w-4 text-muted-foreground flex-shrink-0" />
                        <span className="text-sm flex-1 truncate">{file.name}</span>
                        <span className="text-xs text-muted-foreground uppercase font-medium px-1.5 py-0.5 bg-muted rounded">
                          {file.type}
                        </span>
                        <Button
                          variant="ghost"
                          size="icon"
                          className="h-6 w-6 flex-shrink-0"
                          onClick={() => setFileToRemove({ id: file.id, name: file.name })}
                        >
                          <X className="h-3 w-3" />
                        </Button>
                      </div>
                    ))}
                  </div>
                ) : (
                  <p className="text-sm text-muted-foreground">
                    Add documents, code, and other files for Maple to reference in your chats.
                  </p>
                )}
                <input
                  ref={fileInputRef}
                  type="file"
                  multiple
                  accept=".pdf,.txt,.md,.jpg,.jpeg,.png,.webp"
                  onChange={handleFileUpload}
                  className="hidden"
                />
              </div>
            </div>
          </div>
        </div>
      </main>

      {/* Dialogs */}
      <CreateProjectDialog
        open={isRenameOpen}
        onOpenChange={setIsRenameOpen}
        onSubmit={handleRename}
        mode="rename"
        initialName={project.name}
      />
      <DeleteProjectDialog
        open={isDeleteOpen}
        onOpenChange={setIsDeleteOpen}
        onConfirm={handleDelete}
        projectName={project.name}
      />
      <CustomInstructionsDialog
        open={isInstructionsOpen}
        onOpenChange={setIsInstructionsOpen}
        currentInstructions={project.customInstructions}
        onSave={handleSaveInstructions}
      />
      <RemoveFileDialog
        open={!!fileToRemove}
        onOpenChange={(open) => !open && setFileToRemove(null)}
        onConfirm={handleRemoveFile}
        fileName={fileToRemove?.name || ""}
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
    </div>
  );
}

// --- Project Chat List (shows chats assigned to this project) ---

function ProjectChatList({
  project,
  projects,
  conversationMap,
  onSelectConversation,
  onRemoveFromProject,
  onMoveToProject,
  onRenameChat,
  onDeleteChat,
  isMobile
}: {
  project: Project;
  projects: Project[];
  conversationMap: Map<string, ConversationData>;
  onSelectConversation: (id: string) => void;
  onRemoveFromProject: (chatId: string) => void;
  onMoveToProject: (chatId: string, projectId: string) => void;
  onRenameChat: (chatId: string, title: string) => void;
  onDeleteChat: (chatId: string, title: string) => void;
  isMobile: boolean;
}) {
  if (project.chatIds.length === 0) {
    return (
      <p className="text-sm text-muted-foreground py-4">
        No chats yet. Start a conversation above.
      </p>
    );
  }

  // Other projects to move chats to
  const otherProjects = projects.filter((p) => p.id !== project.id);

  return (
    <div className="space-y-1">
      {project.chatIds.map((chatId) => {
        const conv = conversationMap.get(chatId);
        const title = conv?.metadata?.title || "Untitled Chat";
        return (
          <ProjectChatRow
            key={chatId}
            chatId={chatId}
            title={title}
            project={project}
            otherProjects={otherProjects}
            onClick={() => onSelectConversation(chatId)}
            onRemove={() => onRemoveFromProject(chatId)}
            onMoveToProject={(targetId) => onMoveToProject(chatId, targetId)}
            onRenameChat={() => onRenameChat(chatId, title)}
            onDeleteChat={() => onDeleteChat(chatId, title)}
            isMobile={isMobile}
          />
        );
      })}
    </div>
  );
}

function ProjectChatRow({
  chatId,
  title,
  project,
  otherProjects,
  onClick,
  onRemove,
  onMoveToProject,
  onRenameChat,
  onDeleteChat,
  isMobile
}: {
  chatId: string;
  title: string;
  project: Project;
  otherProjects: { id: string; name: string }[];
  onClick: () => void;
  onRemove: () => void;
  onMoveToProject: (projectId: string) => void;
  onRenameChat: () => void;
  onDeleteChat: () => void;
  isMobile: boolean;
}) {
  return (
    <div
      className="group relative flex items-center py-2 px-3 rounded-lg hover:bg-muted/50 cursor-pointer transition-colors"
      onClick={onClick}
    >
      <div className="flex-1 min-w-0">
        <div className="text-sm truncate">{title}</div>
      </div>
      <ChatContextMenu
        chatId={chatId}
        isMobile={isMobile}
        projects={otherProjects}
        currentProjectName={project.name}
        onRename={onRenameChat}
        onMoveToProject={onMoveToProject}
        onRemoveFromProject={onRemove}
        onDelete={onDeleteChat}
      />
    </div>
  );
}
