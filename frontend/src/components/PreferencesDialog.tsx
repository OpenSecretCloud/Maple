import { useState, useEffect } from "react";
import { useOpenSecret } from "@opensecret/react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle
} from "@/components/ui/dialog";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import { Alert, AlertDescription } from "@/components/ui/alert";

interface PreferencesDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

export function PreferencesDialog({ open, onOpenChange }: PreferencesDialogProps) {
  const os = useOpenSecret();
  const [prompt, setPrompt] = useState("");
  const [instructionId, setInstructionId] = useState<string | null>(null);
  const [isLoading, setIsLoading] = useState(false);
  const [isSaving, setIsSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState(false);

  useEffect(() => {
    if (open) {
      loadPreferences();
    } else {
      setError(null);
      setSuccess(false);
    }
  }, [open]);

  const loadPreferences = async () => {
    setIsLoading(true);
    setError(null);
    try {
      const response = await os.listInstructions({ limit: 100 });
      const defaultInstruction = response.data.find((inst) => inst.is_default);

      if (defaultInstruction) {
        setInstructionId(defaultInstruction.id);
        setPrompt(defaultInstruction.prompt);
      } else {
        // No default instruction exists yet
        setInstructionId(null);
        setPrompt("");
      }
    } catch (error) {
      console.error("Failed to load preferences:", error);
      setError("Failed to load preferences. Please try again.");
    } finally {
      setIsLoading(false);
    }
  };

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setError(null);
    setSuccess(false);

    setIsSaving(true);
    try {
      if (instructionId) {
        // Update existing instruction
        await os.updateInstruction(instructionId, {
          prompt: prompt
        });
      } else {
        // Create new instruction
        const newInstruction = await os.createInstruction({
          name: "User Preferences",
          prompt: prompt,
          is_default: true
        });
        setInstructionId(newInstruction.id);
      }
      setSuccess(true);
    } catch (error) {
      console.error("Failed to save preferences:", error);
      setError("Failed to save preferences. Please try again.");
    } finally {
      setIsSaving(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-[525px]">
        <DialogHeader>
          <DialogTitle>User Preferences</DialogTitle>
          <DialogDescription>
            Customize your default system prompt for AI conversations.
          </DialogDescription>
        </DialogHeader>
        <form onSubmit={handleSubmit} className="grid gap-4 py-4">
          {error && (
            <Alert variant="destructive">
              <AlertDescription>{error}</AlertDescription>
            </Alert>
          )}
          {success && (
            <Alert>
              <AlertDescription>Preferences saved successfully.</AlertDescription>
            </Alert>
          )}
          <div className="grid gap-2">
            <Label htmlFor="prompt">System Prompt</Label>
            <Textarea
              id="prompt"
              value={prompt}
              onChange={(e) => setPrompt(e.target.value)}
              placeholder="Enter your custom system prompt here..."
              className="min-h-[200px] resize-y"
              disabled={isLoading}
            />
            <p className="text-sm text-muted-foreground">
              This prompt will be used as the default instruction for your AI conversations.
            </p>
          </div>
          <DialogFooter>
            <Button type="submit" disabled={isLoading || isSaving || success}>
              {isLoading ? "Loading..." : isSaving ? "Saving..." : "Save Preferences"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
