import { Check, Folder, FolderOpen } from "lucide-react";
import { useQuery } from "@tanstack/react-query";
import { useOpenSecret } from "@opensecret/react";
import { Button } from "@/components/ui/button";
import { cn } from "@/utils/utils";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger
} from "@/components/ui/dropdown-menu";

interface ConversationProjectPickerProps {
  selectedProjectId: string | null;
  onSelect: (projectId: string | null) => void | Promise<void>;
  disabled?: boolean;
}

export function ConversationProjectPicker({
  selectedProjectId,
  onSelect,
  disabled = false
}: ConversationProjectPickerProps) {
  const os = useOpenSecret();
  const { data: projects = [] } = useQuery({
    queryKey: ["conversationProjects"],
    queryFn: async () => {
      const response = await os.listConversationProjects({ limit: 20 });
      return response.data ?? [];
    },
    enabled: !!os.auth.user
  });

  const selectedProject = projects.find((project) => project.id === selectedProjectId) ?? null;
  const isProjectSelected = !!selectedProject;

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button
          type="button"
          variant="ghost"
          size="sm"
          className={cn(
            "h-8 gap-2 px-2 text-muted-foreground",
            isProjectSelected &&
              "bg-blue-500/10 text-blue-500 hover:bg-blue-500/15 hover:text-blue-500"
          )}
          disabled={disabled}
          aria-label={selectedProject ? `Project: ${selectedProject.name}` : "No project selected"}
        >
          {isProjectSelected ? <FolderOpen className="h-4 w-4" /> : <Folder className="h-4 w-4" />}
          {selectedProject ? (
            <span className="hidden max-w-[120px] truncate md:inline">{selectedProject.name}</span>
          ) : null}
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start">
        <DropdownMenuItem onClick={() => onSelect(null)}>
          <Check className={`mr-2 h-4 w-4 ${selectedProjectId ? "invisible" : "visible"}`} />
          <FolderOpen className="mr-2 h-4 w-4" />
          No project
        </DropdownMenuItem>
        {projects.length > 0 ? <DropdownMenuSeparator /> : null}
        {projects.map((project) => (
          <DropdownMenuItem key={project.id} onClick={() => onSelect(project.id)}>
            <Check
              className={`mr-2 h-4 w-4 ${selectedProjectId === project.id ? "visible" : "invisible"}`}
            />
            <Folder className="mr-2 h-4 w-4" />
            {project.name}
          </DropdownMenuItem>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
