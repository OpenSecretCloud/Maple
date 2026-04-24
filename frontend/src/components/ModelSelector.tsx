import { ChevronDown, Check, Lock, Camera, ChevronLeft, Zap, Brain } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
  DropdownMenuSeparator
} from "@/components/ui/dropdown-menu";
import { useLocalState } from "@/state/useLocalState";
import { useOpenSecret } from "@opensecret/react";
import { useEffect, useRef, useState } from "react";
import type { Model } from "openai/resources/models.js";
import { UpgradePromptDialog } from "@/components/UpgradePromptDialog";
import { aliasModelName, LLAMA_MODEL_ID } from "@/utils/utils";

// Model configuration for display names, badges, and token limits
type ModelCfg = {
  displayName: string;
  shortName: string;
  badges?: string[];
  disabled?: boolean;
  requiresPro?: boolean;
  requiresStarter?: boolean;
  supportsVision?: boolean;
  tokenLimit: number;
};

export const MODEL_CONFIG: Record<string, ModelCfg> = {
  [LLAMA_MODEL_ID]: {
    displayName: "Llama 3.3 70B",
    shortName: "Llama 3.3",
    tokenLimit: 70000
  },
  "gemma4-31b": {
    displayName: "Gemma 4 31B",
    shortName: "Gemma 4",
    badges: ["New", "Reasoning"],
    requiresStarter: true,
    supportsVision: true,
    tokenLimit: 256000
  },
  "glm-5-1": {
    displayName: "GLM 5.1",
    shortName: "GLM 5.1",
    badges: ["Pro", "New", "Reasoning"],
    requiresPro: true,
    tokenLimit: 202000
  },
  "deepseek-v4-pro": {
    displayName: "DeepSeek V4 Pro",
    shortName: "DeepSeek V4 Pro",
    badges: ["Pro", "New", "Reasoning"],
    requiresPro: true,
    tokenLimit: 256000
  },
  "kimi-k2-5": {
    displayName: "Kimi K2.5",
    shortName: "Kimi K2.5",
    badges: ["Pro", "Reasoning"],
    requiresPro: true,
    supportsVision: true,
    tokenLimit: 256000
  },
  "kimi-k2-6": {
    displayName: "Kimi K2.6",
    shortName: "Kimi K2.6",
    badges: ["Pro", "New", "Reasoning"],
    requiresPro: true,
    supportsVision: true,
    tokenLimit: 256000
  },
  "gpt-oss-120b": {
    displayName: "OpenAI GPT-OSS 120B",
    shortName: "GPT-OSS",
    badges: ["Reasoning"],
    tokenLimit: 128000
  },
  "qwen3-vl-30b": {
    displayName: "Qwen3-VL 30B",
    shortName: "Qwen3-VL",
    requiresStarter: true,
    supportsVision: true,
    tokenLimit: 256000
  }
};

// Default token limit for unknown models
export const DEFAULT_TOKEN_LIMIT = 64000;

// Get token limit for a specific model
export function getModelTokenLimit(modelId: string): number {
  return MODEL_CONFIG[aliasModelName(modelId)]?.tokenLimit || DEFAULT_TOKEN_LIMIT;
}

// Primary model options
const PRIMARY_MODELS = {
  quick: "gpt-oss-120b",
  powerful: "kimi-k2-6"
};

const PRIMARY_INFO = {
  quick: {
    label: "Quick",
    icon: Zap,
    description: "Fast, everyday responses"
  },
  powerful: {
    label: "Powerful",
    icon: Brain,
    description: "Deeper thinking & analysis"
  }
};

