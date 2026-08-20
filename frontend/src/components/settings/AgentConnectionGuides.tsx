import { type ReactNode, useState } from "react";
import { AlertCircle, Check, Copy, Server, ShieldCheck, Terminal } from "lucide-react";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import {
  BUZZ_DEFAULT_AGENT_PARALLELISM,
  BUZZ_MAPLE_AGENT_PARALLELISM,
  BUZZ_MAPLE_HARNESS_ID,
  BUZZ_MAPLE_HARNESS_NAME,
  PASEO_MAPLE_PROVIDER_ID,
  serializeBuzzCustomHarness,
  serializePaseoCustomProviderConfig,
  type MapleAcpStatus
} from "@/services/mapleAcpService";

function CopyButton({ value, label }: { value: string; label: string }) {
  const [copyState, setCopyState] = useState<"idle" | "copied" | "error">("idle");

  const copyValue = async () => {
    try {
      await navigator.clipboard.writeText(value);
      setCopyState("copied");
      window.setTimeout(() => setCopyState("idle"), 2_000);
    } catch {
      setCopyState("error");
    }
  };

  return (
    <Button
      type="button"
      variant="ghost"
      size="icon"
      className="h-7 w-7 shrink-0"
      onClick={() => void copyValue()}
      disabled={!value}
      aria-label={
        copyState === "copied"
          ? `${label} copied`
          : copyState === "error"
            ? `Copy ${label} failed`
            : `Copy ${label}`
      }
    >
      {copyState === "copied" ? (
        <Check className="h-3.5 w-3.5 text-maple-success" />
      ) : copyState === "error" ? (
        <AlertCircle className="h-3.5 w-3.5 text-destructive" />
      ) : (
        <Copy className="h-3.5 w-3.5" />
      )}
      <span className="sr-only" aria-live="polite">
        {copyState === "copied"
          ? `${label} copied`
          : copyState === "error"
            ? `Copy ${label} failed`
            : ""}
      </span>
    </Button>
  );
}

function SetupField({ label, value }: { label: string; value: string }) {
  return (
    <div className="min-w-0 rounded-lg border border-border/70 bg-background/60 p-3">
      <p className="text-[11px] font-medium uppercase tracking-wide text-muted-foreground">
        {label}
      </p>
      <div className="mt-1 flex min-w-0 items-center justify-between gap-2">
        <code className="min-w-0 break-all text-xs text-foreground">{value || "Unavailable"}</code>
        <CopyButton value={value} label={label} />
      </div>
    </div>
  );
}

function CodeExample({ label, code }: { label: string; code: string }) {
  return (
    <div className="overflow-hidden rounded-lg border border-border/70 bg-background">
      <div className="flex items-center justify-between border-b border-border/70 px-3 py-2">
        <span className="text-xs font-medium">{label}</span>
        <CopyButton value={code} label={label} />
      </div>
      {code ? (
        <pre className="max-h-72 overflow-auto p-3 text-xs leading-relaxed">
          <code>{code}</code>
        </pre>
      ) : (
        <p className="p-3 text-xs text-muted-foreground">
          Start Maple's ACP service to resolve this app's executable path.
        </p>
      )}
    </div>
  );
}

function Step({ number, children }: { number: number; children: ReactNode }) {
  return (
    <li className="flex gap-3 text-sm leading-relaxed">
      <span
        aria-hidden="true"
        className="mt-0.5 flex h-5 w-5 shrink-0 items-center justify-center rounded-full bg-[hsl(var(--maple-primary-container))] text-[11px] font-semibold text-foreground"
      >
        {number}
      </span>
      <span>{children}</span>
    </li>
  );
}

