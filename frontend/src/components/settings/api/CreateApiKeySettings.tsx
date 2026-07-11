import { useState } from "react";
import { useBlocker, useNavigate } from "@tanstack/react-router";
import { useQueryClient } from "@tanstack/react-query";
import { useOpenSecret } from "@opensecret/react";
import { AlertCircle, CheckCircle, Copy, Loader2 } from "lucide-react";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { useSettingsNavigationLock } from "@/contexts/SettingsNavigationLockContext";
import { SettingsSection } from "../SettingsPage";

export function CreateApiKeySettings() {
  const { createApiKey } = useOpenSecret();
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const [keyName, setKeyName] = useState("");
  const [createdKey, setCreatedKey] = useState<string | null>(null);
  const [isCreating, setIsCreating] = useState(false);
  const [copied, setCopied] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useBlocker({
    shouldBlockFn: () => isCreating || createdKey !== null,
    disabled: !isCreating && createdKey === null,
    enableBeforeUnload: isCreating || createdKey !== null
  });
  useSettingsNavigationLock(isCreating || createdKey !== null);

  const handleCreate = async () => {
    const trimmedName = keyName.trim();

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
    } catch (createFailure) {
      console.error("Failed to create API key:", createFailure);
      const apiError = createFailure as { status?: number; message?: string };
      if (apiError.status === 409 || apiError.message?.toLowerCase().includes("conflict")) {
        setError(`An API key named "${trimmedName}" already exists. Choose a different name.`);
      } else if (apiError.status === 400 || apiError.message?.toLowerCase().includes("invalid")) {
        setError(
          "Invalid API key name. Use only letters, numbers, spaces, hyphens, and underscores."
        );
      } else if (
        apiError.status === 401 ||
        apiError.message?.toLowerCase().includes("unauthorized")
      ) {
        setError("You are not authorized to create API keys. Check your subscription plan.");
      } else if (apiError.status === 429 || apiError.message?.toLowerCase().includes("limit")) {
        setError("You have reached the API key limit. Delete an existing key first.");
      } else {
        setError(apiError.message || "Failed to create API key. Please try again.");
      }
    } finally {
      setIsCreating(false);
    }
  };

  const handleCopy = async () => {
    if (!createdKey) return;

    try {
      await navigator.clipboard.writeText(createdKey);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch (copyFailure) {
      console.error("Failed to copy API key:", copyFailure);
    }
  };

  const handleDone = async () => {
    await queryClient.invalidateQueries({ queryKey: ["apiKeys"] });
    await navigate({ to: "/settings/api/keys", replace: true, ignoreBlocker: true });
  };

  return (
    <SettingsSection
      title={createdKey ? "Save your API key" : "Create API key"}
      description={
        createdKey
          ? "This is the only time Maple will show the complete key. Copy it before selecting Done."
          : "Name the application or workflow that will use this key."
      }
    >
      {!createdKey ? (
        <div className="space-y-4">
          {error && (
            <Alert variant="destructive">
              <AlertCircle className="h-4 w-4" />
              <AlertDescription>{error}</AlertDescription>
            </Alert>
          )}

          <div className="grid gap-2">
            <Label htmlFor="settings-api-key-name">Key name</Label>
            <Input
              id="settings-api-key-name"
              value={keyName}
              onChange={(event) => {
                setKeyName(event.target.value);
                setError(null);
              }}
              onKeyDown={(event) => {
                if (event.key === "Enter" && !isCreating) {
                  event.preventDefault();
                  void handleCreate();
                }
              }}
              placeholder="e.g. Production App, Development"
              maxLength={100}
              disabled={isCreating}
              autoFocus
            />
            <span className="text-xs text-muted-foreground">
              {keyName.trim().length}/100 characters
            </span>
          </div>

          <div className="flex flex-col-reverse gap-2 sm:flex-row sm:justify-end">
            <Button
              type="button"
              variant="outline"
              onClick={() => navigate({ to: "/settings/api/keys" })}
              disabled={isCreating}
            >
              Cancel
            </Button>
            <Button type="button" onClick={handleCreate} disabled={isCreating || !keyName.trim()}>
              {isCreating && <Loader2 className="mr-2 h-4 w-4 animate-spin" />}
              {isCreating ? "Creating..." : "Create API key"}
            </Button>
          </div>
        </div>
      ) : (
        <div className="space-y-4">
          <Alert className="border-maple-warning/40 bg-maple-warning/10">
            <AlertCircle className="h-4 w-4 text-maple-warning" />
            <AlertDescription>
              Copy this key now. You will not be able to view it again after selecting Done.
            </AlertDescription>
          </Alert>

          <div className="grid gap-2">
            <Label htmlFor="settings-created-api-key">Your API key</Label>
            <div className="flex gap-2">
              <Input
                id="settings-created-api-key"
                value={createdKey}
                readOnly
                className="min-w-0 font-mono text-xs"
                onClick={(event) => event.currentTarget.select()}
              />
              <Button
                type="button"
                size="icon"
                variant="outline"
                onClick={handleCopy}
                className="shrink-0"
                aria-label={copied ? "API key copied" : "Copy API key"}
              >
                {copied ? (
                  <CheckCircle className="h-4 w-4 text-maple-success" />
                ) : (
                  <Copy className="h-4 w-4" />
                )}
              </Button>
            </div>
          </div>

          <div className="flex justify-end">
            <Button type="button" onClick={handleDone}>
              Done
            </Button>
          </div>
        </div>
      )}
    </SettingsSection>
  );
}
