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
const MODEL_CONFIG: Record<string, { displayName: string; badge?: string }> = {
  "ibnzterrell/Meta-Llama-3.3-70B-Instruct-AWQ-INT4": {
    displayName: "Llama 3.3 70B"
  },
  "google/gemma-3-27b-it": {
    displayName: "Gemma 3 27B",
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
      os.fetchModels()
        .then((models) => {
          // Filter out embedding models and "latest"
          interface ModelWithTasks extends Model {
            tasks?: string[];
          }
          const filteredModels = models.filter(
            (model) =>
              model.id !== "latest" && (model as ModelWithTasks).tasks?.includes("generate")
          );

          // Get current models for merging from ref
          const currentModels = availableModelsRef.current || [];
          const existingModelIds = new Set(currentModels.map((m) => m.id));
          const newModels = filteredModels.filter((m) => !existingModelIds.has(m.id));

          // Merge with existing models (keeping the hardcoded one)
          setAvailableModels([...currentModels, ...newModels]);
        })
        .catch((error) => {
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
        return (
          <span className="flex items-center gap-1">
            {config.displayName}
            <span className="text-[10px] bg-purple-500/10 text-purple-600 px-1.5 py-0.5 rounded-sm font-medium">
              {config.badge}
            </span>
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
          availableModels.map((availableModel) => (
            <DropdownMenuItem
              key={availableModel.id}
              onClick={() => setModel(availableModel.id)}
              className="flex items-center justify-between"
            >
              <div className="text-sm">{getDisplayName(availableModel.id)}</div>
              {model === availableModel.id && <Check className="h-4 w-4" />}
            </DropdownMenuItem>
          ))}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
