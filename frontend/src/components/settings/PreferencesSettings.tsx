import { useEffect, useState } from "react";
import { useBlocker } from "@tanstack/react-router";
import { useOpenSecret } from "@opensecret/react";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import { useSettingsNavigationLock } from "@/contexts/SettingsNavigationLockContext";
import { SettingsPage, SettingsSection } from "./SettingsPage";

export function PreferencesSettings() {
  const os = useOpenSecret();
  const [prompt, setPrompt] = useState("");
  const [instructionId, setInstructionId] = useState<string | null>(null);
  const [isLoading, setIsLoading] = useState(true);
  const [isSaving, setIsSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState(false);

  useBlocker({
    shouldBlockFn: () => isSaving,
    disabled: !isSaving,
    enableBeforeUnload: isSaving
  });
  useSettingsNavigationLock(isSaving);

  useEffect(() => {
    let active = true;

    const loadPreferences = async () => {
      setIsLoading(true);
      setError(null);
      try {
        const response = await os.listInstructions({ limit: 100 });
        if (!active) return;
        const defaultInstruction = response.data.find((instruction) => instruction.is_default);
        setInstructionId(defaultInstruction?.id ?? null);
        setPrompt(defaultInstruction?.prompt ?? "");
      } catch (loadError) {
        console.error("Failed to load preferences:", loadError);
        if (active) setError("Failed to load preferences. Please try again.");
      } finally {
        if (active) setIsLoading(false);
      }
    };

    void loadPreferences();
    return () => {
      active = false;
    };
  }, [os]);

  const handleSubmit = async (event: React.FormEvent) => {
    event.preventDefault();
    setError(null);
    setSuccess(false);
    setIsSaving(true);

    try {
      if (instructionId) {
        if (prompt.trim() === "") {
          await os.deleteInstruction(instructionId);
          setInstructionId(null);
          setPrompt("");
        } else {
          await os.updateInstruction(instructionId, { prompt });
        }
      } else if (prompt.trim() !== "") {
        const newInstruction = await os.createInstruction({
          name: "User Preferences",
          prompt,
          is_default: true
        });
        setInstructionId(newInstruction.id);
      }
      setSuccess(true);
    } catch (saveError) {
      console.error("Failed to save preferences:", saveError);
      setError("Failed to save preferences. Please try again.");
    } finally {
      setIsSaving(false);
    }
  };

  return (
    <SettingsPage
      title="Preferences"
      description="Customize the defaults Maple uses for your AI conversations."
    >
      <SettingsSection
        title="Default system prompt"
        description="This instruction is included by default when you start a conversation."
      >
        <form onSubmit={handleSubmit} className="space-y-4">
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
            <Label htmlFor="settings-system-prompt">System prompt</Label>
            <Textarea
              id="settings-system-prompt"
              value={prompt}
              onChange={(event) => {
                setPrompt(event.target.value);
                setSuccess(false);
                setError(null);
              }}
              placeholder="Enter your custom system prompt here..."
              className="min-h-[240px] resize-y"
              disabled={isLoading}
            />
            <p className="text-xs leading-relaxed text-muted-foreground">
              Leave this empty and save to remove your current default instruction.
            </p>
          </div>
          <div className="flex justify-end">
            <Button type="submit" disabled={isLoading || isSaving || success}>
              {isLoading ? "Loading..." : isSaving ? "Saving..." : "Save preferences"}
            </Button>
          </div>
        </form>
      </SettingsSection>
    </SettingsPage>
  );
}
