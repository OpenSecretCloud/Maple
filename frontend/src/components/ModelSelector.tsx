import { ChevronDown, Check, Loader2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger
} from "@/components/ui/dropdown-menu";
import { useLocalState } from "@/state/useLocalState";
import { useOpenSecret } from "@opensecret/react";
import { useEffect, useRef, useState } from "react";
import type { Model } from "openai/resources/models.js";

export function ModelSelector() {
  const { model, setModel, availableModels, setAvailableModels } = useLocalState();
  const os = useOpenSecret();
  const isFetching = useRef(false);
  const [isLoading, setIsLoading] = useState(false);

  useEffect(() => {
    if (availableModels.length === 0 && os.fetchModels && !isFetching.current) {
      isFetching.current = true;
      setIsLoading(true);
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
          setAvailableModels(filteredModels);

          // If current model is empty or not in the available models, set to first available or fallback
          if (!model || !filteredModels.find((m) => m.id === model)) {
            const fallbackModel =
              filteredModels[0]?.id || "ibnzterrell/Meta-Llama-3.3-70B-Instruct-AWQ-INT4";
            setModel(fallbackModel);
          }
        })
        .catch((error) => {
          // Silently handle error - models will remain empty
          if (import.meta.env.DEV) {
            console.warn("Failed to fetch available models:", error);
          }
        })
        .finally(() => {
          isFetching.current = false;
          setIsLoading(false);
        });
    }
  }, [availableModels.length, os, setAvailableModels, model, setModel]);

  const getDisplayName = (modelId: string) => {
    if (modelId === "ibnzterrell/Meta-Llama-3.3-70B-Instruct-AWQ-INT4") {
      return "Llama 3.3 70B";
    }
    if (modelId === "google/gemma-3-27b-it") {
      return (
        <span className="flex items-center gap-1">
          Gemma 3 27B
          <span className="text-[10px] bg-purple-500/10 text-purple-600 px-1.5 py-0.5 rounded-sm font-medium">
            BETA
          </span>
        </span>
      );
    }
    const model = availableModels.find((m) => m.id === modelId);
    return model?.id || modelId;
  };

  // Always show the same format, whether dropdown or not
  const modelDisplay = (
    <div className="flex items-center gap-1">
      <span className="text-xs text-muted-foreground">Model:</span>
      {isLoading ? (
        <Loader2 className="h-3 w-3 animate-spin" />
      ) : (
        <div className="text-xs font-medium">{getDisplayName(model)}</div>
      )}
    </div>
  );

  // If only one model or no models, show just the model info without dropdown
  if (availableModels.length <= 1) {
    return <div className="h-8 flex items-center gap-1 px-2">{modelDisplay}</div>;
  }

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button variant="ghost" size="sm" className="h-8 gap-1 px-2">
          {modelDisplay}
          <ChevronDown className="h-3 w-3 opacity-50" />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-64">
        {availableModels.map((availableModel) => (
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
