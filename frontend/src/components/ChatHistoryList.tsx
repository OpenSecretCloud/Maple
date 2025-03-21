import { useState, useMemo } from "react";
import { useLocalState } from "@/state/useLocalState";
import { Link } from "@tanstack/react-router";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { MoreHorizontal, Trash, Pencil } from "lucide-react";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger
} from "@/components/ui/dropdown-menu";
import { RenameChatDialog } from "@/components/RenameChatDialog";

interface ChatHistoryListProps {
  currentChatId?: string;
  searchQuery?: string;
}

export function ChatHistoryList({ currentChatId, searchQuery = "" }: ChatHistoryListProps) {
  const { fetchOrCreateHistoryList, deleteChat, renameChat } = useLocalState();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const [isRenameDialogOpen, setIsRenameDialogOpen] = useState(false);
  const [selectedChat, setSelectedChat] = useState<{ id: string; title: string } | null>(null);

  const {
    isPending,
    error,
    data: chats
  } = useQuery({
    queryKey: ["chatHistory"],
    queryFn: () => fetchOrCreateHistoryList()
  });

  // Filter chats based on search query
  const filteredChats = useMemo(() => {
    if (!chats) return [];
    if (!searchQuery.trim()) return chats;

    const normalizedQuery = searchQuery.trim().toLowerCase();
    return chats.filter((chat) => chat.title.toLowerCase().includes(normalizedQuery));
  }, [chats, searchQuery]);

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

  if (error) {
    return <div>{error.message}</div>;
  }

  if (isPending) {
    return <div>Loading chat history...</div>;
  }

  if (filteredChats.length === 0 && searchQuery.trim()) {
    return (
      <div className="text-muted-foreground text-center py-4">
        <p>No chats found matching "{searchQuery.trim()}"</p>
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
    </>
  );
}
