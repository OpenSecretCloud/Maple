import { ChevronDown, Check, Lock } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger
} from "@/components/ui/dropdown-menu";
import { useLocalState } from "@/state/useLocalState";
import { useOpenSecret } from "@opensecret/react";
import { useEffect, useRef } from "react";
import { useNavigate } from "@tanstack/react-router";
import type { Model } from "openai/resources/models.js";

// Model configuration for display names and badges
const MODEL_CONFIG: Record<
  string,
  {
    displayName: string;
    badge?: string;
    disabled?: boolean;
    requiresPro?: boolean;
    requiresStarter?: boolean;
  }
> = {
  "ibnzterrell/Meta-Llama-3.3-70B-Instruct-AWQ-INT4": {
    displayName: "Llama 3.3 70B"
  },
  "google/gemma-3-27b-it": {
    displayName: "Gemma 3 27B",
    badge: "Starter",
    requiresStarter: true
  },
  "leon-se/gemma-3-27b-it-fp8-dynamic": {
    displayName: "Gemma 3 27B",
    badge: "Starter",
    requiresStarter: true
  },
  "deepseek-r1-70b": {
    displayName: "DeepSeek R1 70B",
    badge: "Pro",
    requiresPro: true
  }
};

export function ModelSelector() {
  const { model, setModel, availableModels, setAvailableModels, billingStatus } = useLocalState();
  const os = useOpenSecret();
  const navigate = useNavigate();
  const isFetching = useRef(false);
  const hasFetched = useRef(false);
  const availableModelsRef = useRef(availableModels);

  // Keep ref updated
  useEffect(() => {
    availableModelsRef.current = availableModels;
  }, [availableModels]);

  useEffect(() => {
    // Always fetch once at startup
    if (!hasFetched.current && os.fetchModels && !isFetching.current) {
      hasFetched.current = true;
      isFetching.current = true;
      os.fetchModels()
        .then((models) => {
          // Filter out embedding models and "latest"
          interface ModelWithTasks extends Model {
            tasks?: string[];
          }
          const filteredModels = models.filter((model) => {
            if (model.id === "latest") return false;

            // Filter out duplicate llama model
            if (model.id === "llama3-3-70b") return false;

            // Filter out models with lowercase 'instruct' or 'embed' in their ID
            if (model.id.includes("instruct") || model.id.includes("embed")) {
              return false;
            }

            const modelWithTasks = model as ModelWithTasks;
            // If no tasks property, include the model
            if (!modelWithTasks.tasks) return true;

            // If tasks exists, exclude only if it has "embed" but not "generate"
            if (
              modelWithTasks.tasks.includes("embed") &&
              !modelWithTasks.tasks.includes("generate")
            ) {
              return false;
            }

            return true;
          });

          // Get current models for merging from ref
          const currentModels = availableModelsRef.current || [];
          const existingModelIds = new Set(currentModels.map((m) => m.id));
          const newModels = filteredModels.filter((m) => !existingModelIds.has(m.id));

          // Merge with existing models (keeping the hardcoded one)
          setAvailableModels([...currentModels, ...newModels]);
        })
        .catch((error) => {
          console.error("Failed to fetch models from endpoint:", error);
          // Silently handle error - will continue with hardcoded model
          if (import.meta.env.DEV) {
            console.warn("Failed to fetch available models:", error);
          }
        })
        .finally(() => {
          isFetching.current = false;
        });
    }
  }, [os, setAvailableModels]);

  // Check if user has access to a model based on their plan
  const hasAccessToModel = (modelId: string) => {
    const config = MODEL_CONFIG[modelId];

    // If no restrictions, allow access
    if (!config?.requiresPro && !config?.requiresStarter) return true;

    const planName = billingStatus?.product_name?.toLowerCase() || "";

    // Check if user is on Pro or Team plan (for requiresPro models)
    if (config?.requiresPro) {
      return planName.includes("pro") || planName.includes("team");
    }

    // Check if user is on Starter, Pro, or Team plan (for requiresStarter models)
    if (config?.requiresStarter) {
      return planName.includes("starter") || planName.includes("pro") || planName.includes("team");
    }

    return true;
  };

  const getDisplayName = (modelId: string, showLock = false) => {
    const config = MODEL_CONFIG[modelId];
    const elements: React.ReactNode[] = [];

    if (config) {
      elements.push(config.displayName);

      if (config.badge) {
        let badgeClass = "text-[10px] px-1.5 py-0.5 rounded-sm font-medium";

        if (config.badge === "Coming Soon") {
          badgeClass += " bg-gray-500/10 text-gray-600";
        } else if (config.badge === "Pro") {
          badgeClass += " bg-gradient-to-r from-purple-500/10 to-blue-500/10 text-purple-600";
        } else if (config.badge === "Starter") {
          badgeClass += " bg-gradient-to-r from-green-500/10 to-emerald-500/10 text-green-600";
        } else {
          badgeClass += " bg-purple-500/10 text-purple-600";
        }

        elements.push(
          <span key="badge" className={badgeClass}>
            {config.badge}
          </span>
        );
      }

      if (
        showLock &&
        (config.requiresPro || config.requiresStarter) &&
        !hasAccessToModel(modelId)
      ) {
        elements.push(<Lock key="lock" className="h-3 w-3 opacity-50" />);
      }
    } else {
      // Unknown models: show model ID with "Coming Soon" badge
      const model = availableModels.find((m) => m.id === modelId);
      elements.push(model?.id || modelId);
      elements.push(
        <span
          key="badge"
          className="text-[10px] px-1.5 py-0.5 rounded-sm font-medium bg-gray-500/10 text-gray-600"
        >
          Coming Soon
        </span>
      );
    }

    return <span className="flex items-center gap-1">{elements}</span>;
  };

  // Always show the same format, whether dropdown or not
  const modelDisplay = (
    <div className="flex items-center gap-1">
      <span className="text-xs text-muted-foreground">Model:</span>
      <div className="text-xs font-medium">{getDisplayName(model)}</div>
    </div>
  );

  // Always show dropdown even with single model (it may be loading more)

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button variant="ghost" size="sm" className="h-8 gap-1 px-2">
          {modelDisplay}
          <ChevronDown className="h-3 w-3 opacity-50" />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-64">
        {availableModels &&
          Array.isArray(availableModels) &&
          // Sort models: available first, then restricted (pro-only), then disabled
          [...availableModels]
            .sort((a, b) => {
              const aConfig = MODEL_CONFIG[a.id];
              const bConfig = MODEL_CONFIG[b.id];
              // Unknown models are treated as disabled
              const aDisabled = aConfig?.disabled || !aConfig;
              const bDisabled = bConfig?.disabled || !bConfig;
              const aRestricted =
                (aConfig?.requiresPro || aConfig?.requiresStarter || false) &&
                !hasAccessToModel(a.id);
              const bRestricted =
                (bConfig?.requiresPro || bConfig?.requiresStarter || false) &&
                !hasAccessToModel(b.id);

              // Disabled models go last
              if (aDisabled && !bDisabled) return 1;
              if (!aDisabled && bDisabled) return -1;

              // Restricted models go after available but before disabled
              if (aRestricted && !bRestricted) return 1;
              if (!aRestricted && bRestricted) return -1;

              return 0;
            })
            .map((availableModel) => {
              const config = MODEL_CONFIG[availableModel.id];
              // Unknown models are treated as disabled
              const isDisabled = config?.disabled || !config;
              const requiresPro = config?.requiresPro || false;
              const requiresStarter = config?.requiresStarter || false;
              const hasAccess = hasAccessToModel(availableModel.id);
              const isRestricted = (requiresPro || requiresStarter) && !hasAccess;

              return (
                <DropdownMenuItem
                  key={availableModel.id}
                  onClick={() => {
                    if (isDisabled) return;
                    if (isRestricted) {
                      // Navigate to pricing page for upgrade
                      navigate({ to: "/pricing" });
                    } else {
                      setModel(availableModel.id);
                    }
                  }}
                  className={`flex items-center justify-between group ${
                    isDisabled ? "opacity-50 cursor-not-allowed" : ""
                  } ${isRestricted ? "hover:bg-purple-50 dark:hover:bg-purple-950/20" : ""}`}
                  disabled={isDisabled}
                >
                  <div className="flex items-center gap-2 flex-1">
                    <div className="text-sm">{getDisplayName(availableModel.id, true)}</div>
                    {isRestricted && (
                      <span className="text-[10px] text-purple-600 dark:text-purple-400 opacity-0 group-hover:opacity-100 transition-opacity">
                        Upgrade?
                      </span>
                    )}
                  </div>
                  {model === availableModel.id && <Check className="h-4 w-4" />}
                </DropdownMenuItem>
              );
            })}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
