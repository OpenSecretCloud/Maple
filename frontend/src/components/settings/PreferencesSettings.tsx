import { useEffect, useState } from "react";
import { useBlocker } from "@tanstack/react-router";
import { useOpenSecret } from "@opensecret/react";
import { RotateCcw } from "lucide-react";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectLabel,
  SelectTrigger,
  SelectValue
} from "@/components/ui/select";
import { Textarea } from "@/components/ui/textarea";
import { useSettingsNavigationLock } from "@/contexts/SettingsNavigationLockContext";
import { useTTS } from "@/services/tts/TTSContext";
import {
  isVoxtralTTSVoice,
  TTS_MAX_PLAYBACK_SPEED,
  TTS_MIN_PLAYBACK_SPEED,
  TTS_PLAYBACK_SPEED_STEP,
  VOXTRAL_TTS_VOICE_OPTIONS
} from "@/services/tts/ttsPreferences";
import { SettingsPage, SettingsSection } from "./SettingsPage";

const TTS_VOICE_GROUPS = ["Default voices", "Reference accents"] as const;

function formatPlaybackSpeed(speed: number): string {
  return `${speed.toFixed(1)}x`;
}

export function PreferencesSettings() {
  const os = useOpenSecret();
  const {
    playbackSpeed,
    hasCustomPlaybackSpeed,
    voice,
    setPlaybackSpeed,
    resetPlaybackSpeed,
    setVoice
  } = useTTS();
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
      <SettingsSection
        title="Text-to-speech"
        description="Choose how Maple reads assistant responses aloud on this device."
      >
        <div className="space-y-6">
          <div className="grid gap-2">
            <Label htmlFor="settings-tts-voice">Voice accent</Label>
            <Select
              value={voice}
              onValueChange={(value) => {
                if (isVoxtralTTSVoice(value)) {
                  setVoice(value);
                }
              }}
            >
              <SelectTrigger id="settings-tts-voice">
                <SelectValue placeholder="Select a voice" />
              </SelectTrigger>
              <SelectContent>
                {TTS_VOICE_GROUPS.map((group) => (
                  <SelectGroup key={group}>
                    <SelectLabel>{group}</SelectLabel>
                    {VOXTRAL_TTS_VOICE_OPTIONS.filter((option) => option.group === group).map(
                      (option) => (
                        <SelectItem key={option.value} value={option.value}>
                          {option.label}
                        </SelectItem>
                      )
                    )}
                  </SelectGroup>
                ))}
              </SelectContent>
            </Select>
            <p className="text-xs leading-relaxed text-muted-foreground">
              Any voice can read any supported language; your text determines the spoken language.
              Matching the voice accent to the text generally sounds most natural.
            </p>
          </div>

          <div className="space-y-3">
            <div className="flex items-center justify-between gap-3">
              <Label htmlFor="settings-tts-speech-speed">Speech speed</Label>
              <output
                htmlFor="settings-tts-speech-speed"
                className="text-sm tabular-nums text-muted-foreground"
              >
                {formatPlaybackSpeed(playbackSpeed)}
              </output>
            </div>
            <input
              id="settings-tts-speech-speed"
              type="range"
              min={TTS_MIN_PLAYBACK_SPEED}
              max={TTS_MAX_PLAYBACK_SPEED}
              step={TTS_PLAYBACK_SPEED_STEP}
              value={playbackSpeed}
              onChange={(event) => setPlaybackSpeed(Number(event.currentTarget.value))}
              className="h-2 w-full cursor-pointer accent-primary"
              aria-valuetext={formatPlaybackSpeed(playbackSpeed)}
            />
            <div className="flex items-center justify-between text-xs text-muted-foreground">
              <span>{formatPlaybackSpeed(TTS_MIN_PLAYBACK_SPEED)}</span>
              <span>{formatPlaybackSpeed(TTS_MAX_PLAYBACK_SPEED)}</span>
            </div>
            <div className="flex items-center justify-between gap-4">
              <p className="text-xs leading-relaxed text-muted-foreground">
                Speech is generated at this speed to preserve the selected voice&apos;s pitch.
              </p>
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={resetPlaybackSpeed}
                disabled={!hasCustomPlaybackSpeed}
                className="shrink-0 gap-2"
              >
                <RotateCcw className="h-4 w-4" aria-hidden="true" />
                Reset
              </Button>
            </div>
          </div>

          <p className="text-xs leading-relaxed text-muted-foreground">
            Changes are saved automatically on this device.
          </p>
        </div>
      </SettingsSection>
    </SettingsPage>
  );
}
