import { useState, useMemo } from "react";
import { useLocalState } from "@/state/useLocalState";
import { Link } from "@tanstack/react-router";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { MoreHorizontal, Trash, Pencil, FolderPlus } from "lucide-react";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger
} from "@/components/ui/dropdown-menu";
import { RenameChatDialog } from "@/components/RenameChatDialog";
import { AddToProjectDialog } from "@/components/AddToProjectDialog";

interface ChatHistoryListProps {
  currentChatId?: string;
  currentProjectId?: string;
  searchQuery?: string;
}

export function ChatHistoryList({
  currentChatId,
  currentProjectId,
  searchQuery = ""
}: ChatHistoryListProps) {
  // We directly destructure only what we use in this component
  const { fetchOrCreateHistoryList, deleteChat, renameChat, removeChatFromProject } =
    useLocalState();

  // We'll get addChatToProject from the context in the AddToProjectDialog component
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const [isRenameDialogOpen, setIsRenameDialogOpen] = useState(false);
  const [selectedChat, setSelectedChat] = useState<{ id: string; title: string } | null>(null);
  const [isAddToProjectDialogOpen, setIsAddToProjectDialogOpen] = useState(false);
  const [selectedChatForProject, setSelectedChatForProject] = useState<string | null>(null);

  const {
    isPending,
    error,
    data: chats
  } = useQuery({
    queryKey: ["chatHistory"],
    queryFn: () => fetchOrCreateHistoryList()
  });

  // Filter chats based on search query and project
  const filteredChats = useMemo(() => {
    if (!chats) return [];

    // Start with all chats
    let filtered = chats;

    // Filter by project if applicable
    if (currentProjectId) {
      filtered = filtered.filter((chat) => chat.projectId === currentProjectId);
    } else {
      // When viewing "all chats", show those not in any project
      filtered = filtered.filter((chat) => !chat.projectId);
    }

    // Then filter by search query if applicable
    if (searchQuery.trim()) {
      const normalizedQuery = searchQuery.trim().toLowerCase();
      filtered = filtered.filter((chat) => chat.title.toLowerCase().includes(normalizedQuery));
    }

    return filtered;
  }, [chats, searchQuery, currentProjectId]);

  const handleDeleteChat = async (chatId: string) => {
    try {
      await deleteChat(chatId);
    } catch (error) {
      console.error("Error deleting chat:", error);
    }
    queryClient.invalidateQueries({ queryKey: ["chatHistory"] });
    if (chatId === currentChatId) {
      navigate({ to: "/" });
    }
  };

  const handleOpenRenameDialog = (chat: { id: string; title: string }) => {
    setSelectedChat(chat);
    setIsRenameDialogOpen(true);
  };

  const handleRenameChat = async (chatId: string, newTitle: string) => {
    try {
      await renameChat(chatId, newTitle);
      // Invalidate both the chat history list and the specific chat
      queryClient.invalidateQueries({ queryKey: ["chatHistory"] });
      queryClient.invalidateQueries({ queryKey: ["chat", chatId] });
    } catch (error) {
      console.error("Error renaming chat:", error);
      throw error;
    }
  };

  const handleAddToProject = (chatId: string) => {
    setSelectedChatForProject(chatId);
    setIsAddToProjectDialogOpen(true);
  };

  const handleRemoveFromProject = async (chatId: string) => {
    try {
      await removeChatFromProject(chatId);
      queryClient.invalidateQueries({ queryKey: ["chatHistory"] });
      queryClient.invalidateQueries({ queryKey: ["chat", chatId] });
    } catch (error) {
      console.error("Error removing chat from project:", error);
    }
  };

  if (error) {
    return <div>{error.message}</div>;
  }

  if (isPending) {
    return <div>Loading chat history...</div>;
  }

  // Only show no results message if we have a trimmed search query
  const trimmedQuery = searchQuery.trim();
  if (trimmedQuery && filteredChats.length === 0) {
    return (
      <div className="text-muted-foreground text-center py-4">
        <p>No chats found matching "{trimmedQuery}"</p>
        <p className="text-sm mt-1">Try a different search term</p>
      </div>
    );
  }

  return (
    <>
      {filteredChats.map((chat) => (
        <div key={chat.id} className="relative">
          <Link to="/chat/$chatId" params={{ chatId: chat.id }}>
            <div
              className={`rounded-lg py-2 transition-all hover:text-primary cursor-pointer ${
                chat.id === currentChatId ? "text-primary" : "text-muted-foreground"
              }`}
            >
              <div className="overflow-hidden whitespace-nowrap hover:underline pr-8">
                {chat.title}
              </div>
            </div>
          </Link>
          {chat.id === currentChatId && (
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <button
                  className="z-50 bg-background/80 absolute right-2 top-1/2 transform -translate-y-1/2 text-primary"
                  onClick={(e) => {
                    e.preventDefault();
                    e.stopPropagation();
                  }}
                >
                  <MoreHorizontal size={16} />
                </button>
              </DropdownMenuTrigger>
              <DropdownMenuContent>
                <DropdownMenuItem onClick={() => handleOpenRenameDialog(chat)}>
                  <Pencil className="mr-2 h-4 w-4" />
                  <span>Rename Chat</span>
                </DropdownMenuItem>

                {/* Project-related actions */}
                {chat.projectId ? (
                  <DropdownMenuItem onClick={() => handleRemoveFromProject(chat.id)}>
                    <FolderPlus className="mr-2 h-4 w-4" />
                    <span>Remove from Project</span>
                  </DropdownMenuItem>
                ) : (
                  <DropdownMenuItem onClick={() => handleAddToProject(chat.id)}>
                    <FolderPlus className="mr-2 h-4 w-4" />
                    <span>Add to Project</span>
                  </DropdownMenuItem>
                )}

                <DropdownMenuItem onClick={() => handleDeleteChat(chat.id)}>
                  <Trash className="mr-2 h-4 w-4" />
                  <span>Delete Chat</span>
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
          )}
          <div className="absolute inset-y-0 right-0 w-[3rem] bg-gradient-to-l from-background to-transparent pointer-events-none"></div>
        </div>
      ))}

      {selectedChat && (
        <RenameChatDialog
          open={isRenameDialogOpen}
          onOpenChange={setIsRenameDialogOpen}
          chatId={selectedChat.id}
          currentTitle={selectedChat.title}
          onRename={handleRenameChat}
        />
      )}

      <AddToProjectDialog
        chatId={selectedChatForProject || ""}
        open={isAddToProjectDialogOpen}
        onOpenChange={(open) => {
          if (!open) setSelectedChatForProject(null);
          setIsAddToProjectDialogOpen(open);
        }}
      />
    </>
  );
}
