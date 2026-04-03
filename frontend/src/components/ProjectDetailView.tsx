import { Folder, MessageSquare, MoreHorizontal, Pencil, Trash2, X } from "lucide-react";
import type { Conversation, ConversationProject } from "@opensecret/react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger
} from "@/components/ui/dropdown-menu";

interface ProjectDetailViewProps {
  project: ConversationProject | null;
  conversations: Conversation[];
  input: string;
  onInputChange: (value: string) => void;
  onSubmit: (event: React.FormEvent<HTMLFormElement>) => void;
  onSelectConversation: (conversation: Conversation) => void;
  onRenameProject: () => void;
  onDeleteProject: () => void;
  onClearProject: () => void;
  instructionsDraft: string;
  onInstructionsChange: (value: string) => void;
  onSaveInstructions: () => void;
  instructionsDirty: boolean;
  isSavingInstructions: boolean;
  isGenerating: boolean;
  isLoading?: boolean;
}

export function ProjectDetailView({
  project,
  conversations,
  input,
  onInputChange,
  onSubmit,
  onSelectConversation,
  onRenameProject,
  onDeleteProject,
  onClearProject,
  instructionsDraft,
  onInstructionsChange,
  onSaveInstructions,
  instructionsDirty,
  isSavingInstructions,
  isGenerating,
  isLoading = false
}: ProjectDetailViewProps) {
  if (isLoading && !project) {
    return (
      <div className="flex flex-1 items-center justify-center">
        <p className="text-sm text-muted-foreground">Loading project...</p>
      </div>
    );
  }

  if (!project) {
    return (
      <div className="flex flex-1 items-center justify-center">
        <p className="text-sm text-muted-foreground">Project not found.</p>
      </div>
    );
  }

  return (
    <div className="flex flex-1 min-h-0 overflow-y-auto">
      <div className="mx-auto flex w-full max-w-6xl flex-col gap-6 px-4 py-6 md:px-8">
        <div className="flex items-start justify-between gap-4">
          <div className="min-w-0">
            <div className="flex items-center gap-3">
              <div className="flex h-10 w-10 items-center justify-center rounded-lg border bg-muted/40">
                <Folder className="h-5 w-5 text-muted-foreground" />
              </div>
              <div className="min-w-0">
                <h1 className="truncate text-xl font-semibold">{project.name}</h1>
                <p className="text-sm text-muted-foreground">
                  Organize chats and keep project-specific instructions in one place.
                </p>
              </div>
            </div>
          </div>

          <div className="flex items-center gap-2">
            <Button type="button" variant="ghost" size="icon" onClick={onClearProject}>
              <X className="h-4 w-4" />
            </Button>
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button type="button" variant="outline" size="icon">
                  <MoreHorizontal className="h-4 w-4" />
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end">
                <DropdownMenuItem onClick={onRenameProject}>
                  <Pencil className="mr-2 h-4 w-4" />
                  Rename Project
                </DropdownMenuItem>
                <DropdownMenuItem onClick={onDeleteProject}>
                  <Trash2 className="mr-2 h-4 w-4" />
                  Delete Project
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
          </div>
        </div>

        <div className="grid min-h-0 gap-6 lg:grid-cols-[minmax(0,1fr)_320px]">
          <div className="space-y-4">
            <form onSubmit={onSubmit}>
              <div className="flex flex-col gap-3 sm:flex-row">
                <Input
                  value={input}
                  onChange={(event) => onInputChange(event.target.value)}
                  placeholder={`New chat in ${project.name}`}
                  className="h-11"
                  disabled={isGenerating}
                />
                <Button
                  type="submit"
                  className="h-11 sm:px-6"
                  disabled={!input.trim() || isGenerating}
                >
                  {isGenerating ? "Starting..." : "Start Chat"}
                </Button>
              </div>
            </form>

            <div className="rounded-xl border bg-background/60">
              <div className="border-b px-4 py-3">
                <h2 className="font-medium">Chats</h2>
              </div>
              <div className="flex flex-col">
                {conversations.length === 0 ? (
                  <div className="px-4 py-8 text-sm text-muted-foreground">
                    No chats in this project yet.
                  </div>
                ) : (
                  conversations.map((conversation) => (
                    <button
                      key={conversation.id}
                      type="button"
                      onClick={() => onSelectConversation(conversation)}
                      className="flex items-start gap-3 border-b px-4 py-3 text-left transition-colors hover:bg-accent/50 last:border-b-0"
                    >
                      <div className="mt-0.5 flex h-8 w-8 items-center justify-center rounded-md bg-muted">
                        <MessageSquare className="h-4 w-4 text-muted-foreground" />
                      </div>
                      <div className="min-w-0">
                        <div className="truncate font-medium">
                          {(conversation.metadata?.title as string | undefined) || "Untitled Chat"}
                        </div>
                        <div className="text-xs text-muted-foreground">
                          {new Date(conversation.last_activity_at * 1000).toLocaleDateString()}
                        </div>
                      </div>
                    </button>
                  ))
                )}
              </div>
            </div>
          </div>

          <div className="rounded-xl border bg-background/60 p-4">
            <div className="mb-4">
              <h2 className="font-medium">Custom instructions</h2>
              <p className="mt-1 text-sm text-muted-foreground">
                These instructions apply to chats in this project.
              </p>
            </div>
            <Textarea
              value={instructionsDraft}
              onChange={(event) => onInstructionsChange(event.target.value)}
              placeholder="e.g. Write concise bullet points. Focus on practical examples."
              className="min-h-[220px] resize-none"
            />
            <div className="mt-3 flex justify-end">
              <Button
                type="button"
                variant="outline"
                onClick={onSaveInstructions}
                disabled={!instructionsDirty || isSavingInstructions}
              >
                {isSavingInstructions ? "Saving..." : "Save Instructions"}
              </Button>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
