import { useState, useEffect } from "react";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
  DialogFooter
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";

interface CustomInstructionsDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  currentInstructions: string;
  onSave: (instructions: string) => void;
}

export function CustomInstructionsDialog({
  open,
  onOpenChange,
  currentInstructions,
  onSave
}: CustomInstructionsDialogProps) {
  const [instructions, setInstructions] = useState("");

  useEffect(() => {
    if (open) {
      setInstructions(currentInstructions);
    }
  }, [open, currentInstructions]);

  const handleSave = () => {
    onSave(instructions);
    onOpenChange(false);
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-[525px]">
        <DialogHeader>
          <DialogTitle>Custom instructions</DialogTitle>
          <DialogDescription>
            Set context and customize how Maple responds in this project.
          </DialogDescription>
        </DialogHeader>
        <div className="py-4">
          <Textarea
            value={instructions}
            onChange={(e) => setInstructions(e.target.value)}
            placeholder="e.g., 'Use concise bullet points. Focus on practical examples.'"
            className="min-h-[200px] resize-y"
            autoFocus
          />
        </div>
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button onClick={handleSave}>Save</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
