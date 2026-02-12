import { useState } from "react";
import {
  CheckSquare,
  ChevronLeft,
  ChevronRight,
  Folder,
  FolderMinus,
  FolderPlus,
  MoreHorizontal,
  Pencil,
  Plus,
  Trash2
} from "lucide-react";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger
} from "@/components/ui/dropdown-menu";

interface ChatContextMenuProps {
  chatId: string;
  isMobile: boolean;
  // All projects for "Move to project" submenu
  projects: { id: string; name: string }[];
  // If set, shows "Remove from {projectName}" and grays out current project in submenu
  currentProjectName?: string;
  currentProjectId?: string;
  // Optional callbacks — item only rendered when provided
  onSelect?: () => void;
  onRename?: () => void;
  onMoveToProject?: (projectId: string) => void;
  onRemoveFromProject?: () => void;
  onDelete?: () => void;
}

export function ChatContextMenu({
  chatId,
  isMobile,
  projects,
  currentProjectName,
  currentProjectId,
  onSelect,
  onRename,
  onMoveToProject,
  onRemoveFromProject,
  onDelete
}: ChatContextMenuProps) {
  const [showProjectSubmenu, setShowProjectSubmenu] = useState(false);

  return (
    <DropdownMenu onOpenChange={(open) => !open && setShowProjectSubmenu(false)}>
      <DropdownMenuTrigger asChild>
        <button
          className={`z-50 bg-background/80 absolute right-2 top-1/2 transform -translate-y-1/2 text-primary transition-opacity p-2 ${
            isMobile ? "opacity-100" : "opacity-0 group-hover:opacity-100"
          }`}
          onClick={(e) => {
            e.preventDefault();
            e.stopPropagation();
          }}
        >
          <MoreHorizontal size={16} />
        </button>
      </DropdownMenuTrigger>
      <DropdownMenuContent className="w-56 overflow-hidden">
        <div className="relative overflow-hidden">
          {/* Main menu layer — in-flow when active, absolute when hidden */}
          <div
            className={`transition-transform duration-300 ease-in-out ${
              showProjectSubmenu
                ? "absolute top-0 left-0 w-full -translate-x-[200%]"
                : "translate-x-0"
            }`}
          >
            {onSelect && (
              <DropdownMenuItem onClick={onSelect}>
                <CheckSquare className="mr-2 h-4 w-4" />
                <span>Select</span>
              </DropdownMenuItem>
            )}
            {onRename && (
              <DropdownMenuItem onClick={onRename}>
                <Pencil className="mr-2 h-4 w-4" />
                <span>Rename chat</span>
              </DropdownMenuItem>
            )}
            {onMoveToProject && (
              <DropdownMenuItem
                onClick={(e) => {
                  e.preventDefault();
                  e.stopPropagation();
                  setShowProjectSubmenu(true);
                }}
                onSelect={(e) => e.preventDefault()}
              >
                <FolderPlus className="mr-2 h-4 w-4" />
                <span className="flex-1">Move to project</span>
                <ChevronRight className="h-4 w-4 ml-auto" />
              </DropdownMenuItem>
            )}
            {onRemoveFromProject && currentProjectName && (
              <DropdownMenuItem onClick={onRemoveFromProject}>
                <FolderMinus className="mr-2 h-4 w-4" />
                <span>Remove from {currentProjectName}</span>
              </DropdownMenuItem>
            )}
            {onDelete && (
              <DropdownMenuItem
                onClick={onDelete}
                className="text-destructive focus:text-destructive"
              >
                <Trash2 className="mr-2 h-4 w-4" />
                <span>Delete chat</span>
              </DropdownMenuItem>
            )}
          </div>

          {/* Project submenu layer — in-flow when active, absolute when hidden */}
          {onMoveToProject && (
            <div
              className={`transition-transform duration-300 ease-in-out ${
                showProjectSubmenu
                  ? "translate-x-0"
                  : "absolute top-0 left-0 w-full translate-x-[200%]"
              }`}
            >
              <DropdownMenuItem
                onClick={(e) => {
                  e.preventDefault();
                  e.stopPropagation();
                  setShowProjectSubmenu(false);
                }}
                onSelect={(e) => e.preventDefault()}
              >
                <ChevronLeft className="mr-2 h-4 w-4" />
                <span>Back</span>
              </DropdownMenuItem>
              <DropdownMenuSeparator />
              <DropdownMenuItem
                onClick={() => {
                  window.dispatchEvent(
                    new CustomEvent("createprojectforchat", {
                      detail: { chatId }
                    })
                  );
                }}
              >
                <Plus className="mr-2 h-4 w-4" />
                <span>New project</span>
              </DropdownMenuItem>
              {projects.length > 0 && <DropdownMenuSeparator />}
              <div className="max-h-[40vh] overflow-y-auto">
                {projects.map((project) => {
                  const isCurrent = project.id === currentProjectId;
                  return (
                    <DropdownMenuItem
                      key={project.id}
                      onClick={() => !isCurrent && onMoveToProject(project.id)}
                      onSelect={(e) => isCurrent && e.preventDefault()}
                      disabled={isCurrent}
                    >
                      <Folder className="mr-2 h-4 w-4" />
                      <span className="truncate">{project.name}</span>
                    </DropdownMenuItem>
                  );
                })}
              </div>
            </div>
          )}
        </div>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
