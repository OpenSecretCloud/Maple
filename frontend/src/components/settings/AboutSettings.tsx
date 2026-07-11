import { Link } from "@tanstack/react-router";
import { ExternalLink, FileText, Info, Mail, Shield } from "lucide-react";
import packageJson from "../../../package.json";
import { Button } from "@/components/ui/button";
import { openExternalUrl } from "@/utils/openUrl";
import { SettingsPage, SettingsSection } from "./SettingsPage";

type ExternalRowProps = {
  label: string;
  url: string;
  icon: typeof Shield;
};

function ExternalRow({ label, url, icon: Icon }: ExternalRowProps) {
  return (
    <button
      type="button"
      onClick={() => void openExternalUrl(url)}
      className="flex w-full items-center gap-3 rounded-lg border border-border/70 p-3 text-left transition-colors hover:border-[hsl(var(--maple-primary))]/60 hover:bg-muted/50 sm:p-4"
    >
      <Icon className="h-4 w-4 shrink-0 text-muted-foreground" />
      <span className="min-w-0 flex-1 text-sm font-medium">{label}</span>
      <ExternalLink className="h-4 w-4 shrink-0 text-muted-foreground" />
    </button>
  );
}

export function AboutSettings() {
  return (
    <SettingsPage title="About" description="Maple information, policies, and support.">
      <SettingsSection title="Maple Research">
        <div className="flex flex-col gap-4 sm:flex-row sm:items-center sm:justify-between">
          <div>
            <p className="text-sm leading-relaxed text-muted-foreground">
              A private AI workspace built for secure research and collaboration.
            </p>
            <p className="mt-2 text-xs text-muted-foreground">Version {packageJson.version}</p>
          </div>
          <Button asChild variant="outline">
            <Link to="/about">
              <Info className="mr-2 h-4 w-4" />
              About Maple
            </Link>
          </Button>
        </div>
      </SettingsSection>

      <SettingsSection title="Policies and support">
        <div className="space-y-2">
          <ExternalRow label="Privacy policy" url="https://trymaple.ai/privacy" icon={Shield} />
          <ExternalRow label="Terms of service" url="https://trymaple.ai/terms" icon={FileText} />
          <ExternalRow label="Contact us" url="mailto:support@trymaple.ai" icon={Mail} />
        </div>
      </SettingsSection>
    </SettingsPage>
  );
}
