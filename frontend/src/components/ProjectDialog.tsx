import { useState, useEffect } from "react";
import { useLocalState } from "@/state/useLocalState";
import { useQueryClient } from "@tanstack/react-query";
import { Project } from "@/state/LocalStateContext";
import { Button } from "./ui/button";
import { Input } from "./ui/input";
import { Textarea } from "./ui/textarea";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle
} from "./ui/dialog";

interface ProjectDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  project?: Project;
  onSuccess?: (projectId: string) => void;
}

export function ProjectDialog({ open, onOpenChange, project, onSuccess }: ProjectDialogProps) {
  const { createProject, updateProject } = useLocalState();
  const queryClient = useQueryClient();

  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [systemPrompt, setSystemPrompt] = useState("");
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [error, setError] = useState("");

  // Reset form when dialog opens/closes or project changes
  useEffect(() => {
    if (open && project) {
      setName(project.name);
      setDescription(project.description || "");
      setSystemPrompt(project.systemPrompt || "");
    } else if (open) {
      setName("");
      setDescription("");
      setSystemPrompt("");
    }
    setError("");
  }, [open, project]);

  const handleSubmit = async () => {
    if (!name.trim()) {
      setError("Project name is required");
      return;
    }

    setIsSubmitting(true);
    setError("");

    try {
      let projectId: string;

      if (project) {
        // Update existing project
        const updatedProject = {
          ...project,
          name,
          description: description.trim() || undefined,
          systemPrompt: systemPrompt.trim() || undefined
        };
        await updateProject(updatedProject);
        projectId = project.id;
      } else {
        // Create new project
        const newProject = await createProject(
          name.trim(),
          description.trim() || undefined,
          systemPrompt.trim() || undefined
        );
        projectId = newProject.id;
      }

      // Invalidate projects query to refresh the list
      queryClient.invalidateQueries({ queryKey: ["projects"] });

      // Close dialog and call success callback if provided
      onOpenChange(false);
      if (onSuccess) {
        onSuccess(projectId);
      }
    } catch (error) {
      console.error("Error saving project:", error);
      setError(error instanceof Error ? error.message : "Failed to save project");
    } finally {
      setIsSubmitting(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>{project ? "Edit Project" : "Create New Project"}</DialogTitle>
          <DialogDescription>
            {project
              ? "Edit your project details and settings."
              : "Create a new project to organize related chats."}
          </DialogDescription>
        </DialogHeader>

        <div className="grid gap-4 py-4">
          <div className="grid gap-2">
            <label htmlFor="project-name" className="text-sm font-medium">
              Project Name
            </label>
            <Input
              id="project-name"
              placeholder="Enter project name"
              value={name}
              onChange={(e) => setName(e.target.value)}
            />
          </div>

          <div className="grid gap-2">
            <label htmlFor="project-description" className="text-sm font-medium">
              Description (optional)
            </label>
            <Textarea
              id="project-description"
              placeholder="Enter a description for this project"
              value={description}
              onChange={(e) => setDescription(e.target.value)}
              rows={2}
            />
          </div>

          <div className="grid gap-2">
            <label htmlFor="system-prompt" className="text-sm font-medium">
              System Prompt (optional)
            </label>
            <Textarea
              id="system-prompt"
              placeholder="Set a default system prompt for this project"
              value={systemPrompt}
              onChange={(e) => setSystemPrompt(e.target.value)}
              rows={4}
            />
            <p className="text-xs text-muted-foreground">
              This prompt will be sent at the beginning of every conversation in this project.
            </p>
          </div>

          {error && <p className="text-sm text-destructive">{error}</p>}
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button onClick={handleSubmit} disabled={isSubmitting}>
            {isSubmitting ? "Saving..." : project ? "Save Changes" : "Create Project"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
