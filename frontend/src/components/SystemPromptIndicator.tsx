import { useState, useCallback } from "react";
import { Bot, Copy, Check } from "lucide-react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
  DialogTrigger
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { cn } from "@/utils/utils";

interface SystemPromptIndicatorProps {
  systemPrompt?: string;
  className?: string;
}

export function SystemPromptIndicator({ systemPrompt, className }: SystemPromptIndicatorProps) {
  const [isCopied, setIsCopied] = useState(false);
  const [isOpen, setIsOpen] = useState(false);

  const handleCopy = useCallback(async () => {
    if (!systemPrompt) return;

    try {
      await navigator.clipboard.writeText(systemPrompt);
      setIsCopied(true);
      setTimeout(() => setIsCopied(false), 2000);
    } catch (error) {
      console.error("Failed to copy system prompt:", error);
    }
  }, [systemPrompt]);

  // Don't render if there's no system prompt
  if (!systemPrompt?.trim()) {
    return null;
  }

  return (
    <Dialog open={isOpen} onOpenChange={setIsOpen}>
      <DialogTrigger asChild>
        <button
          className={cn(
            "inline-flex items-center gap-1 px-2 py-1 rounded-md",
            "text-xs text-muted-foreground/70 hover:text-muted-foreground",
            "bg-muted/30 hover:bg-muted/50 transition-colors",
            "border border-muted-foreground/10",
            className
          )}
          title="View system prompt"
        >
          <Bot className="h-3 w-3" />
          <span>System</span>
        </button>
      </DialogTrigger>
      <DialogContent className="max-w-2xl max-h-[80vh] overflow-y-auto">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Bot className="h-4 w-4" />
            System Prompt
          </DialogTitle>
          <DialogDescription>
            The system prompt that was used for this conversation
          </DialogDescription>
        </DialogHeader>
        <div className="space-y-4">
          <div className="bg-muted/50 rounded-lg p-4 font-mono text-sm whitespace-pre-wrap">
            {systemPrompt}
          </div>
          <div className="flex justify-end">
            <Button onClick={handleCopy} variant="outline" size="sm" className="gap-2">
              {isCopied ? (
                <>
                  <Check className="h-4 w-4" />
                  Copied!
                </>
              ) : (
                <>
                  <Copy className="h-4 w-4" />
                  Copy System Prompt
                </>
              )}
            </Button>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}
