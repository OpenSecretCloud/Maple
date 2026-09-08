import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { useOpenSecret } from "@opensecret/react";
import { AlertCircle, Loader2, Play, RefreshCw, Server, ShieldCheck, Square } from "lucide-react";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { useSettingsNavigationLock } from "@/contexts/SettingsNavigationLockContext";
import { awaitAgentAuthUser } from "@/services/agentRuntimeService";
import {
  isMapleAcpConfigReady,
  mapleAcpService,
  type MapleAcpConfig,
  type MapleAcpStatus
} from "@/services/mapleAcpService";
import { AgentConnectionGuides } from "./AgentConnectionGuides";
import { SettingsPage, SettingsSection } from "./SettingsPage";

const STATUS_POLL_INTERVAL_MS = 2_500;

export function AgentConnectionsSettings() {
  const os = useOpenSecret();
  const userId = os.auth.user?.user.id ?? null;
  const [config, setConfig] = useState<MapleAcpConfig | null>(null);
  const [savedConfig, setSavedConfig] = useState<MapleAcpConfig | null>(null);
  const [configUserId, setConfigUserId] = useState<string | null>(null);
  const [status, setStatus] = useState<MapleAcpStatus | null>(null);
  const [isConfigLoading, setIsConfigLoading] = useState(true);
  const [isStatusLoading, setIsStatusLoading] = useState(true);
  const [operation, setOperation] = useState<"start" | "stop" | "refresh" | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [configLoadError, setConfigLoadError] = useState<string | null>(null);
  const [statusLoadError, setStatusLoadError] = useState<string | null>(null);
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

    await Promise.all([configRequest, statusRequest]);
    if (userIdRef.current !== userId) return;
    setOperation(null);
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

  const startService = async () => {
    if (!userId || !config || !savedConfig || configUserId !== userId) return;
    statusRequestGenerationRef.current += 1;
    const operationUserId = userId;
    setOperation("start");
    setError(null);
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
    try {
      const nextStatus = await mapleAcpService.stop(userId);
      if (userIdRef.current !== operationUserId) return;
      setStatus(nextStatus);
      setConfig((current) => (current ? { ...current, enabled: false } : current));
      setSavedConfig((current) => (current ? { ...current, enabled: false } : current));
    } catch (stopError) {
      if (userIdRef.current === operationUserId) setError(errorMessage(stopError));
    } finally {
      if (userIdRef.current === operationUserId) setOperation(null);
    }
  };

  const displayedConfig = configUserId === userId ? config : null;
  const displayedSavedConfig = configUserId === userId ? savedConfig : null;
  const running = status?.running === true;
  const configReady = isMapleAcpConfigReady(displayedConfig, displayedSavedConfig);
  const mutationsDisabled = isBusy || !userId || !configReady;

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
      description="Use Maple Agent from local apps that support the Agent Client Protocol."
      actions={<Badge variant="outline">Preview</Badge>}
    >
      <SettingsSection
        title="Local ACP service"
        description="Start the bridge, then keep Maple open while a client is connected."
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
                {configLoadError} Starting remains locked so Maple cannot replace your saved
                connection settings with defaults. Use refresh to try again.
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
                      ? "Checking service"
                      : status === null
                        ? "Status unavailable"
                        : running
                          ? status.connectedClients > 0
                            ? `${status.connectedClients} client${status.connectedClients === 1 ? "" : "s"} connected`
                            : "Ready for connections"
                          : "Service stopped"}
                  </p>
                  <Badge variant={running ? "secondary" : "outline"}>
                    {isStatusLoading ? "Checking" : statusLabel}
                  </Badge>
                </div>
                <p className="mt-1 text-xs leading-relaxed text-muted-foreground">
                  {isStatusLoading
                    ? "Reading the current status."
                    : status === null
                      ? "Refresh to try again."
                      : running
                        ? "Maple is accepting local ACP sessions."
                        : "Start it when you want an ACP client to connect."}
                </p>
                {running && status ? (
                  <div className="mt-2 flex flex-wrap gap-x-4 gap-y-1 text-xs text-muted-foreground">
                    <StatusMetric label="clients" value={status.connectedClients} />
                    <StatusMetric label="sessions" value={status.activeSessions} />
                    <StatusMetric label="active runs" value={status.activeRuns} />
                  </div>
                ) : null}
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

          <Alert role="note" className="border-maple-warning/40 bg-maple-warning/10">
            <ShieldCheck className="h-4 w-4 text-maple-warning" />
            <AlertDescription>
              Only connect ACP clients you trust. They can choose any folder Maple can access; Read
              only asks before guarded actions but is not an operating-system sandbox.
            </AlertDescription>
          </Alert>
        </div>
      </SettingsSection>

      <SettingsSection
        title="Connect your tools"
        description="Choose an ACP client and follow its setup guide."
      >
        <AgentConnectionGuides status={status} />
      </SettingsSection>
    </SettingsPage>
  );
}

function StatusMetric({ label, value }: { label: string; value: number }) {
  return (
    <span>
      <strong className="font-medium tabular-nums text-foreground">{value}</strong> {label}
    </span>
  );
}

function errorMessage(
  error: unknown,
  fallback = "Maple could not update the ACP service."
): string {
  if (error instanceof Error && error.message.trim()) return error.message;
  if (typeof error === "string" && error.trim()) return error;
  return fallback;
}
