import { useEffect, useId, useMemo, useState, type ReactNode } from "react";
import { Globe2, Pencil, Plus, Puzzle, Search, Server, Terminal, Trash2, X } from "lucide-react";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle
} from "@/components/ui/alert-dialog";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle
} from "@/components/ui/dialog";
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger
} from "@/components/ui/dropdown-menu";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import type {
  AgentMcpKeyValue,
  AgentMcpServer,
  AgentSessionMcpServer
} from "@/services/agentRuntimeService";
import { gooseMcpServerKey, isValidMcpTimeoutSeconds } from "@/services/agentMcpServers";

const DEFAULT_TIMEOUT_SECONDS = 300;
type PendingDiscardAction = "close_form" | "close_dialog";

export function AgentMcpMenu({
  servers,
  disabled,
  togglesDisabled,
  loading,
  onToggle,
  onManage
}: {
  servers: AgentSessionMcpServer[];
  disabled: boolean;
  togglesDisabled: boolean;
  loading: boolean;
  onToggle: (name: string, enabled: boolean) => void;
  onManage: () => void;
}) {
  const [query, setQuery] = useState("");
  const activeCount = servers.filter((server) => server.enabled).length;
  const filteredServers = useMemo(() => {
    const normalizedQuery = query.trim().toLowerCase();
    if (!normalizedQuery) return servers;
    return servers.filter((server) =>
      `${server.name} ${server.description}`.toLowerCase().includes(normalizedQuery)
    );
  }, [query, servers]);

  return (
    <DropdownMenu onOpenChange={(open) => !open && setQuery("")}>
      <DropdownMenuTrigger asChild>
        <Button
          type="button"
          variant="ghost"
          size="sm"
          className="h-8 gap-1 border-0 bg-transparent px-2 text-[hsl(var(--maple-secondary-700))] hover:bg-[hsl(var(--maple-primary-container))] hover:text-[hsl(var(--maple-secondary-700))]"
          disabled={disabled}
          aria-label={`${activeCount} MCP ${activeCount === 1 ? "server" : "servers"} enabled`}
        >
          <Puzzle className="h-4 w-4 shrink-0" />
          <span className="text-xs font-medium">{activeCount}</span>
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start" className="w-[min(22rem,calc(100vw-2rem))] p-2">
        <DropdownMenuLabel className="px-1 pb-2 pt-1">MCP servers</DropdownMenuLabel>
        <div className="relative mb-2" onKeyDown={(event) => event.stopPropagation()}>
          <Search className="pointer-events-none absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-muted-foreground" />
          <Input
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            onClick={(event) => event.stopPropagation()}
            placeholder="Search servers"
            aria-label="Search MCP servers"
            className="h-8 pl-8"
          />
        </div>

        <div className="max-h-64 overflow-y-auto">
          {loading ? (
            <p className="px-2 py-3 text-sm text-muted-foreground">Loading MCP servers…</p>
          ) : filteredServers.length === 0 ? (
            <p className="px-2 py-3 text-sm text-muted-foreground">
              {servers.length === 0 ? "No MCP servers configured." : "No matching servers."}
            </p>
          ) : (
            filteredServers.map((server) => (
              <DropdownMenuCheckboxItem
                key={server.name}
                checked={server.enabled}
                disabled={togglesDisabled || (!server.available && !server.enabled)}
                onCheckedChange={(checked) => onToggle(server.name, checked === true)}
                onSelect={(event) => event.preventDefault()}
                className="items-start"
              >
                <div className="min-w-0 py-0.5">
                  <p className="truncate font-medium">{server.name}</p>
                  <p className="line-clamp-2 text-xs text-muted-foreground">
                    {!server.available
                      ? "No longer available in your saved MCP servers"
                      : server.description || transportLabel(server.transport)}
                  </p>
                </div>
              </DropdownMenuCheckboxItem>
            ))
          )}
        </div>

        <DropdownMenuSeparator />
        <DropdownMenuItem onSelect={onManage}>
          <Server className="mr-2 h-4 w-4" />
          Manage MCP servers…
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

export function AgentMcpServersDialog({
  open,
  servers,
  disabled,
  onOpenChange,
  onSave
}: {
  open: boolean;
  servers: AgentMcpServer[];
  disabled: boolean;
  onOpenChange: (open: boolean) => void;
  onSave: (servers: AgentMcpServer[]) => Promise<void>;
}) {
  const [formServer, setFormServer] = useState<AgentMcpServer | null>(null);
  const [editingIndex, setEditingIndex] = useState<number | null>(null);
  const [formBaseline, setFormBaseline] = useState<AgentMcpServer | null>(null);
  const [isSaving, setIsSaving] = useState(false);
  const [formError, setFormError] = useState<string | null>(null);
  const [pendingDiscardAction, setPendingDiscardAction] = useState<PendingDiscardAction | null>(
    null
  );
  const [pendingDeleteIndex, setPendingDeleteIndex] = useState<number | null>(null);

  useEffect(() => {
    if (!open) {
      setFormServer(null);
      setFormBaseline(null);
      setEditingIndex(null);
      setFormError(null);
      setPendingDiscardAction(null);
      setPendingDeleteIndex(null);
    }
  }, [open]);

  const enabledServers = servers.filter((server) => server.enabled);
  const availableServers = servers.filter((server) => !server.enabled);
  const formIsDirty =
    formServer !== null &&
    formBaseline !== null &&
    JSON.stringify(formServer) !== JSON.stringify(formBaseline);

  const resetForm = () => {
    setFormServer(null);
    setFormBaseline(null);
    setEditingIndex(null);
    setFormError(null);
  };

  const requestCloseForm = () => {
    if (formIsDirty) {
      setPendingDiscardAction("close_form");
      return;
    }
    resetForm();
  };

  const discardForm = () => {
    const action = pendingDiscardAction;
    setPendingDiscardAction(null);
    resetForm();
    if (action === "close_dialog") onOpenChange(false);
  };

  const persist = async (nextServers: AgentMcpServer[]) => {
    setFormError(null);
    setIsSaving(true);
    try {
      await onSave(nextServers);
    } catch (error) {
      setFormError(error instanceof Error ? error.message : String(error));
      throw error;
    } finally {
      setIsSaving(false);
    }
  };

  const beginAdd = () => {
    const server = newMcpServer();
    setEditingIndex(null);
    setFormError(null);
    setFormBaseline(cloneServer(server));
    setFormServer(server);
  };

  const beginEdit = (index: number) => {
    setEditingIndex(index);
    setFormError(null);
    const server = cloneServer(servers[index]);
    setFormBaseline(cloneServer(server));
    setFormServer(server);
  };

  const saveForm = async () => {
    if (!formServer) return;
    const validationError = validateServer(formServer, servers, editingIndex);
    if (validationError) {
      setFormError(validationError);
      return;
    }

    const normalized = normalizeServer(formServer);
    const nextServers =
      editingIndex === null
        ? [...servers, normalized]
        : servers.map((server, index) => (index === editingIndex ? normalized : server));
    try {
      await persist(nextServers);
      resetForm();
    } catch {
      // The inline error keeps the form and its values available for correction.
    }
  };

  const toggleDefault = async (index: number, enabled: boolean) => {
    try {
      await persist(
        servers.map((server, candidateIndex) =>
          candidateIndex === index ? { ...server, enabled } : server
        )
      );
    } catch {
      // The inline error is enough; the authoritative list remains unchanged.
    }
  };

  const deleteServer = (index: number) => setPendingDeleteIndex(index);

  const confirmDeleteServer = async () => {
    if (pendingDeleteIndex === null) return;
    const index = pendingDeleteIndex;
    setPendingDeleteIndex(null);
    try {
      await persist(servers.filter((_, candidateIndex) => candidateIndex !== index));
    } catch {
      // The inline error is enough; the authoritative list remains unchanged.
    }
  };

  return (
    <>
      <Dialog
        open={open}
        onOpenChange={(nextOpen) => {
          if (isSaving) return;
          if (!nextOpen && formIsDirty) {
            setPendingDiscardAction("close_dialog");
            return;
          }
          onOpenChange(nextOpen);
        }}
      >
        <DialogContent className="max-h-[90vh] max-w-3xl overflow-hidden p-0">
          {formServer ? (
            <McpServerForm
              server={formServer}
              isEditing={editingIndex !== null}
              disabled={disabled || isSaving}
              error={formError}
              onChange={setFormServer}
              onCancel={requestCloseForm}
              onSave={() => void saveForm()}
            />
          ) : (
            <div className="flex max-h-[90vh] min-h-0 flex-col">
              <DialogHeader className="shrink-0 px-6 pt-6">
                <DialogTitle>MCP servers</DialogTitle>
                <DialogDescription>
                  Add tools for Agent Mode over Standard IO or Streamable HTTP. Default servers are
                  used for future tasks; existing tasks keep their own selection.
                </DialogDescription>
              </DialogHeader>

              <div className="min-h-0 flex-1 space-y-6 overflow-y-auto px-6 py-5">
                {formError ? (
                  <p className="rounded-md border border-destructive/30 bg-destructive/5 px-3 py-2 text-sm text-destructive">
                    {formError}
                  </p>
                ) : null}

                <McpServerGroup
                  title="Default MCP servers"
                  description="Enabled automatically for new Agent tasks."
                  servers={enabledServers}
                  allServers={servers}
                  disabled={disabled || isSaving}
                  onEdit={beginEdit}
                  onDelete={deleteServer}
                  onToggle={(index, enabled) => void toggleDefault(index, enabled)}
                />

                <McpServerGroup
                  title="Available MCP servers"
                  description="Configured and ready to enable when you need them."
                  servers={availableServers}
                  allServers={servers}
                  disabled={disabled || isSaving}
                  onEdit={beginEdit}
                  onDelete={deleteServer}
                  onToggle={(index, enabled) => void toggleDefault(index, enabled)}
                />
              </div>

              <DialogFooter className="shrink-0 border-t px-6 py-4">
                <Button type="button" onClick={beginAdd} disabled={disabled || isSaving}>
                  <Plus className="mr-2 h-4 w-4" />
                  Add custom server
                </Button>
              </DialogFooter>
            </div>
          )}
        </DialogContent>
      </Dialog>

      <AlertDialog
        open={pendingDiscardAction !== null}
        onOpenChange={(nextOpen) => !nextOpen && setPendingDiscardAction(null)}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Discard unsaved MCP changes?</AlertDialogTitle>
            <AlertDialogDescription>
              The values entered in this server form have not been saved.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Keep editing</AlertDialogCancel>
            <AlertDialogAction onClick={discardForm}>Discard changes</AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      <AlertDialog
        open={pendingDeleteIndex !== null}
        onOpenChange={(nextOpen) => !nextOpen && setPendingDeleteIndex(null)}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Delete this MCP server?</AlertDialogTitle>
            <AlertDialogDescription>
              {pendingDeleteIndex === null
                ? "This server will be removed from future tasks."
                : `“${servers[pendingDeleteIndex]?.name}” will be removed from future tasks. Existing tasks keep their current selection.`}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={isSaving}>Cancel</AlertDialogCancel>
            <AlertDialogAction
              className="bg-destructive text-destructive-onFilled hover:bg-destructive/90"
              disabled={isSaving}
              onClick={() => void confirmDeleteServer()}
            >
              Delete server
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>
  );
}

function McpServerGroup({
  title,
  description,
  servers,
  allServers,
  disabled,
  onEdit,
  onDelete,
  onToggle
}: {
  title: string;
  description: string;
  servers: AgentMcpServer[];
  allServers: AgentMcpServer[];
  disabled: boolean;
  onEdit: (index: number) => void;
  onDelete: (index: number) => void;
  onToggle: (index: number, enabled: boolean) => void;
}) {
  return (
    <section className="space-y-2">
      <div>
        <h3 className="text-sm font-semibold">{title}</h3>
        <p className="text-xs text-muted-foreground">{description}</p>
      </div>
      {servers.length === 0 ? (
        <div className="rounded-xl border border-dashed px-4 py-5 text-center text-sm text-muted-foreground">
          No servers in this group.
        </div>
      ) : (
        <div className="space-y-2">
          {servers.map((server) => {
            const index = allServers.indexOf(server);
            const TransportIcon = server.transport.type === "stdio" ? Terminal : Globe2;
            return (
              <div
                key={server.name}
                className="flex items-center gap-3 rounded-xl border bg-card px-3 py-3"
              >
                <div className="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg bg-muted">
                  <TransportIcon className="h-4 w-4" />
                </div>
                <div className="min-w-0 flex-1">
                  <p className="truncate text-sm font-medium">{server.name}</p>
                  <p className="truncate text-xs text-muted-foreground">
                    {server.description || transportLabel(server.transport.type)}
                  </p>
                </div>
                <Switch
                  checked={server.enabled}
                  onCheckedChange={(enabled) => onToggle(index, enabled)}
                  disabled={disabled}
                  aria-label={`${server.enabled ? "Disable" : "Enable"} ${server.name} by default`}
                />
                <Button
                  type="button"
                  variant="ghost"
                  size="icon"
                  className="h-8 w-8"
                  onClick={() => onEdit(index)}
                  disabled={disabled}
                  aria-label={`Edit ${server.name}`}
                >
                  <Pencil className="h-4 w-4" />
                </Button>
                <Button
                  type="button"
                  variant="ghost"
                  size="icon"
                  className="h-8 w-8 text-destructive hover:text-destructive"
                  onClick={() => onDelete(index)}
                  disabled={disabled}
                  aria-label={`Delete ${server.name}`}
                >
                  <Trash2 className="h-4 w-4" />
                </Button>
              </div>
            );
          })}
        </div>
      )}
    </section>
  );
}

function McpServerForm({
  server,
  isEditing,
  disabled,
  error,
  onChange,
  onCancel,
  onSave
}: {
  server: AgentMcpServer;
  isEditing: boolean;
  disabled: boolean;
  error: string | null;
  onChange: (server: AgentMcpServer) => void;
  onCancel: () => void;
  onSave: () => void;
}) {
  const transport = server.transport;
  const idPrefix = useId();
  const nameId = `${idPrefix}-name`;
  const transportId = `${idPrefix}-transport`;
  const descriptionId = `${idPrefix}-description`;
  const commandId = `${idPrefix}-command`;
  const endpointId = `${idPrefix}-endpoint`;
  const timeoutId = `${idPrefix}-timeout`;
  const setTransportType = (type: "stdio" | "streamable_http") => {
    const environment = transport.environment;
    onChange({
      ...server,
      transport:
        type === "stdio"
          ? { type, command: "", environment }
          : { type, url: "", environment, headers: [] }
    });
  };

  return (
    <form
      className="flex max-h-[90vh] min-h-0 flex-col"
      onSubmit={(event) => {
        event.preventDefault();
        onSave();
      }}
    >
      <DialogHeader className="shrink-0 px-6 pt-6">
        <DialogTitle>{isEditing ? "Edit MCP server" : "Add MCP server"}</DialogTitle>
        <DialogDescription>
          Maple saves this configuration without testing the connection. It will connect when a task
          enables the server.
        </DialogDescription>
      </DialogHeader>

      <div className="min-h-0 flex-1 space-y-5 overflow-y-auto px-6 py-5">
        {error ? (
          <p className="rounded-md border border-destructive/30 bg-destructive/5 px-3 py-2 text-sm text-destructive">
            {error}
          </p>
        ) : null}

        <div className="grid gap-4 sm:grid-cols-2">
          <Field label="Name" controlId={nameId} required>
            <Input
              id={nameId}
              value={server.name}
              onChange={(event) => onChange({ ...server, name: event.target.value })}
              placeholder="My server"
              disabled={disabled}
              autoFocus
            />
          </Field>
          <Field label="Transport" controlId={transportId} required>
            <Select
              value={transport.type}
              onValueChange={(value) =>
                setTransportType(value === "streamable_http" ? value : "stdio")
              }
              disabled={disabled}
            >
              <SelectTrigger id={transportId}>
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="stdio">Standard IO (STDIO)</SelectItem>
                <SelectItem value="streamable_http">Streamable HTTP</SelectItem>
              </SelectContent>
            </Select>
          </Field>
        </div>

        <Field label="Description" controlId={descriptionId} hint="Optional">
          <Input
            id={descriptionId}
            value={server.description}
            onChange={(event) => onChange({ ...server, description: event.target.value })}
            placeholder="What this server helps the agent do"
            disabled={disabled}
          />
        </Field>

        {transport.type === "stdio" ? (
          <Field
            label="Command"
            controlId={commandId}
            required
            hint="Executable and arguments, as one command"
          >
            <Input
              id={commandId}
              value={transport.command}
              onChange={(event) =>
                onChange({
                  ...server,
                  transport: { ...transport, command: event.target.value }
                })
              }
              placeholder="npx -y @modelcontextprotocol/server-everything stdio"
              disabled={disabled}
              spellCheck={false}
            />
          </Field>
        ) : (
          <Field label="Endpoint URL" controlId={endpointId} required>
            <Input
              id={endpointId}
              value={transport.url}
              onChange={(event) =>
                onChange({
                  ...server,
                  transport: { ...transport, url: event.target.value }
                })
              }
              placeholder="http://127.0.0.1:3000/mcp"
              disabled={disabled}
              spellCheck={false}
            />
          </Field>
        )}

        <Field label="Timeout" controlId={timeoutId} required hint="Seconds">
          <Input
            id={timeoutId}
            type="number"
            min={1}
            step={1}
            value={server.timeoutSeconds}
            onChange={(event) =>
              onChange({ ...server, timeoutSeconds: Number(event.target.value) })
            }
            disabled={disabled}
            className="max-w-40"
          />
        </Field>

        <KeyValueFields
          title="Environment variables"
          pairs={transport.environment}
          disabled={disabled}
          onChange={(environment) =>
            onChange({
              ...server,
              transport: { ...transport, environment }
            })
          }
        />

        {transport.type === "streamable_http" ? (
          <KeyValueFields
            title="HTTP headers"
            pairs={transport.headers}
            disabled={disabled}
            keyPlaceholder="Authorization"
            onChange={(headers) =>
              onChange({
                ...server,
                transport: { ...transport, headers }
              })
            }
          />
        ) : null}

        <p className="text-xs text-muted-foreground">
          Values are masked here and saved in this account’s local Agent data, including each task’s
          selected server snapshot. OAuth and legacy SSE transports are not supported in this first
          version.
        </p>
      </div>

      <DialogFooter className="shrink-0 border-t px-6 py-4">
        <Button type="button" variant="outline" onClick={onCancel} disabled={disabled}>
          Cancel
        </Button>
        <Button type="submit" disabled={disabled}>
          {isEditing ? "Save changes" : "Add server"}
        </Button>
      </DialogFooter>
    </form>
  );
}

function KeyValueFields({
  title,
  pairs,
  disabled,
  keyPlaceholder = "VARIABLE_NAME",
  onChange
}: {
  title: string;
  pairs: AgentMcpKeyValue[];
  disabled: boolean;
  keyPlaceholder?: string;
  onChange: (pairs: AgentMcpKeyValue[]) => void;
}) {
  return (
    <div className="space-y-2">
      <div className="flex items-center justify-between gap-3">
        <Label>{title}</Label>
        <Button
          type="button"
          variant="ghost"
          size="sm"
          className="h-7 px-2"
          onClick={() => onChange([...pairs, { key: "", value: "" }])}
          disabled={disabled}
        >
          <Plus className="mr-1 h-3.5 w-3.5" />
          Add
        </Button>
      </div>
      {pairs.length === 0 ? (
        <p className="rounded-lg border border-dashed px-3 py-3 text-xs text-muted-foreground">
          None configured.
        </p>
      ) : (
        <div className="space-y-2">
          {pairs.map((pair, index) => (
            <div key={index} className="grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)_auto] gap-2">
              <Input
                value={pair.key}
                onChange={(event) =>
                  onChange(
                    pairs.map((candidate, candidateIndex) =>
                      candidateIndex === index
                        ? { ...candidate, key: event.target.value }
                        : candidate
                    )
                  )
                }
                placeholder={keyPlaceholder}
                aria-label={`${title} key ${index + 1}`}
                disabled={disabled}
                spellCheck={false}
              />
              <Input
                type="password"
                value={pair.value}
                onChange={(event) =>
                  onChange(
                    pairs.map((candidate, candidateIndex) =>
                      candidateIndex === index
                        ? { ...candidate, value: event.target.value }
                        : candidate
                    )
                  )
                }
                placeholder="Value"
                aria-label={`${title} value ${index + 1}`}
                disabled={disabled}
                autoComplete="off"
              />
              <Button
                type="button"
                variant="ghost"
                size="icon"
                className="h-10 w-10"
                onClick={() =>
                  onChange(pairs.filter((_, candidateIndex) => candidateIndex !== index))
                }
                disabled={disabled}
                aria-label={`Remove ${title.toLowerCase()} row ${index + 1}`}
              >
                <X className="h-4 w-4" />
              </Button>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

function Field({
  label,
  controlId,
  hint,
  required,
  children
}: {
  label: string;
  controlId: string;
  hint?: string;
  required?: boolean;
  children: ReactNode;
}) {
  return (
    <div className="space-y-2">
      <div className="flex items-baseline justify-between gap-3">
        <Label htmlFor={controlId}>
          {label}
          {required ? <span className="text-destructive"> *</span> : null}
        </Label>
        {hint ? <span className="text-xs text-muted-foreground">{hint}</span> : null}
      </div>
      {children}
    </div>
  );
}

function newMcpServer(): AgentMcpServer {
  return {
    name: "",
    description: "",
    enabled: true,
    timeoutSeconds: DEFAULT_TIMEOUT_SECONDS,
    transport: { type: "stdio", command: "", environment: [] }
  };
}

function cloneServer(server: AgentMcpServer): AgentMcpServer {
  return {
    ...server,
    transport:
      server.transport.type === "stdio"
        ? {
            ...server.transport,
            environment: server.transport.environment.map((pair) => ({ ...pair }))
          }
        : {
            ...server.transport,
            environment: server.transport.environment.map((pair) => ({ ...pair })),
            headers: server.transport.headers.map((pair) => ({ ...pair }))
          }
  };
}

function normalizeServer(server: AgentMcpServer): AgentMcpServer {
  const environment = normalizePairs(server.transport.environment);
  return {
    ...server,
    name: server.name.trim(),
    description: server.description.trim(),
    timeoutSeconds: server.timeoutSeconds,
    transport:
      server.transport.type === "stdio"
        ? { ...server.transport, command: server.transport.command.trim(), environment }
        : {
            ...server.transport,
            url: server.transport.url.trim(),
            environment,
            headers: normalizePairs(server.transport.headers)
          }
  };
}

function normalizePairs(pairs: AgentMcpKeyValue[]): AgentMcpKeyValue[] {
  return pairs
    .filter((pair) => pair.key.trim() || pair.value)
    .map((pair) => ({ key: pair.key.trim(), value: pair.value }));
}

function validateServer(
  server: AgentMcpServer,
  servers: AgentMcpServer[],
  editingIndex: number | null
): string | null {
  const name = server.name.trim();
  if (!name) return "Enter a server name.";
  const nameKey = gooseMcpServerKey(name);
  if (
    servers.some(
      (candidate, index) => index !== editingIndex && gooseMcpServerKey(candidate.name) === nameKey
    )
  ) {
    return "Another MCP server already uses that name.";
  }
  if (!isValidMcpTimeoutSeconds(server.timeoutSeconds)) {
    return "Timeout must be a positive whole number of seconds.";
  }
  if (server.transport.type === "stdio" && !server.transport.command.trim()) {
    return "Enter the command used to start this STDIO server.";
  }
  if (server.transport.type === "streamable_http" && !server.transport.url.trim()) {
    return "Enter the Streamable HTTP endpoint URL.";
  }
  const pairs = [
    ...server.transport.environment,
    ...(server.transport.type === "streamable_http" ? server.transport.headers : [])
  ];
  if (pairs.some((pair) => !pair.key.trim() && Boolean(pair.value))) {
    return "Every environment variable and HTTP header value needs a key.";
  }
  return null;
}

function transportLabel(transport: "stdio" | "streamable_http"): string {
  return transport === "stdio" ? "Standard IO (STDIO)" : "Streamable HTTP";
}
