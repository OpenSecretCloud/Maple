import { useState } from "react";
import { useLocalState } from "@/state/useLocalState";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Button } from "./ui/button";
import { Input } from "./ui/input";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle
} from "./ui/dialog";
import { FolderPlus } from "lucide-react";
import { ScrollArea } from "./ui/scroll-area";
import { RadioGroup, RadioGroupItem } from "./ui/radio-group";
import { Label } from "./ui/label";
import { Project } from "@/state/LocalStateContext";

interface AddToProjectDialogProps {
  chatId: string;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

export function AddToProjectDialog({ chatId, open, onOpenChange }: AddToProjectDialogProps) {
  const { getProjects, createProject, addChatToProject } = useLocalState();
  const queryClient = useQueryClient();
  const [newProjectName, setNewProjectName] = useState("");
  const [selectedProjectId, setSelectedProjectId] = useState<string>("");
  const [isCreatingProject, setIsCreatingProject] = useState(false);
  const [error, setError] = useState("");

  const { data: projects = [] } = useQuery({
    queryKey: ["projects"],
    queryFn: getProjects
  });

  const handleCreateProject = async () => {
    if (!newProjectName.trim()) return;

    try {
      setError("");
      const project = await createProject(newProjectName.trim());
      setNewProjectName("");
      setSelectedProjectId(project.id);
      queryClient.invalidateQueries({ queryKey: ["projects"] });
    } catch (error) {
      console.error("Error creating project:", error);
      setError("Failed to create project");
    }
  };

  const handleAddToProject = async () => {
    if (!selectedProjectId) {
      setError("Please select a project");
      return;
    }

    try {
      setError("");
      await addChatToProject(chatId, selectedProjectId);
      queryClient.invalidateQueries({ queryKey: ["chatHistory"] });
      queryClient.invalidateQueries({ queryKey: ["chat", chatId] });
      onOpenChange(false);
    } catch (error) {
      console.error("Error adding chat to project:", error);
      setError("Failed to add chat to project");
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>Add to Project</DialogTitle>
          <DialogDescription>
            Choose an existing project or create a new one to organize your chats.
          </DialogDescription>
        </DialogHeader>

        <div className="flex flex-col gap-4 py-2">
          {/* Existing Projects */}
          {projects.length > 0 ? (
            <ScrollArea className="h-[200px] rounded-md border p-4">
              <RadioGroup value={selectedProjectId} onValueChange={setSelectedProjectId}>
                {projects.map((project: Project) => (
                  <div key={project.id} className="flex items-center space-x-2 mb-3">
                    <RadioGroupItem value={project.id} id={project.id} />
                    <Label htmlFor={project.id}>{project.name}</Label>
                  </div>
                ))}
              </RadioGroup>
            </ScrollArea>
          ) : (
            <div className="text-center py-4 text-muted-foreground">
              No projects yet. Create your first project below.
            </div>
          )}

          {/* Create New Project */}
          {isCreatingProject ? (
            <div className="flex gap-2">
              <Input
                placeholder="Enter project name"
                value={newProjectName}
                onChange={(e) => setNewProjectName(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") {
                    handleCreateProject();
                  }
                }}
              />
              <Button onClick={handleCreateProject} disabled={!newProjectName.trim()}>
                Create
              </Button>
            </div>
          ) : (
            <Button variant="outline" className="gap-2" onClick={() => setIsCreatingProject(true)}>
              <FolderPlus className="h-4 w-4" />
              New Project
            </Button>
          )}

          {error && <p className="text-sm text-destructive">{error}</p>}
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button onClick={handleAddToProject} disabled={!selectedProjectId}>
            Add to Project
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
