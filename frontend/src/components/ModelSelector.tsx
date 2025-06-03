import { ChevronDown, Check } from "lucide-react";
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
import type { Model } from "openai/resources/models.js";

// Model configuration for display names and badges
const MODEL_CONFIG: Record<string, { displayName: string; badge?: string; disabled?: boolean }> = {
  "ibnzterrell/Meta-Llama-3.3-70B-Instruct-AWQ-INT4": {
    displayName: "Llama 3.3 70B"
  },
  "google/gemma-3-27b-it": {
    displayName: "Gemma 3 27B",
    badge: "Coming Soon",
    disabled: true
  },
  "deepseek-r1-70b": {
    displayName: "DeepSeek R1 70B",
    badge: "BETA"
  }
};

export function ModelSelector() {
  const { model, setModel, availableModels, setAvailableModels } = useLocalState();
  const os = useOpenSecret();
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
      console.log("Fetching models from /v1/models endpoint...");
      os.fetchModels()
        .then((models) => {
          console.log("Models endpoint response:", models);
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
          console.log("Filtered models (excluding embed-only):", filteredModels);

          // Get current models for merging from ref
          const currentModels = availableModelsRef.current || [];
          const existingModelIds = new Set(currentModels.map((m) => m.id));
          const newModels = filteredModels.filter((m) => !existingModelIds.has(m.id));
          console.log("New models to add:", newModels);

          // Merge with existing models (keeping the hardcoded one)
          setAvailableModels([...currentModels, ...newModels]);
          console.log("Final available models set:", [...currentModels, ...newModels]);
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

  const getDisplayName = (modelId: string) => {
    const config = MODEL_CONFIG[modelId];

    if (config) {
      if (config.badge) {
        const badgeClass =
          config.badge === "Coming Soon"
            ? "text-[10px] bg-gray-500/10 text-gray-600 px-1.5 py-0.5 rounded-sm font-medium"
            : "text-[10px] bg-purple-500/10 text-purple-600 px-1.5 py-0.5 rounded-sm font-medium";

        return (
          <span className="flex items-center gap-1">
            {config.displayName}
            <span className={badgeClass}>{config.badge}</span>
          </span>
        );
      }
      return config.displayName;
    }

    // Fallback to model ID if not in config
    const model = availableModels.find((m) => m.id === modelId);
    return model?.id || modelId;
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
          // Sort models: enabled first, then disabled
          [...availableModels]
            .sort((a, b) => {
              const aDisabled = MODEL_CONFIG[a.id]?.disabled || false;
              const bDisabled = MODEL_CONFIG[b.id]?.disabled || false;
              if (aDisabled && !bDisabled) return 1;
              if (!aDisabled && bDisabled) return -1;
              return 0;
            })
            .map((availableModel) => {
              const isDisabled = MODEL_CONFIG[availableModel.id]?.disabled || false;
              return (
                <DropdownMenuItem
                  key={availableModel.id}
                  onClick={() => !isDisabled && setModel(availableModel.id)}
                  className={`flex items-center justify-between ${
                    isDisabled ? "opacity-50 cursor-not-allowed" : ""
                  }`}
                  disabled={isDisabled}
                >
                  <div className="text-sm">{getDisplayName(availableModel.id)}</div>
                  {model === availableModel.id && <Check className="h-4 w-4" />}
                </DropdownMenuItem>
              );
            })}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
