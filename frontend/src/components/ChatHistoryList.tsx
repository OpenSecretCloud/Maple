import { useLocalState } from "@/state/useLocalState";
import { Link } from "@tanstack/react-router";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { MoreHorizontal, Trash } from "lucide-react";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger
} from "@/components/ui/dropdown-menu";

interface ChatHistoryListProps {
  currentChatId?: string;
}

export function ChatHistoryList({ currentChatId }: ChatHistoryListProps) {
  const { fetchOrCreateHistoryList, deleteChat } = useLocalState();
  const navigate = useNavigate();
  const queryClient = useQueryClient();

  const {
    isPending,
    error,
    data: chats
  } = useQuery({
    queryKey: ["chatHistory"],
    queryFn: () => fetchOrCreateHistoryList()
  });

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

  if (error) {
    return <div>{error.message}</div>;
  }

  if (isPending) {
    return <div>Loading chat history...</div>;
  }

  return (
    <>
      {chats.map((chat) => (
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
    </>
  );
}
