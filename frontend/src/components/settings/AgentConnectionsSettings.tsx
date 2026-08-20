import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { useOpenSecret } from "@opensecret/react";
import {
  AlertCircle,
  Check,
  Copy,
  Loader2,
  Play,
  RefreshCw,
  Server,
  ShieldCheck,
  Square,
  Terminal
} from "lucide-react";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue
} from "@/components/ui/select";
import { Textarea } from "@/components/ui/textarea";
import { useSettingsNavigationLock } from "@/contexts/SettingsNavigationLockContext";
import { awaitAgentAuthUser } from "@/services/agentRuntimeService";
import {
  BUZZ_DEFAULT_AGENT_PARALLELISM,
  BUZZ_MAPLE_AGENT_PARALLELISM,
  BUZZ_MAPLE_HARNESS_ID,
  BUZZ_MAPLE_HARNESS_NAME,
  isMapleAcpConfigReady,
  isMapleAcpPolicyDirty,
  mapleAcpService,
  serializeBuzzCustomHarness,
  type MapleAcpConfig,
  type MapleAcpPermissionMode,
  type MapleAcpStatus
} from "@/services/mapleAcpService";
import { SettingsPage, SettingsSection } from "./SettingsPage";

const STATUS_POLL_INTERVAL_MS = 2_500;

type CopyTarget = "name" | "id" | "command" | "argument" | "harness";

