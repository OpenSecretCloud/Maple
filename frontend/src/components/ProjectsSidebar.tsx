// No need for useState in this component
import { useLocalState } from "@/state/useLocalState";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { Folder, MoreHorizontal, Trash, FolderPlus } from "lucide-react";
import { Button } from "./ui/button";
import { Project } from "@/state/LocalStateContext";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger
} from "@/components/ui/dropdown-menu";
import { cn } from "@/utils/utils";

interface ProjectsSidebarProps {
  currentProjectId?: string;
  onProjectSelect: (projectId: string) => void;
  onCreateProject: () => void;
}

export function ProjectsSidebar({
  currentProjectId,
  onProjectSelect,
  onCreateProject
}: ProjectsSidebarProps) {
  const { getProjects, deleteProject } = useLocalState();
  const queryClient = useQueryClient();
  const navigate = useNavigate();

  const {
    isPending,
    error,
    data: projects = []
  } = useQuery({
    queryKey: ["projects"],
    queryFn: getProjects
  });

  const handleDeleteProject = async (projectId: string) => {
    try {
      await deleteProject(projectId);
      queryClient.invalidateQueries({ queryKey: ["projects"] });
      queryClient.invalidateQueries({ queryKey: ["chatHistory"] });

      // If we're on the deleted project's page, navigate to root
      if (projectId === currentProjectId) {
        navigate({ to: "/" });
      }
    } catch (error) {
      console.error("Error deleting project:", error);
    }
  };

  if (isPending) {
    return <div className="px-2 py-2 text-sm">Loading projects...</div>;
  }

  if (error) {
    return <div className="px-2 py-2 text-sm text-destructive">Error: {error.message}</div>;
  }

  if (projects.length === 0) {
    return (
      <div className="px-2 py-2 text-sm">
        <p className="text-muted-foreground mb-2">No projects yet</p>
        <Button
          variant="outline"
          size="sm"
          className="w-full justify-start gap-2"
          onClick={onCreateProject}
        >
          <FolderPlus className="h-4 w-4" />
          <span>Create Project</span>
        </Button>
      </div>
    );
  }

  return (
    <div className="space-y-1">
      {projects.map((project) => (
        <ProjectItem
          key={project.id}
          project={project}
          isActive={project.id === currentProjectId}
          onSelect={() => onProjectSelect(project.id)}
          onDelete={() => handleDeleteProject(project.id)}
        />
      ))}
    </div>
  );
}

interface ProjectItemProps {
  project: Project;
  isActive: boolean;
  onSelect: () => void;
  onDelete: () => void;
}

function ProjectItem({ project, isActive, onSelect, onDelete }: ProjectItemProps) {
  return (
    <div className="relative group">
      <button
        onClick={onSelect}
        className={cn(
          "w-full flex items-center gap-2 px-2 py-1.5 text-sm rounded-md transition-colors",
          isActive
            ? "bg-accent text-accent-foreground"
            : "text-muted-foreground hover:text-foreground hover:bg-accent/50"
        )}
      >
        <Folder className="h-4 w-4 shrink-0" />
        <span className="truncate">{project.name}</span>
      </button>

      <div className="absolute right-2 top-1/2 transform -translate-y-1/2 opacity-0 group-hover:opacity-100 transition-opacity">
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button variant="ghost" size="sm" className="h-6 w-6 p-0">
              <MoreHorizontal className="h-4 w-4" />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end">
            <DropdownMenuItem onClick={onDelete}>
              <Trash className="mr-2 h-4 w-4" />
              <span>Delete Project</span>
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
    </div>
  );
}