export function AgentConnectionGuides({ status }: { status: MapleAcpStatus | null }) {
  const harness = status?.harness ?? null;
  const paseoConfig = harness ? serializePaseoCustomProviderConfig(harness) : "";
  const buzzConfig = harness ? serializeBuzzCustomHarness(harness) : "";

  return (
    <div className="space-y-4">
      <Tabs defaultValue="paseo" className="w-full">
        <TabsList className="grid h-auto w-full grid-cols-2">
          <TabsTrigger value="paseo">Paseo</TabsTrigger>
          <TabsTrigger value="buzz">Buzz</TabsTrigger>
        </TabsList>

        <TabsContent value="paseo" className="space-y-4 pt-2">
          <ol className="space-y-3">
            <Step number={1}>Start Maple's local ACP service above.</Step>
            <Step number={2}>
              Merge the provider below into <code>~/.paseo/config.json</code>. Keep your existing
              settings and add <code>{PASEO_MAPLE_PROVIDER_ID}</code> under{" "}
              <code>agents.providers</code>.
            </Step>
            <Step number={3}>
              Save the file, then fully quit and reopen Paseo Desktop. If you run a standalone
              daemon, restart that daemon instead.
            </Step>
          </ol>

          <CodeExample label="Paseo provider configuration" code={paseoConfig} />

          <Alert role="note">
            <Server className="h-4 w-4" />
            <AlertDescription>
              Maple supplies its signed-in model list dynamically. Paseo tool injection stays
              enabled, so no static model list or Maple API key belongs in this configuration.
            </AlertDescription>
          </Alert>
        </TabsContent>

        <TabsContent value="buzz" className="space-y-4 pt-2">
          <ol className="space-y-3">
            <Step number={1}>Start Maple's local ACP service above.</Step>
            <Step number={2}>Open Buzz Desktop's custom harness form.</Step>
            <Step number={3}>Enter the separate values below and connect.</Step>
          </ol>

          <div className="grid gap-2 sm:grid-cols-2">
            <SetupField label="Name" value={BUZZ_MAPLE_HARNESS_NAME} />
            <SetupField label="ID" value={BUZZ_MAPLE_HARNESS_ID} />
            <SetupField label="Command" value={harness?.command ?? ""} />
            <SetupField label="Argument" value={harness?.args[0] ?? ""} />
          </div>

          <p className="text-xs leading-relaxed text-muted-foreground">
            Keep <code>acp</code> as a separate argument in Buzz; do not append it to Command.
          </p>

          <Alert role="note">
            <Server className="h-4 w-4" />
            <AlertDescription>
              Set Buzz managed-agent parallelism to {BUZZ_MAPLE_AGENT_PARALLELISM}. Buzz defaults to{" "}
              {BUZZ_DEFAULT_AGENT_PARALLELISM}, while Maple accepts {BUZZ_MAPLE_AGENT_PARALLELISM}{" "}
              local ACP connections by default.
            </AlertDescription>
          </Alert>

          <details className="group rounded-lg border border-border/70">
            <summary className="cursor-pointer select-none px-4 py-3 text-sm font-medium">
              Advanced configuration
            </summary>
            <div className="space-y-3 border-t border-border/70 p-4">
              <CodeExample label="Buzz harness JSON" code={buzzConfig} />
              <p className="text-xs leading-relaxed text-muted-foreground">
                Use this JSON only for manual configuration. Buzz's custom harness form uses the
                separate fields above.
              </p>
            </div>
          </details>

          <details className="group rounded-lg border border-border/70">
            <summary className="cursor-pointer select-none px-4 py-3 text-sm font-medium">
              Connection diagnostics
            </summary>
            <dl className="grid gap-3 border-t border-border/70 p-4 text-xs sm:grid-cols-2">
              <Diagnostic label="Transport" value={formatEndpointKind(status?.endpointKind)} />
              <Diagnostic label="Endpoint" value={status?.endpoint || "Not listening"} mono />
              <Diagnostic
                label="Protocol"
                value={status?.protocolVersion ? `ACP ${status.protocolVersion}` : "ACP"}
              />
              <Diagnostic
                label="Buzz relay credentials"
                value={status?.buzzCredentialsAvailable ? "Available" : "Waiting for Buzz"}
              />
            </dl>
          </details>

          {!status?.buzzCredentialsAvailable && status?.connectedClients ? (
            <Alert className="border-maple-warning/40 bg-maple-warning/10">
              <Terminal className="h-4 w-4 text-maple-warning" />
              <AlertDescription>
                A client is connected without Buzz relay credentials. Streaming works, but Maple may
                not be able to post a durable reply to the Buzz channel.
              </AlertDescription>
            </Alert>
          ) : null}
        </TabsContent>
      </Tabs>

      <div className="flex gap-2 text-xs leading-relaxed text-muted-foreground">
        <ShieldCheck className="mt-0.5 h-4 w-4 shrink-0 text-maple-success" />
        <p>These setup values contain no Maple access token or API key.</p>
      </div>
    </div>
  );
}

function Diagnostic({
  label,
  value,
  mono = false
}: {
  label: string;
  value: string;
  mono?: boolean;
}) {
  return (
    <div className="min-w-0">
      <dt className="font-medium text-muted-foreground">{label}</dt>
      <dd className={mono ? "mt-1 break-all font-mono" : "mt-1 break-words"}>{value}</dd>
    </div>
  );
}

function formatEndpointKind(value?: string | null): string {
  if (!value) return "Local IPC";
  return value
    .split("_")
    .filter(Boolean)
    .map((part) => `${part[0]?.toUpperCase() ?? ""}${part.slice(1)}`)
    .join(" ");
}
