import { Loader2, ShieldCheck } from "lucide-react";

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
import { Switch } from "@/components/ui/switch";
import type { AgentProjectTrustStatus } from "@/services/agentRuntimeService";

interface AgentProjectSettingsDialogProps {
  open: boolean;
  projectName: string;
  projectPath: string;
  trustStatus: AgentProjectTrustStatus | null;
  isSaving: boolean;
  error: string | null;
  onOpenChange: (open: boolean) => void;
  onTrustChange: (trusted: boolean) => void;
}

export function AgentProjectSettingsDialog({
  open,
  projectName,
  projectPath,
  trustStatus,
  isSaving,
  error,
  onOpenChange,
  onTrustChange
}: AgentProjectSettingsDialogProps) {
  const isLoading = trustStatus === null && error === null;
  const trustEnabled = trustStatus?.decision === true;
  const hasProjectSkills = trustStatus?.protectedFeatures.includes("skills") ?? false;

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-[500px]">
        <DialogHeader>
          <DialogTitle>{projectName} Settings</DialogTitle>
          <DialogDescription>
            Configure how Maple uses guidance supplied by this project. Trust is remembered for the
            canonical folder and can be changed later.
          </DialogDescription>
        </DialogHeader>

        <div className="grid gap-4 py-2">
          <div className="rounded-md border bg-muted/40 px-3 py-2 font-mono text-xs break-all">
            {projectPath}
          </div>

          <div className="flex items-start justify-between gap-4 rounded-xl border bg-card p-4">
            <div className="flex min-w-0 gap-3">
              <div className="mt-0.5 flex h-8 w-8 shrink-0 items-center justify-center rounded-lg bg-muted">
                <ShieldCheck className="h-4 w-4" />
              </div>
              <div className="min-w-0 space-y-1">
                <Label htmlFor="agent-project-trust" className="text-sm font-medium">
                  Trust this project
                </Label>
                <p id="agent-project-trust-description" className="text-xs text-muted-foreground">
                  Allows Maple to use project-provided guidance, including agent skills. These
                  instructions can influence how agents work and use tools; normal tool permissions
                  still apply.
                </p>
                {trustStatus && !hasProjectSkills ? (
                  <p className="text-xs text-muted-foreground">
                    No project-provided skills are currently detected. This decision will also apply
                    to trusted project capabilities Maple adds in the future.
                  </p>
                ) : null}
              </div>
            </div>
            {isLoading ? (
              <Loader2 className="mt-1 h-4 w-4 shrink-0 animate-spin text-muted-foreground" />
            ) : (
              <Switch
                id="agent-project-trust"
                checked={trustEnabled}
                onCheckedChange={onTrustChange}
                disabled={isSaving || !trustStatus?.available}
                aria-describedby="agent-project-trust-description"
                aria-label={`${trustEnabled ? "Disable" : "Enable"} trust for ${projectName}`}
              />
            )}
          </div>

          {error ? (
            <p className="text-sm text-destructive" role="alert" aria-live="assertive">
              {error}
            </p>
          ) : null}
        </div>

        <DialogFooter>
          <Button type="button" variant="outline" onClick={() => onOpenChange(false)}>
            Done
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