export function ModelSelector({ hasImages = false }: { hasImages?: boolean }) {
  const {
    model,
    setModel,
    availableModels,
    setAvailableModels,
    billingStatus,
    setHasWhisperModel
  } = useLocalState();
  const os = useOpenSecret();
  const isFetching = useRef(false);
  const hasFetched = useRef(false);
  const availableModelsRef = useRef(availableModels);
  const [upgradeDialogOpen, setUpgradeDialogOpen] = useState(false);
  const [selectedModelName, setSelectedModelName] = useState<string>("");
  const [showAdvanced, setShowAdvanced] = useState(false);

  // Use the passed hasImages prop directly
  const chatHasImages = hasImages;

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
          // Check if whisper-large-v3 is available before filtering
          const hasWhisper = models.some((model) => model.id === "whisper-large-v3");
          setHasWhisperModel(hasWhisper);

          // Filter out embedding models and "latest"
          interface ModelWithTasks extends Model {
            tasks?: string[];
          }
          const filteredModels = models.filter((model) => {
            if (model.id === "latest") return false;

            // Filter out whisper models (transcription)
            if (model.id.toLowerCase().includes("whisper")) {
              return false;
            }

            // Filter out qwen3-coder-30b-a3b
            if (model.id === "qwen3-coder-30b-a3b") {
              return false;
            }

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
          const existingModelIds = new Set(currentModels.map((m) => aliasModelName(m.id)));
          const newModels = filteredModels.filter(
            (m) => !existingModelIds.has(aliasModelName(m.id))
          );

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
  }, [os, setAvailableModels, setHasWhisperModel]);

  // Auto-switch to a vision-capable model when images are uploaded
  useEffect(() => {
    if (!chatHasImages) return;
    const currentModelConfig = MODEL_CONFIG[model];
    if (currentModelConfig?.supportsVision) return; // Already on a vision model

    const planName = billingStatus?.product_name?.toLowerCase() || "";
    const isProMaxOrTeam =
      planName.includes("pro") || planName.includes("max") || planName.includes("team");
    const isStarter = planName.includes("starter");

    if (isProMaxOrTeam) {
      // Pro/Max/Team: switch to Powerful (Kimi K2.6 has vision)
      setModel(PRIMARY_MODELS.powerful);
    } else if (isStarter) {
      // Starter: switch to Gemma 4
      setModel("gemma4-31b");
    }
    // Free: no auto-switch (existing upgrade prompt handles it)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [chatHasImages]);

  // Get dropdown button label based on selected model
  const getDropdownLabel = (): string => {
    if (model === PRIMARY_MODELS.quick) return "Quick";
    if (model === PRIMARY_MODELS.powerful) return "Powerful";
    const config = MODEL_CONFIG[model];
    return config?.shortName || config?.displayName || model;
  };

  // Check if user has access to a model based on their plan
  const hasAccessToModel = (modelId: string) => {
    const config = MODEL_CONFIG[modelId];

    // If no restrictions, allow access
    if (!config?.requiresPro && !config?.requiresStarter) return true;

    const planName = billingStatus?.product_name?.toLowerCase() || "";

    // Check if user is on Pro, Max, or Team plan (for requiresPro models)
    if (config?.requiresPro) {
      return planName.includes("pro") || planName.includes("max") || planName.includes("team");
    }

    // Check if user is on Starter, Pro, Max, or Team plan (for requiresStarter models)
    if (config?.requiresStarter) {
      return (
        planName.includes("starter") ||
        planName.includes("pro") ||
        planName.includes("max") ||
        planName.includes("team")
      );
    }

    return true;
  };

  // Handle primary option selection
  const handlePrimarySelect = (key: "quick" | "powerful") => {
    const targetModel = PRIMARY_MODELS[key];

    // Prevent switching to non-vision models if chat has images
    const targetModelConfig = MODEL_CONFIG[targetModel];
    if (chatHasImages && !targetModelConfig?.supportsVision) {
      return;
    }

    // Check access
    if (!hasAccessToModel(targetModel)) {
      const modelConfig = MODEL_CONFIG[targetModel];
      setSelectedModelName(modelConfig?.displayName || targetModel);
      setUpgradeDialogOpen(true);
      return;
    }

    setModel(targetModel);
  };

  // Get dynamic badges for a model based on billing status
  const getModelBadges = (modelId: string): string[] => {
    const config = MODEL_CONFIG[modelId];

    // Filter out Pro and Starter badges
    const badges = config?.badges || [];
    return badges.filter((badge) => badge !== "Pro" && badge !== "Starter");
  };

  const getDisplayName = (modelId: string, showLock = false) => {
    const config = MODEL_CONFIG[modelId];
    const elements: React.ReactNode[] = [];

    if (config) {
      elements.push(config.displayName);

      const badges = getModelBadges(modelId);
      if (badges && badges.length > 0) {
        badges.forEach((badge, index) => {
          let badgeClass = "rounded-md px-1.5 py-0.5 text-[10px] font-medium";

          if (badge === "Coming Soon") {
            badgeClass += " bg-muted text-muted-foreground";
          } else if (badge === "Pro") {
            badgeClass +=
              " bg-gradient-to-r from-[hsl(var(--maple-primary))]/10 to-[hsl(var(--maple-tertiary))]/10 text-[hsl(var(--maple-primary))]";
          } else if (badge === "Starter") {
            badgeClass += " bg-maple-success/10 text-maple-success";
          } else if (badge === "New") {
            badgeClass += " bg-maple-info/10 text-maple-info";
          } else if (badge === "Reasoning") {
            badgeClass += " bg-maple-error/10 text-maple-error";
          } else if (badge === "Beta") {
            badgeClass += " bg-maple-warning/10 text-maple-warning";
          } else {
            badgeClass += " bg-[hsl(var(--maple-primary))]/10 text-[hsl(var(--maple-primary))]";
          }

          elements.push(
            <span key={`badge-${index}`} className={badgeClass}>
              {badge}
            </span>
          );
        });
      }

      if (
        showLock &&
        (config.requiresPro || config.requiresStarter) &&
        !hasAccessToModel(modelId)
      ) {
        elements.push(<Lock key="lock" className="h-3 w-3 opacity-50" />);
      }

      if (config.supportsVision) {
        elements.push(<Camera key="cam" className="h-3 w-3 opacity-50" />);
      }
    } else {
      // Unknown models: show model ID with "Coming Soon" badge
      const model = availableModels.find((m) => m.id === modelId);
      elements.push(model?.id || modelId);
      elements.push(
        <span
          key="badge"
          className="rounded-md bg-muted px-1.5 py-0.5 text-[10px] font-medium text-muted-foreground"
        >
          Coming Soon
        </span>
      );
    }

    return <span className="flex items-center gap-1">{elements}</span>;
  };

  // Show current category or model name in the collapsed view
  const modelDisplay = (
    <div className="flex items-center gap-1">
      <div className="text-xs font-medium">{getDropdownLabel()}</div>
    </div>
  );

  // Always show dropdown even with single model (it may be loading more)

  return (
    <>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button
            variant="ghost"
            size="sm"
            className="h-8 gap-1 px-2 text-[hsl(var(--maple-secondary-700))] hover:bg-[hsl(var(--maple-primary-container))] hover:text-[hsl(var(--maple-secondary-700))]"
            data-testid="model-selector-button"
            aria-label={`Current model: ${MODEL_CONFIG[model]?.displayName || model}. Click to change model.`}
          >
            {modelDisplay}
            <ChevronDown className="h-3 w-3" />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="start" className="w-64 p-0">
          {!showAdvanced ? (
            <div className="p-1 flex flex-col">
              {/* Primary options */}
              {(["quick", "powerful"] as const).map((key) => {
                const info = PRIMARY_INFO[key];
                const Icon = info.icon;
                const targetModel = PRIMARY_MODELS[key];
                const isActive = model === targetModel;
                const hasAccess = hasAccessToModel(targetModel);
                const targetModelConfig = MODEL_CONFIG[targetModel];
                const requiresUpgrade = !hasAccess;

                // Disable non-vision options if chat has images
                const isDisabledDueToImages = chatHasImages && !targetModelConfig?.supportsVision;
                const isDisabled = isDisabledDueToImages || targetModelConfig?.disabled;

                return (
                  <DropdownMenuItem
                    key={key}
                    onClick={() => handlePrimarySelect(key)}
                    className={`flex items-center gap-2 px-3 py-1.5 cursor-pointer ${
                      isDisabled ? "opacity-50 cursor-not-allowed" : ""
                    } ${
                      requiresUpgrade
                        ? "hover:bg-[hsl(var(--maple-primary-container))] dark:hover:bg-[hsl(var(--maple-primary))]/10"
                        : ""
                    }`}
                    disabled={isDisabled}
                  >
                    <Icon className="h-4 w-4 opacity-70" />
                    <div className="flex-1">
                      <div className="flex items-center gap-1.5">
                        <span className="text-sm font-medium">{info.label}</span>
                        {requiresUpgrade && <Lock className="h-3 w-3 opacity-50" />}
                      </div>
                      <div className="text-xs text-muted-foreground">{info.description}</div>
                    </div>
                    {isActive && <Check className="h-4 w-4" />}
                  </DropdownMenuItem>
                );
              })}

              <DropdownMenuSeparator />

              {/* More models option */}
              <DropdownMenuItem
                onSelect={(e) => {
                  e.preventDefault();
                  setShowAdvanced(true);
                }}
                className="flex items-center gap-2 px-3 py-1.5 cursor-pointer"
              >
                <ChevronLeft className="h-4 w-4 opacity-70 rotate-180" />
                <div className="flex-1">
                  <span className="text-sm font-medium">More models</span>
                  <div className="text-xs text-muted-foreground">All models</div>
                </div>
              </DropdownMenuItem>
            </div>
          ) : (
            <div className="p-1 flex flex-col">
              {/* Back button */}
              <DropdownMenuItem
                onSelect={(e) => {
                  e.preventDefault();
                  setShowAdvanced(false);
                }}
                className="flex items-center gap-2 px-3 py-1.5 cursor-pointer mb-1"
              >
                <ChevronLeft className="h-4 w-4" />
                <span className="text-sm font-medium">Back</span>
              </DropdownMenuItem>

              <DropdownMenuSeparator />

              {/* Scrollable model list */}
              <div className="overflow-y-auto flex-1">
                {availableModels &&
                  Array.isArray(availableModels) &&
                  [...availableModels]
                    .filter((m) => MODEL_CONFIG[m.id] !== undefined)
                    // Remove duplicates by id
                    .filter(
                      (m, index, self) =>
                        MODEL_CONFIG[m.id] !== undefined &&
                        self.findIndex((model) => model.id === m.id) === index
                    )
                    .sort((a, b) => {
                      const aConfig = MODEL_CONFIG[a.id];
                      const bConfig = MODEL_CONFIG[b.id];

                      // If chat has images, prioritize vision models
                      if (chatHasImages) {
                        const aHasVision = aConfig?.supportsVision || false;
                        const bHasVision = bConfig?.supportsVision || false;
                        if (aHasVision && !bHasVision) return -1;
                        if (!aHasVision && bHasVision) return 1;
                      }

                      const aDisabled = aConfig?.disabled || false;
                      const bDisabled = bConfig?.disabled || false;
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
                      const isDisabled = config?.disabled || false;
                      const requiresPro = config?.requiresPro || false;
                      const requiresStarter = config?.requiresStarter || false;
                      const hasAccess = hasAccessToModel(availableModel.id);
                      const isRestricted = (requiresPro || requiresStarter) && !hasAccess;

                      // Disable non-vision models if chat has images
                      const isDisabledDueToImages = chatHasImages && !config?.supportsVision;
                      const effectivelyDisabled = isDisabled || isDisabledDueToImages;

                      return (
                        <DropdownMenuItem
                          key={`advanced-${availableModel.id}`}
                          onClick={() => {
                            if (effectivelyDisabled) return;
                            if (isRestricted) {
                              const modelConfig = MODEL_CONFIG[availableModel.id];
                              setSelectedModelName(modelConfig?.displayName || availableModel.id);
                              setUpgradeDialogOpen(true);
                            } else {
                              setModel(availableModel.id);
                              setShowAdvanced(false);
                            }
                          }}
                          className={`flex items-center justify-between group ${
                            effectivelyDisabled ? "opacity-50 cursor-not-allowed" : ""
                          } ${
                            isRestricted
                              ? "hover:bg-[hsl(var(--maple-primary-container))] dark:hover:bg-[hsl(var(--maple-primary))]/10"
                              : ""
                          }`}
                          disabled={effectivelyDisabled}
                        >
                          <div className="flex items-center gap-2 flex-1">
                            <div className="text-sm">{getDisplayName(availableModel.id, true)}</div>
                          </div>
                          {model === availableModel.id && <Check className="h-4 w-4" />}
                        </DropdownMenuItem>
                      );
                    })}
              </div>
            </div>
          )}
        </DropdownMenuContent>
      </DropdownMenu>

      <UpgradePromptDialog
        open={upgradeDialogOpen}
        onOpenChange={setUpgradeDialogOpen}
        feature="model"
        modelName={selectedModelName}
      />
    </>
  );
}
