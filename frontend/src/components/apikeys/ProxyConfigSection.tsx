import { useState, useEffect } from "react";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Play, Square, Loader2, AlertCircle, CheckCircle, Server, Copy, Check } from "lucide-react";
import { proxyService, ProxyConfig, ProxyStatus } from "@/services/proxyService";
import { useIsTauriDesktop } from "@/hooks/usePlatform";

interface ProxyConfigSectionProps {
  apiKeys: Array<{ name: string; created_at: string }>;
  onRequestNewApiKey: (name: string) => Promise<string>;
}

export function ProxyConfigSection({ apiKeys, onRequestNewApiKey }: ProxyConfigSectionProps) {
  const [proxyStatus, setProxyStatus] = useState<ProxyStatus | null>(null);
  const [config, setConfig] = useState<ProxyConfig>({
    host: "127.0.0.1",
    port: 8080,
    api_key: "",
    enabled: false,
    enable_cors: true,
    auto_start: false
  });
  const [isLoading, setIsLoading] = useState(false);
  const [message, setMessage] = useState<{ type: "success" | "error"; text: string } | null>(null);
  const [copied, setCopied] = useState(false);
  const { isTauriDesktop } = useIsTauriDesktop();

  useEffect(() => {
    if (!isTauriDesktop) return;

    // Load saved config and status on mount
    loadProxyState();
  }, [isTauriDesktop]);

  const loadProxyState = async () => {
    try {
      const [savedConfig, status] = await Promise.all([
        proxyService.loadProxyConfig(),
        proxyService.getProxyStatus()
      ]);

      setConfig(savedConfig);
      setProxyStatus(status);
    } catch (error) {
      console.error("Failed to load proxy state:", error);
    }
  };

  const handleStartProxy = async () => {
    setIsLoading(true);

    try {
      // If no API key is set, auto-generate one
      let apiKey = config.api_key;
      if (!apiKey) {
        const date = new Date().toISOString().split("T")[0].replace(/-/g, "");
        const keyName = `maple-desktop-${date}`;

        // Check if a key with this name already exists
        const existingKey = apiKeys.find((k) => k.name === keyName);
        if (existingKey) {
          // Use the existing key (we don't have access to the actual key value)
          setMessage({
            type: "error",
            text: "Please select an existing API key or create a new one"
          });
          setIsLoading(false);
          return;
        }

        // Request a new API key
        try {
          apiKey = await onRequestNewApiKey(keyName);
          setConfig((prev) => ({ ...prev, api_key: apiKey }));
        } catch {
          setMessage({
            type: "error",
            text: "Failed to create API key. Please create an API key manually first"
          });
          setIsLoading(false);
          return;
        }
      }

      // Validate port range (1-65535)
      const port = Number(config.port);
      if (!Number.isInteger(port) || port < 1 || port > 65535) {
        setMessage({
          type: "error",
          text: `Invalid port: ${config.port}. Port must be between 1 and 65535.`
        });
        setIsLoading(false);
        return;
      }

      // Get the backend URL from environment
      const backendUrl = import.meta.env.VITE_OPEN_SECRET_API_URL || "https://enclave.trymaple.ai";

      const updatedConfig = {
        ...config,
        api_key: apiKey,
        enabled: true,
        backend_url: backendUrl,
        auto_start: config.auto_start // Preserve auto_start setting
      };
      const status = await proxyService.startProxy(updatedConfig);
      setProxyStatus(status);
      setConfig(updatedConfig);

      setMessage({
        type: "success",
        text: `Proxy is now running on ${config.host}:${config.port}`
      });
    } catch (error) {
      setMessage({ type: "error", text: `Failed to start proxy: ${error}` });
    } finally {
      setIsLoading(false);
    }
  };

  const handleStopProxy = async () => {
    setIsLoading(true);

    try {
      const status = await proxyService.stopProxy();
      setProxyStatus(status);
      setConfig((prev) => ({ ...prev, enabled: false }));

      setMessage({ type: "success", text: "The proxy server has been stopped" });
    } catch (error) {
      setMessage({ type: "error", text: `Failed to stop proxy: ${error}` });
    } finally {
      setIsLoading(false);
    }
  };

  const handleConfigChange = (field: keyof ProxyConfig, value: string | number | boolean) => {
    setConfig((prev) => ({ ...prev, [field]: value }));
  };

  const copyProxyUrl = () => {
    const url = `http://${config.host}:${config.port}/v1`;
    navigator.clipboard.writeText(url);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  if (!isTauriDesktop) {
    return null; // Don't show proxy config on non-desktop platforms (includes mobile)
  }

  const isRunning = proxyStatus?.running || false;

  return (
    <>
      {/* Show message alerts */}
      {message && (
        <Alert
          className={`${
            message.type === "error" ? "border-destructive/50" : "border-green-500/50"
          } mb-3`}
        >
          <div className="flex items-start gap-2">
            {message.type === "error" ? (
              <AlertCircle className="h-4 w-4 flex-shrink-0" />
            ) : (
              <CheckCircle className="h-4 w-4 text-green-500 flex-shrink-0" />
            )}
            <AlertDescription className="text-xs">{message.text}</AlertDescription>
          </div>
        </Alert>
      )}

      <div className="flex items-center justify-between mb-3">
        <h3 className="text-sm font-medium flex items-center gap-2">
          <Server className="h-4 w-4" />
          Local OpenAI Proxy
        </h3>
        <div className="flex items-center gap-2">
          <span className="text-xs text-muted-foreground">{isRunning ? "Running" : "Stopped"}</span>
          <div className={`h-2 w-2 rounded-full ${isRunning ? "bg-green-500" : "bg-gray-400"}`} />
        </div>
      </div>

      <Card className="p-4 space-y-4">
        {/* Configuration */}
        <div className="grid grid-cols-2 gap-4">
          <div>
            <Label htmlFor="proxy-host" className="text-xs">
              Host
            </Label>
            <Input
              id="proxy-host"
              value={config.host}
              onChange={(e) => handleConfigChange("host", e.target.value)}
              placeholder="127.0.0.1"
              disabled={isRunning}
              className="h-8 text-sm"
            />
          </div>
          <div>
            <Label htmlFor="proxy-port" className="text-xs">
              Port
            </Label>
            <div className="flex gap-2">
              <Input
                id="proxy-port"
                type="number"
                value={config.port}
                onChange={(e) => handleConfigChange("port", parseInt(e.target.value) || 8080)}
                placeholder="8080"
                disabled={isRunning}
                className="h-8 text-sm"
              />
            </div>
          </div>
        </div>

        {/* API Key Info */}
        {config.api_key && (
          <div>
            <Label className="text-xs">API Key Status</Label>
            <p className="text-xs text-muted-foreground mt-1">API key configured</p>
          </div>
        )}

        {/* Proxy URL */}
        {isRunning && (
          <div>
            <Label className="text-xs">Proxy URL</Label>
            <div className="flex gap-2">
              <Input
                value={`http://${config.host}:${config.port}/v1`}
                readOnly
                className="h-8 text-sm font-mono"
              />
              <Button size="sm" variant="outline" onClick={copyProxyUrl} className="h-8">
                {copied ? <Check className="h-3 w-3" /> : <Copy className="h-3 w-3" />}
              </Button>
            </div>
            <p className="text-xs text-muted-foreground mt-1">
              Use this URL as your OpenAI base URL in any client
            </p>
          </div>
        )}

        {/* Control Buttons */}
        <div className="flex gap-2">
          {!isRunning ? (
            <Button onClick={handleStartProxy} disabled={isLoading} size="sm" className="flex-1">
              {isLoading ? (
                <Loader2 className="mr-2 h-4 w-4 animate-spin" />
              ) : (
                <Play className="mr-2 h-4 w-4" />
              )}
              Start Proxy
            </Button>
          ) : (
            <Button
              onClick={handleStopProxy}
              disabled={isLoading}
              variant="destructive"
              size="sm"
              className="flex-1"
            >
              {isLoading ? (
                <Loader2 className="mr-2 h-4 w-4 animate-spin" />
              ) : (
                <Square className="mr-2 h-4 w-4" />
              )}
              Stop Proxy
            </Button>
          )}
        </div>

        {/* CORS Toggle */}
        <div className="flex items-center justify-between pt-2 border-t">
          <Label htmlFor="enable-cors" className="text-xs">
            Enable CORS (for web clients)
          </Label>
          <input
            id="enable-cors"
            type="checkbox"
            checked={config.enable_cors ?? true}
            onChange={(e) => handleConfigChange("enable_cors", e.target.checked)}
            disabled={isRunning}
            className="h-4 w-4"
          />
        </div>

        {/* Auto-start Toggle */}
        <div className="flex items-center justify-between">
          <div className="flex-1">
            <Label htmlFor="auto-start" className="text-xs">
              Auto-start proxy when app launches
            </Label>
            <p className="text-xs text-muted-foreground mt-0.5">
              Proxy will start automatically on app launch if configured
            </p>
          </div>
          <input
            id="auto-start"
            type="checkbox"
            checked={config.auto_start ?? false}
            onChange={async (e) => {
              const newConfig = { ...config, auto_start: e.target.checked };
              setConfig(newConfig);
              // Save immediately when toggling auto-start
              try {
                await proxyService.saveProxySettings(newConfig);
                setMessage({
                  type: "success",
                  text: e.target.checked ? "Auto-start enabled" : "Auto-start disabled"
                });
              } catch {
                setMessage({ type: "error", text: "Failed to save auto-start setting" });
              }
            }}
            disabled={!config.api_key} // Only enable if we have an API key
            className="h-4 w-4"
          />
        </div>
      </Card>

      {/* Usage Examples */}
      {isRunning && (
        <div className="space-y-3">
          <Card className="p-3">
            <div className="text-xs">
              <strong>Python Example:</strong>
              <pre className="mt-2 text-xs bg-background p-2 rounded border overflow-x-auto">
                <code>{`from openai import OpenAI
client = OpenAI(
  base_url="http://${config.host}:${config.port}/v1",
  api_key="anything"  # API key handled by proxy
)

response = client.chat.completions.create(
  model="llama-3.3-70b",
  messages=[{"role": "user", "content": "Hello!"}],
  stream=True
)

for chunk in response:
    print(chunk.choices[0].delta.content or "", end="")`}</code>
              </pre>
            </div>
          </Card>

          <Card className="p-3">
            <div className="text-xs">
              <strong>cURL Example:</strong>
              <pre className="mt-2 text-xs bg-background p-2 rounded border overflow-x-auto">
                <code>{`curl -N http://${config.host}:${config.port}/v1/chat/completions \\
  -H "Content-Type: application/json" \\
  -d '{
    "model": "llama-3.3-70b",
    "messages": [{"role": "user", "content": "Hello!"}],
    "stream": true
  }'`}</code>
              </pre>
            </div>
          </Card>
        </div>
      )}
    </>
  );
}