export function AgentConnectionsSettings() {
  const os = useOpenSecret();
  const userId = os.auth.user?.user.id ?? null;
  const [config, setConfig] = useState<MapleAcpConfig | null>(null);
  const [savedConfig, setSavedConfig] = useState<MapleAcpConfig | null>(null);
  const [configUserId, setConfigUserId] = useState<string | null>(null);
  const [status, setStatus] = useState<MapleAcpStatus | null>(null);
  const [isConfigLoading, setIsConfigLoading] = useState(true);
  const [isStatusLoading, setIsStatusLoading] = useState(true);
  const [operation, setOperation] = useState<"start" | "stop" | "save" | "refresh" | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [configLoadError, setConfigLoadError] = useState<string | null>(null);
  const [statusLoadError, setStatusLoadError] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [copied, setCopied] = useState<CopyTarget | null>(null);
  const userIdRef = useRef(userId);
  const operationRef = useRef(operation);
  const statusRequestGenerationRef = useRef(0);
  const isBusy = operation !== null;
  useSettingsNavigationLock(isBusy);

  useLayoutEffect(() => {
    userIdRef.current = userId;
    operationRef.current = operation;
  }, [operation, userId]);

  const refreshSettings = useCallback(async () => {
    if (!userId) return;

    statusRequestGenerationRef.current += 1;
    const shouldRetryConfig = savedConfig === null || configUserId !== userId;
    setOperation("refresh");
    setError(null);
    setMessage(null);
    setIsStatusLoading(true);
    setStatusLoadError(null);
    if (shouldRetryConfig) {
      setIsConfigLoading(true);
      setConfigLoadError(null);
    }

    const authReady = awaitAgentAuthUser(userId);
    const configRequest = shouldRetryConfig
      ? authReady
          .then(() => mapleAcpService.loadConfig(userId))
          .then((nextConfig) => {
            if (userIdRef.current !== userId) return false;
            setConfig(nextConfig);
            setSavedConfig(nextConfig);
            setConfigUserId(userId);
            setConfigLoadError(null);
            return true;
          })
          .catch((loadError) => {
            if (userIdRef.current !== userId) return false;
            setConfigLoadError(errorMessage(loadError, "Maple could not load the agent policy."));
            return false;
          })
          .finally(() => {
            if (userIdRef.current === userId) setIsConfigLoading(false);
          })
      : Promise.resolve(true);
    const statusRequest = authReady
      .then(() => mapleAcpService.getStatus(userId))
      .then((nextStatus) => {
        if (userIdRef.current !== userId) return false;
        setStatus(nextStatus);
        setStatusLoadError(null);
        return true;
      })
      .catch((loadError) => {
        if (userIdRef.current !== userId) return false;
        setStatusLoadError(errorMessage(loadError, "Maple could not load the ACP service status."));
        return false;
      })
      .finally(() => {
        if (userIdRef.current === userId) setIsStatusLoading(false);
      });

    const [configLoaded, statusLoaded] = await Promise.all([configRequest, statusRequest]);
    if (userIdRef.current !== userId) return;
    setOperation(null);

    if (configLoaded && statusLoaded) {
      setMessage(shouldRetryConfig ? "Agent policy and status refreshed." : "Status refreshed.");
    } else if (configLoaded && shouldRetryConfig) {
      setMessage("Agent policy loaded. Service status is still unavailable.");
    }
  }, [configUserId, savedConfig, userId]);

  useEffect(() => {
    statusRequestGenerationRef.current += 1;
    if (!userId) {
      setConfig(null);
      setSavedConfig(null);
      setConfigUserId(null);
      setStatus(null);
      setIsConfigLoading(false);
      setIsStatusLoading(false);
      setConfigLoadError(null);
      setStatusLoadError(null);
      setError(null);
      setMessage(null);
      setOperation(null);
      return;
    }

    let disposed = false;
    let timer: number | undefined;

    setConfig(null);
    setSavedConfig(null);
    setConfigUserId(null);
    setStatus(null);
    setIsConfigLoading(true);
    setIsStatusLoading(true);
    setConfigLoadError(null);
    setStatusLoadError(null);
    setError(null);
    setMessage(null);
    setOperation(null);

    const authReady = awaitAgentAuthUser(userId);
    const configLoad = authReady
      .then(() => mapleAcpService.loadConfig(userId))
      .then((nextConfig) => {
        if (disposed) return;
        setConfig(nextConfig);
        setSavedConfig(nextConfig);
        setConfigUserId(userId);
        setConfigLoadError(null);
      })
      .catch((loadError) => {
        if (!disposed) {
          setConfigLoadError(errorMessage(loadError, "Maple could not load the agent policy."));
        }
      })
      .finally(() => {
        if (!disposed) setIsConfigLoading(false);
      });

    const statusLoad = authReady
      .then(() => mapleAcpService.getStatus(userId))
      .then((nextStatus) => {
        if (disposed) return;
        setStatus(nextStatus);
        setStatusLoadError(null);
      })
      .catch((loadError) => {
        if (!disposed) {
          setStatusLoadError(
            errorMessage(loadError, "Maple could not load the ACP service status.")
          );
        }
      })
      .finally(() => {
        if (!disposed) setIsStatusLoading(false);
      });

    const poll = async () => {
      if (disposed) return;
      if (operationRef.current === null) {
        const requestGeneration = statusRequestGenerationRef.current;
        try {
          const nextStatus = await mapleAcpService.getStatus(userId);
          if (
            !disposed &&
            requestGeneration === statusRequestGenerationRef.current &&
            operationRef.current === null
          ) {
            setStatus(nextStatus);
            setStatusLoadError(null);
          }
        } catch {
          // Keep the last known status. Explicit refreshes surface diagnostics.
        }
      }
      if (!disposed) timer = window.setTimeout(poll, STATUS_POLL_INTERVAL_MS);
    };

    void Promise.allSettled([configLoad, statusLoad]).then(() => {
      if (!disposed) timer = window.setTimeout(poll, STATUS_POLL_INTERVAL_MS);
    });
    return () => {
      disposed = true;
      if (timer !== undefined) window.clearTimeout(timer);
    };
  }, [userId]);

  const savePolicy = async (): Promise<MapleAcpConfig | null> => {
    if (!userId || !config || !savedConfig || configUserId !== userId) return null;
    const operationUserId = userId;
    setOperation("save");
    setError(null);
    setMessage(null);
    try {
      const nextConfig = await mapleAcpService.saveConfig(userId, {
        ...config,
        enabled: status?.enabled ?? config.enabled
      });
      if (userIdRef.current !== operationUserId) return null;
      setConfig(nextConfig);
      setSavedConfig(nextConfig);
      setMessage("Agent policy saved.");
      return nextConfig;
    } catch (saveError) {
      if (userIdRef.current === operationUserId) setError(errorMessage(saveError));
      return null;
    } finally {
      if (userIdRef.current === operationUserId) setOperation(null);
    }
  };

  const startService = async () => {
    if (!userId || !config || !savedConfig || configUserId !== userId) return;
    statusRequestGenerationRef.current += 1;
    const operationUserId = userId;
    setOperation("start");
    setError(null);
    setMessage(null);
    try {
      const nextConfig = await mapleAcpService.saveConfig(userId, {
        ...config,
        enabled: status?.enabled ?? config.enabled
      });
      if (userIdRef.current !== operationUserId) return;
      const nextStatus = await mapleAcpService.start(userId);
      if (userIdRef.current !== operationUserId) return;
      const startedConfig = { ...nextConfig, enabled: nextStatus.enabled || nextStatus.running };
      setConfig(startedConfig);
      setSavedConfig(startedConfig);
      setStatus(nextStatus);
      setMessage("Maple is ready for local ACP clients.");
    } catch (startError) {
      if (userIdRef.current === operationUserId) setError(errorMessage(startError));
    } finally {
      if (userIdRef.current === operationUserId) setOperation(null);
    }
  };

  const stopService = async () => {
    if (!userId) return;
    statusRequestGenerationRef.current += 1;
    const operationUserId = userId;
    setOperation("stop");
    setError(null);
    setMessage(null);
    try {
      const nextStatus = await mapleAcpService.stop(userId);
      if (userIdRef.current !== operationUserId) return;
      setStatus(nextStatus);
      setConfig((current) => (current ? { ...current, enabled: false } : current));
      setSavedConfig((current) => (current ? { ...current, enabled: false } : current));
      setMessage("Local ACP connections are disabled.");
    } catch (stopError) {
      if (userIdRef.current === operationUserId) setError(errorMessage(stopError));
    } finally {
      if (userIdRef.current === operationUserId) setOperation(null);
    }
  };

  const harnessJson = useMemo(
    () => (status?.harness ? serializeBuzzCustomHarness(status.harness) : ""),
    [status?.harness]
  );
  const displayedConfig = configUserId === userId ? config : null;
  const displayedSavedConfig = configUserId === userId ? savedConfig : null;
  const running = status?.running === true;
  const configReady = isMapleAcpConfigReady(displayedConfig, displayedSavedConfig);
  const permissionMode = displayedConfig?.permissionMode ?? "read_only";
  const policyDirty = isMapleAcpPolicyDirty(displayedConfig, displayedSavedConfig);
  const mutationsDisabled = isBusy || !userId || !configReady;
  const policyMutationsDisabled = mutationsDisabled || running;

  const copyText = async (target: CopyTarget, value: string) => {
    if (!value) return;
    try {
      await navigator.clipboard.writeText(value);
      setCopied(target);
      window.setTimeout(() => setCopied((current) => (current === target ? null : current)), 2_000);
    } catch (copyError) {
      setError(errorMessage(copyError, "Maple could not copy to the clipboard."));
    }
  };

  const statusLabel =
    status === null
      ? "Unavailable"
      : running
        ? status.connectedClients > 0
          ? "Connected"
          : "Ready"
        : "Stopped";

  return (
    <SettingsPage
      title="Agent connections"
      description="Let trusted local ACP clients such as Buzz use Maple Agent through this desktop app."
      actions={<Badge variant="outline">Preview</Badge>}
    >
      <span className="sr-only" aria-live="polite">
        {copied ? `${copied === "harness" ? "Harness JSON" : copied} copied to clipboard.` : ""}
      </span>
      <SettingsSection
        title="Local ACP service"
        description="Maple Desktop owns your authenticated Agent runtime. Keep Maple open and signed in while connected clients are working."
      >
        <div className="space-y-4">
          {error && (
            <Alert variant="destructive">
              <AlertCircle className="h-4 w-4" />
              <AlertDescription>{error}</AlertDescription>
            </Alert>
          )}
          {configLoadError && (
            <Alert variant="destructive">
              <AlertCircle className="h-4 w-4" />
              <AlertDescription>
                {configLoadError} Policy controls remain locked so Maple cannot replace your saved
                policy with defaults. Use refresh to try again.
              </AlertDescription>
            </Alert>
          )}
          {statusLoadError && (
            <Alert variant="destructive">
              <AlertCircle className="h-4 w-4" />
              <AlertDescription>
                {statusLoadError} The last known status is preserved. Use refresh to try again.
              </AlertDescription>
            </Alert>
          )}
          {message && !error && (
            <Alert role="status" aria-live="polite">
              <Check className="h-4 w-4 text-maple-success" />
              <AlertDescription>{message}</AlertDescription>
            </Alert>
          )}
          {status?.error && !error && (
            <Alert variant="destructive">
              <AlertCircle className="h-4 w-4" />
              <AlertDescription>{status.error}</AlertDescription>
            </Alert>
          )}

          <div
            className="flex flex-col gap-4 rounded-xl border border-border/70 bg-background/40 p-4 sm:flex-row sm:items-center sm:justify-between"
            aria-busy={isConfigLoading || isStatusLoading || isBusy}
          >
            <div className="flex min-w-0 items-start gap-3">
              <div className="rounded-lg bg-[hsl(var(--maple-primary-container))] p-2 text-[hsl(var(--maple-primary-strong))]">
                <Server className="h-5 w-5" />
              </div>
              <div className="min-w-0">
                <div className="flex flex-wrap items-center gap-2">
                  <p className="font-medium">
                    {isStatusLoading
                      ? "Checking local ACP service"
                      : status
                        ? `ACP service ${statusLabel.toLowerCase()}`
                        : "ACP service status unavailable"}
                  </p>
                  <Badge variant={running ? "secondary" : "outline"}>
                    {isStatusLoading ? "Checking" : statusLabel}
                  </Badge>
                </div>
                <p className="mt-1 text-xs leading-relaxed text-muted-foreground">
                  {isStatusLoading
                    ? "Reading the local service status."
                    : status === null
                      ? "Refresh to retry. Maple will not assume the service is stopped."
                      : running
                        ? status?.connectedClients
                          ? `${status.connectedClients} local client${status.connectedClients === 1 ? " is" : "s are"} connected.`
                          : "Waiting for a trusted local ACP client to connect."
                        : "Disabled by default. Start it when you are ready to connect Buzz."}
                </p>
              </div>
            </div>
            <div className="flex shrink-0 gap-2">
              <Button
                type="button"
                size="icon"
                variant="outline"
                onClick={() => void refreshSettings()}
                disabled={isConfigLoading || isStatusLoading || isBusy || !userId}
                aria-label="Refresh ACP settings and service status"
              >
                {operation === "refresh" ? (
                  <Loader2 className="h-4 w-4 animate-spin" />
                ) : (
                  <RefreshCw className="h-4 w-4" />
                )}
              </Button>
              {running ? (
                <Button
                  type="button"
                  variant="destructive"
                  onClick={() => void stopService()}
                  disabled={isBusy || !userId}
                >
                  {operation === "stop" ? (
                    <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                  ) : (
                    <Square className="mr-2 h-4 w-4" />
                  )}
                  {operation === "stop" ? "Stopping..." : "Stop service"}
                </Button>
              ) : (
                <Button
                  type="button"
                  onClick={() => void startService()}
                  disabled={mutationsDisabled}
                >
                  {operation === "start" ? (
                    <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                  ) : (
                    <Play className="mr-2 h-4 w-4" />
                  )}
                  {operation === "start" ? "Starting..." : "Start service"}
                </Button>
              )}
            </div>
          </div>

          <div className="grid grid-cols-3 gap-2 text-center">
            <StatusMetric label="Clients" value={status?.connectedClients ?? 0} />
            <StatusMetric label="Sessions" value={status?.activeSessions ?? 0} />
            <StatusMetric label="Active runs" value={status?.activeRuns ?? 0} />
          </div>

          <Alert role="note">
            <ShieldCheck className="h-4 w-4" />
            <AlertDescription>
              The bridge does not put your Maple access tokens, API keys, or Buzz private key in its
              command or harness configuration. Stopping it disconnects every local client.
            </AlertDescription>
          </Alert>
        </div>
      </SettingsSection>

      <SettingsSection
        title="ACP approvals"
        description="The connected ACP client handles unresolved tool approvals."
      >
        <div className="space-y-4">
          <div className="grid gap-2">
            <Label htmlFor="agent-acp-permission-mode">Permission mode</Label>
            <Select
              value={permissionMode}
              onValueChange={(value) => {
                if (!displayedConfig) return;
                setConfig((current) => ({
                  ...(current ?? displayedConfig),
                  permissionMode: value as MapleAcpPermissionMode
                }));
                setMessage(null);
              }}
              disabled={policyMutationsDisabled}
            >
              <SelectTrigger id="agent-acp-permission-mode">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="read_only">ACP client decides</SelectItem>
              </SelectContent>
            </Select>
            <p className="text-xs leading-relaxed text-muted-foreground">
              {displayedConfig === null
                ? isConfigLoading
                  ? "Loading your saved agent policy."
                  : "Your saved policy is unavailable. Refresh before changing or starting ACP."
                : running
                  ? "Stop the ACP service before changing its policy. Maple sends guarded requests to the connected client, which may prompt, deny, or approve automatically."
                  : "Maple sends guarded requests to the connected client. Buzz currently selects Allow once automatically, so trusted prompts can run unattended."}
            </p>
          </div>

          <Alert role="note" className="border-maple-warning/40 bg-maple-warning/10">
            <AlertCircle className="h-4 w-4 text-maple-warning" />
            <AlertDescription>
              This preview does not yet expose project-root allowlisting. A connected client can
              select any absolute working directory Maple can access. Neither policy is an
              operating-system sandbox.
            </AlertDescription>
          </Alert>

          <div className="flex justify-end">
            <Button
              type="button"
              variant="outline"
              onClick={() => void savePolicy()}
              disabled={policyMutationsDisabled || !policyDirty}
            >
              {operation === "save" && <Loader2 className="mr-2 h-4 w-4 animate-spin" />}
              {operation === "save" ? "Saving..." : "Save policy"}
            </Button>
          </div>
        </div>
      </SettingsSection>

      <SettingsSection
        title="Connect Buzz"
        description="Enter these values as separate fields in Buzz Desktop's custom harness form."
      >
        <div className="space-y-5">
          <div className="grid gap-4 sm:grid-cols-2">
            <CopyableHarnessField
              id="agent-acp-buzz-name"
              label="Name"
              value={BUZZ_MAPLE_HARNESS_NAME}
              target="name"
              copied={copied}
              onCopy={copyText}
            />
            <CopyableHarnessField
              id="agent-acp-buzz-id"
              label="ID"
              value={BUZZ_MAPLE_HARNESS_ID}
              target="id"
              copied={copied}
              onCopy={copyText}
            />
            <CopyableHarnessField
              id="agent-acp-buzz-command"
              label="Command"
              value={status?.harness?.command ?? ""}
              placeholder="Exact Maple executable unavailable"
              target="command"
              copied={copied}
              onCopy={copyText}
            />
            <CopyableHarnessField
              id="agent-acp-buzz-argument"
              label="Argument"
              value={status?.harness?.args[0] ?? ""}
              placeholder="Bridge argument unavailable"
              target="argument"
              copied={copied}
              onCopy={copyText}
            />
          </div>
          <p className="text-xs leading-relaxed text-muted-foreground">
            Command is the executable path only. Add <code className="font-mono">acp</code> as a
            separate argument row in Buzz; do not append it to Command.
          </p>

          <Alert role="note">
            <Server className="h-4 w-4" />
            <AlertDescription>
              In Buzz managed-agent settings, set parallelism to {BUZZ_MAPLE_AGENT_PARALLELISM}.
              Buzz defaults to {BUZZ_DEFAULT_AGENT_PARALLELISM}; Maple's default local ACP limit is{" "}
              {BUZZ_MAPLE_AGENT_PARALLELISM}.
            </AlertDescription>
          </Alert>

          <div className="grid gap-2">
            <div className="flex items-center justify-between gap-3">
              <Label htmlFor="agent-acp-buzz-json">Advanced/manual configuration JSON</Label>
              <Button
                type="button"
                size="sm"
                variant="outline"
                onClick={() => void copyText("harness", harnessJson)}
                disabled={!harnessJson}
              >
                {copied === "harness" ? (
                  <Check className="mr-2 h-4 w-4 text-maple-success" />
                ) : (
                  <Copy className="mr-2 h-4 w-4" />
                )}
                {copied === "harness" ? "Copied" : "Copy JSON"}
              </Button>
            </div>
            <Textarea
              id="agent-acp-buzz-json"
              value={harnessJson}
              readOnly
              placeholder="Harness configuration unavailable"
              className="min-h-40 resize-y font-mono text-xs"
            />
            <p className="text-xs leading-relaxed text-muted-foreground">
              This JSON is for advanced or manual configuration. Buzz Desktop's custom harness form
              uses the separate fields above. Keep this Maple Desktop instance running after setup.
            </p>
          </div>

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
                A client is connected without Buzz relay credentials. Maple can stream ACP output,
                but it may not be able to post a durable reply to the Buzz channel.
              </AlertDescription>
            </Alert>
          ) : null}
        </div>
      </SettingsSection>
    </SettingsPage>
  );
}

