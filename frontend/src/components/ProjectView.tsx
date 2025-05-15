import { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useLocalState } from "@/state/useLocalState";
import { HistoryItem } from "@/state/LocalStateContext";
import { Button } from "./ui/button";
import { PenIcon, PlusIcon, MessageSquarePlusIcon } from "lucide-react";
import { ProjectDialog } from "./ProjectDialog";
import { useNavigate, Link } from "@tanstack/react-router";
import { cn } from "@/utils/utils";

interface ProjectViewProps {
  projectId: string;
}

export function ProjectView({ projectId }: ProjectViewProps) {
  const { getProjectById, addChat, fetchOrCreateHistoryList } = useLocalState();
  const [isEditDialogOpen, setIsEditDialogOpen] = useState(false);
  const queryClient = useQueryClient();
  const navigate = useNavigate();

  const {
    isPending: isProjectLoading,
    error: projectError,
    data: project
  } = useQuery({
    queryKey: ["project", projectId],
    queryFn: () => getProjectById(projectId)
  });

  const {
    isPending: isChatsLoading,
    error: chatsError,
    data: allChats = []
  } = useQuery({
    queryKey: ["chatHistory"],
    queryFn: fetchOrCreateHistoryList
  });

  // Filter chats that belong to this project
  const projectChats = allChats.filter((chat) => chat.projectId === projectId);

  const handleCreateNewChat = async () => {
    try {
      // Create a new chat and add it to this project
      const chatId = await addChat(`New Chat`, projectId);

      // Navigate to the new chat
      navigate({ to: "/chat/$chatId", params: { chatId } });
    } catch (error) {
      console.error("Error creating new chat:", error);
    }
  };

  if (isProjectLoading) {
    return <div className="flex justify-center items-center h-full">Loading project...</div>;
  }

  if (projectError || !project) {
    return (
      <div className="flex flex-col justify-center items-center h-full gap-4">
        <p className="text-destructive">Failed to load project details.</p>
        <Button variant="outline" onClick={() => navigate({ to: "/" })}>
          Go to Home
        </Button>
      </div>
    );
  }

  return (
    <div className="container max-w-3xl mx-auto p-6">
      <div className="flex items-center justify-between mb-6">
        <div>
          <h1 className="text-2xl font-bold">{project.name}</h1>
          {project.description && (
            <p className="text-muted-foreground mt-1">{project.description}</p>
          )}
        </div>
        <Button
          variant="outline"
          size="sm"
          className="gap-2"
          onClick={() => setIsEditDialogOpen(true)}
        >
          <PenIcon className="h-4 w-4" />
          <span>Edit</span>
        </Button>
      </div>

      {project.systemPrompt && (
        <div className="mb-6">
          <h2 className="text-lg font-semibold mb-2">System Prompt</h2>
          <div className="p-4 rounded-md bg-muted">
            <pre className="whitespace-pre-wrap font-mono text-sm">{project.systemPrompt}</pre>
          </div>
        </div>
      )}

      <div className="mb-6">
        <div className="flex items-center justify-between mb-4">
          <h2 className="text-lg font-semibold">Chats in this Project</h2>
          <Button onClick={handleCreateNewChat} className="gap-2">
            <MessageSquarePlusIcon className="h-4 w-4" />
            <span>New Chat</span>
          </Button>
        </div>

        {isChatsLoading ? (
          <p className="text-muted-foreground">Loading chats...</p>
        ) : chatsError ? (
          <p className="text-destructive">Error loading chats</p>
        ) : projectChats.length === 0 ? (
          <div className="text-center py-8 border border-dashed rounded-lg">
            <p className="text-muted-foreground mb-4">No chats in this project yet</p>
            <Button onClick={handleCreateNewChat} className="gap-2">
              <PlusIcon className="h-4 w-4" />
              <span>Start a new chat</span>
            </Button>
          </div>
        ) : (
          <div className="grid grid-cols-1 gap-2">
            {projectChats.map((chat) => (
              <ChatItem key={chat.id} chat={chat} />
            ))}
          </div>
        )}
      </div>

      {/* Edit Project Dialog */}
      <ProjectDialog
        open={isEditDialogOpen}
        onOpenChange={setIsEditDialogOpen}
        project={project}
        onSuccess={() => {
          queryClient.invalidateQueries({ queryKey: ["project", projectId] });
        }}
      />
    </div>
  );
}

interface ChatItemProps {
  chat: HistoryItem;
}

function ChatItem({ chat }: ChatItemProps) {
  return (
    <Button
      variant="ghost"
      className={cn("w-full justify-start text-left h-auto py-3 px-4")}
      asChild
    >
      <Link to="/chat/$chatId" params={{ chatId: chat.id }}>
        <span className="truncate">{chat.title}</span>
      </Link>
    </Button>
  );
}
