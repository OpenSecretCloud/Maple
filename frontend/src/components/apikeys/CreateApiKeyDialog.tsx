import { useState } from "react";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
  DialogFooter
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Copy, CheckCircle, Loader2, AlertCircle } from "lucide-react";
import { useOpenSecret } from "@opensecret/react";

interface CreateApiKeyDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onKeyCreated: () => void;
}

export function CreateApiKeyDialog({ open, onOpenChange, onKeyCreated }: CreateApiKeyDialogProps) {
  const [keyName, setKeyName] = useState("");
  const [isCreating, setIsCreating] = useState(false);
  const [createdKey, setCreatedKey] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const { createApiKey } = useOpenSecret();

  const handleCreate = async () => {
    const trimmedName = keyName.trim();

    // Validation
    if (!trimmedName) {
      setError("Please enter a name for your API key");
      return;
    }

    if (trimmedName.length > 100) {
      setError("API key name must be 100 characters or less");
      return;
    }

    setIsCreating(true);
    setError(null);

    try {
      const response = await createApiKey(trimmedName);
      setCreatedKey(response.key);
      console.log("API key created successfully");
    } catch (error) {
      console.error("Failed to create API key:", error);
      setError(error instanceof Error ? error.message : "Failed to create API key");
    } finally {
      setIsCreating(false);
    }
  };

  const handleCopy = async () => {
    if (!createdKey) return;

    try {
      await navigator.clipboard.writeText(createdKey);
      setCopied(true);
      console.log("API key copied to clipboard");
      setTimeout(() => setCopied(false), 2000);
    } catch (error) {
      console.error("Failed to copy:", error);
    }
  };

  const handleClose = () => {
    if (createdKey) {
      onKeyCreated();
    }
    // Reset state
    setKeyName("");
    setCreatedKey(null);
    setCopied(false);
    setError(null);
    onOpenChange(false);
  };

  return (
    <Dialog
      open={open}
      onOpenChange={(newOpen) => {
        if (!newOpen) {
          handleClose();
        }
      }}
    >
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>Create API Key</DialogTitle>
          <DialogDescription>
            {createdKey
              ? "Your API key has been created. Copy it now - you won't be able to see it again."
              : "Create a new API key for programmatic access to Maple."}
          </DialogDescription>
        </DialogHeader>

        {!createdKey ? (
          <>
            <div className="space-y-4">
              <div className="space-y-2">
                <Label htmlFor="key-name">Key Name</Label>
                <Input
                  id="key-name"
                  placeholder="e.g., Production App, Development"
                  value={keyName}
                  onChange={(e) => setKeyName(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter" && !isCreating) {
                      handleCreate();
                    }
                  }}
                  disabled={isCreating}
                  maxLength={100}
                />
                <div className="flex items-center justify-between text-xs">
                  <span className="text-muted-foreground">
                    {keyName.trim().length}/100 characters
                  </span>
                  {error && <span className="text-destructive">{error}</span>}
                </div>
              </div>
            </div>

            <DialogFooter>
              <Button variant="outline" onClick={handleClose} disabled={isCreating}>
                Cancel
              </Button>
              <Button onClick={handleCreate} disabled={isCreating || !keyName.trim()}>
                {isCreating ? (
                  <>
                    <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                    Creating...
                  </>
                ) : (
                  "Create API Key"
                )}
              </Button>
            </DialogFooter>
          </>
        ) : (
          <>
            <div className="space-y-4">
              <Alert className="border-amber-500/50 bg-amber-500/10">
                <AlertCircle className="h-4 w-4 text-amber-500" />
                <AlertDescription className="text-sm">
                  Make sure to copy your API key now. You won't be able to see it again!
                </AlertDescription>
              </Alert>

              <div className="space-y-2">
                <Label>Your API Key</Label>
                <div className="flex gap-2">
                  <Input
                    value={createdKey}
                    readOnly
                    className="font-mono text-xs"
                    onClick={(e) => e.currentTarget.select()}
                  />
                  <Button size="icon" variant="outline" onClick={handleCopy} className="shrink-0">
                    {copied ? (
                      <CheckCircle className="h-4 w-4 text-green-500" />
                    ) : (
                      <Copy className="h-4 w-4" />
                    )}
                  </Button>
                </div>
              </div>

              <div className="text-xs text-muted-foreground space-y-1">
                <p>Use this key to authenticate with the Maple API:</p>
                <code className="block bg-muted p-2 rounded text-xs">
                  Authorization: Bearer {"{your-api-key}"}
                </code>
              </div>
            </div>

            <DialogFooter>
              <Button onClick={handleClose} className="w-full">
                Done
              </Button>
            </DialogFooter>
          </>
        )}
      </DialogContent>
    </Dialog>
  );
}