function CopyableHarnessField({
  id,
  label,
  value,
  placeholder,
  target,
  copied,
  onCopy
}: {
  id: string;
  label: string;
  value: string;
  placeholder?: string;
  target: CopyTarget;
  copied: CopyTarget | null;
  onCopy: (target: CopyTarget, value: string) => Promise<void>;
}) {
  const wasCopied = copied === target;

  return (
    <div className="grid gap-2">
      <Label htmlFor={id}>{label}</Label>
      <div className="flex gap-2">
        <Input
          id={id}
          value={value}
          readOnly
          placeholder={placeholder}
          className="font-mono text-xs"
        />
        <Button
          type="button"
          size="icon"
          variant="outline"
          onClick={() => void onCopy(target, value)}
          disabled={!value}
          aria-label={wasCopied ? `${label} copied` : `Copy ${label}`}
        >
          {wasCopied ? (
            <Check className="h-4 w-4 text-maple-success" />
          ) : (
            <Copy className="h-4 w-4" />
          )}
        </Button>
      </div>
    </div>
  );
}

function StatusMetric({ label, value }: { label: string; value: number }) {
  return (
    <div className="rounded-lg border border-border/70 bg-muted/30 px-2 py-3">
      <p className="text-lg font-semibold tabular-nums">{value}</p>
      <p className="text-[11px] text-muted-foreground">{label}</p>
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

function errorMessage(
  error: unknown,
  fallback = "Maple could not update the ACP service."
): string {
  if (error instanceof Error && error.message.trim()) return error.message;
  if (typeof error === "string" && error.trim()) return error;
  return fallback;
}
