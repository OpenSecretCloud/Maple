import {
  ChevronDown,
  Check,
  Lock,
  Camera,
  ChevronLeft,
  Sparkles,
  Zap,
  Brain,
  Code
} from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
  DropdownMenuSeparator,
  DropdownMenuLabel
} from "@/components/ui/dropdown-menu";
import { useLocalState } from "@/state/useLocalState";
import { useOpenSecret } from "@opensecret/react";
import { useEffect, useRef, useState } from "react";
import type { Model } from "openai/resources/models.js";
import { UpgradePromptDialog } from "@/components/UpgradePromptDialog";

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
  "ibnzterrell/Meta-Llama-3.3-70B-Instruct-AWQ-INT4": {
    displayName: "Llama 3.3 70B",
    shortName: "Llama 3.3",
    tokenLimit: 70000
  },
  "llama-3.3-70b": {
    displayName: "Llama 3.3 70B",
    shortName: "Llama 3.3",
    tokenLimit: 70000
  },
  "leon-se/gemma-3-27b-it-fp8-dynamic": {
    displayName: "Gemma 3 27B",
    shortName: "Gemma 3",
    requiresStarter: true,
    supportsVision: true,
    tokenLimit: 20000
  },
  "deepseek-r1-0528": {
    displayName: "DeepSeek R1 671B",
    shortName: "DeepSeek R1",
    badges: ["Pro", "Reasoning"],
    requiresPro: true,
    tokenLimit: 130000
  },
  "deepseek-v31-terminus": {
    displayName: "DeepSeek V3.1 Terminus",
    shortName: "DeepSeek V3.1",
    badges: ["Pro", "New"],
    requiresPro: true,
    tokenLimit: 130000
  },
  "gpt-oss-120b": {
    displayName: "OpenAI GPT-OSS 120B",
    shortName: "GPT-OSS",
    badges: ["Pro"],
    requiresPro: true,
    tokenLimit: 128000
  },
  "mistral-small-3-1-24b": {
    displayName: "Mistral Small 3.1 24B",
    shortName: "Mistral Small",
    badges: ["Pro"],
    requiresPro: true,
    supportsVision: true,
    tokenLimit: 128000
  },
  "qwen2-5-72b": {
    displayName: "Qwen 2.5 72B",
    shortName: "Qwen 2.5",
    badges: ["Pro"],
    requiresPro: true,
    tokenLimit: 128000
  },
  "qwen3-coder-480b": {
    displayName: "Qwen3 Coder 480B",
    shortName: "Qwen3 Coder",
    badges: ["Pro"],
    requiresPro: true,
    tokenLimit: 128000
  }
};

// Default token limit for unknown models
export const DEFAULT_TOKEN_LIMIT = 64000;

// Get token limit for a specific model
export function getModelTokenLimit(modelId: string): number {
  return MODEL_CONFIG[modelId]?.tokenLimit || DEFAULT_TOKEN_LIMIT;
}

// Model categories for simplified UI
type ModelCategory = "free" | "quick" | "reasoning" | "math" | "advanced";

const CATEGORY_MODELS = {
  free: "llama-3.3-70b",
  quick: "gpt-oss-120b",
  reasoning_on: "deepseek-r1-0528", // R1 with thinking
  reasoning_off: "deepseek-v31-terminus", // V3.1 without thinking
  math: "qwen3-coder-480b"
};

const CATEGORY_CONFIG = {
  free: {
    label: "Free",
    icon: Sparkles,
    description: "Fast and capable"
  },
  quick: {
    label: "Quick",
    icon: Zap,
    description: "Balanced performance"
  },
  reasoning: {
    label: "Reasoning",
    icon: Brain,
    description: "Deep analysis"
  },
  math: {
    label: "Math/Coding",
    icon: Code,
    description: "Technical tasks"
  }
};

export function ModelSelector({ hasImages = false }: { hasImages?: boolean }) {
  const {
    model,
    setModel,
    availableModels,
    setAvailableModels,
    billingStatus,
    setHasWhisperModel,
    thinkingEnabled,
    setThinkingEnabled
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
  }, [os, setAvailableModels, setHasWhisperModel]);

  // Sync thinking toggle when model changes externally
  useEffect(() => {
    if (model === CATEGORY_MODELS.reasoning_on) {
      setThinkingEnabled(true);
    } else if (model === CATEGORY_MODELS.reasoning_off) {
      setThinkingEnabled(false);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [model]);

  // Get current category based on selected model
  const getCurrentCategory = (): string => {
    if (model === CATEGORY_MODELS.free) return "Free";
    if (model === CATEGORY_MODELS.quick) return "Quick";
    if (model === CATEGORY_MODELS.reasoning_on || model === CATEGORY_MODELS.reasoning_off) {
      return "Reasoning";
    }
    if (model === CATEGORY_MODELS.math) return "Math/Coding";
    // If in advanced mode, show model name
    const config = MODEL_CONFIG[model];
    return config?.displayName || model;
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

  // Handle category selection
  const handleCategorySelect = (category: ModelCategory) => {
    if (category === "advanced") {
      setShowAdvanced(true);
      return;
    }

    let targetModel: string;
    switch (category) {
      case "free":
        targetModel = CATEGORY_MODELS.free;
        break;
      case "quick":
        targetModel = CATEGORY_MODELS.quick;
        break;
      case "reasoning":
        targetModel = thinkingEnabled
          ? CATEGORY_MODELS.reasoning_on
          : CATEGORY_MODELS.reasoning_off;
        break;
      case "math":
        targetModel = CATEGORY_MODELS.math;
        break;
      default:
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
    const planName = billingStatus?.product_name?.toLowerCase() || "";
    const isStarter = planName.includes("starter");

    // Gemma: "starter" for starter users, "pro" for others
    if (modelId === "leon-se/gemma-3-27b-it-fp8-dynamic") {
      return isStarter ? ["Starter"] : ["Pro"];
    }

    // Llama models: no badges
    if (modelId.includes("llama") || modelId.includes("Llama")) {
      return [];
    }

    // Other models: use their existing badges or default to ["Pro"]
    return config?.badges || ["Pro"];
  };

  const getDisplayName = (modelId: string, showLock = false) => {
    const config = MODEL_CONFIG[modelId];
    const elements: React.ReactNode[] = [];

    if (config) {
      elements.push(config.displayName);

      const badges = getModelBadges(modelId);
      if (badges && badges.length > 0) {
        badges.forEach((badge, index) => {
          let badgeClass = "text-[10px] px-1.5 py-0.5 rounded-sm font-medium";

          if (badge === "Coming Soon") {
            badgeClass += " bg-gray-500/10 text-gray-600";
          } else if (badge === "Pro") {
            badgeClass += " bg-gradient-to-r from-purple-500/10 to-blue-500/10 text-purple-600";
          } else if (badge === "Starter") {
            badgeClass += " bg-gradient-to-r from-green-500/10 to-emerald-500/10 text-green-600";
          } else if (badge === "New") {
            badgeClass += " bg-gradient-to-r from-blue-500/10 to-cyan-500/10 text-blue-600";
          } else if (badge === "Reasoning") {
            badgeClass += " bg-gradient-to-r from-orange-500/10 to-red-500/10 text-orange-600";
          } else if (badge === "Beta") {
            badgeClass += " bg-gradient-to-r from-yellow-500/10 to-orange-500/10 text-yellow-600";
          } else {
            badgeClass += " bg-purple-500/10 text-purple-600";
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
          className="text-[10px] px-1.5 py-0.5 rounded-sm font-medium bg-gray-500/10 text-gray-600"
        >
          Coming Soon
        </span>
      );
    }

    return <span className="flex items-center gap-1">{elements}</span>;
  };

  return (
    <>
      <DropdownMenu onOpenChange={(open) => !open && setShowAdvanced(false)}>
        <DropdownMenuTrigger asChild>
          <Button
            variant="ghost"
            size="sm"
            className="h-8 gap-1.5 px-2"
            data-testid="model-selector-button"
            aria-label={`Current model: ${getCurrentCategory()}. Click to change model.`}
          >
            <div className="text-xs font-medium">{getCurrentCategory()}</div>
            <ChevronDown className="h-3 w-3 opacity-50" />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="start" className="w-72 overflow-hidden">
          <div className="relative">
            {/* Main Category Menu */}
            <div
              className={`transition-transform duration-300 ease-in-out ${
                showAdvanced ? "-translate-x-full" : "translate-x-0"
              }`}
            >
              <DropdownMenuLabel>Select Model</DropdownMenuLabel>
              <DropdownMenuSeparator />

              {/* Free Category */}
              <DropdownMenuItem key="category-free" onClick={() => handleCategorySelect("free")}>
                <div className="flex items-center justify-between w-full">
                  <div className="flex items-center gap-3">
                    <Sparkles className="h-4 w-4 text-primary" />
                    <div className="flex flex-col">
                      <span className="font-medium">{CATEGORY_CONFIG.free.label}</span>
                      <span className="text-xs text-muted-foreground">
                        {CATEGORY_CONFIG.free.description}
                      </span>
                    </div>
                  </div>
                  {model === CATEGORY_MODELS.free && <Check className="h-4 w-4 text-primary" />}
                </div>
              </DropdownMenuItem>

              {/* Quick Category */}
              <DropdownMenuItem
                key="category-quick"
                onClick={() => handleCategorySelect("quick")}
                className={
                  !hasAccessToModel(CATEGORY_MODELS.quick)
                    ? "hover:bg-purple-50 dark:hover:bg-purple-950/20"
                    : ""
                }
              >
                <div className="flex items-center justify-between w-full">
                  <div className="flex items-center gap-3">
                    <Zap className="h-4 w-4 text-primary" />
                    <div className="flex flex-col">
                      <div className="flex items-center gap-2">
                        <span className="font-medium">{CATEGORY_CONFIG.quick.label}</span>
                        {!hasAccessToModel(CATEGORY_MODELS.quick) && (
                          <Lock className="h-3 w-3 opacity-50" />
                        )}
                      </div>
                      <span className="text-xs text-muted-foreground">
                        {CATEGORY_CONFIG.quick.description}
                      </span>
                    </div>
                  </div>
                  {model === CATEGORY_MODELS.quick && <Check className="h-4 w-4 text-primary" />}
                </div>
              </DropdownMenuItem>

              {/* Reasoning Category */}
              <DropdownMenuItem
                key="category-reasoning"
                onClick={() => handleCategorySelect("reasoning")}
                className={
                  !hasAccessToModel(
                    thinkingEnabled ? CATEGORY_MODELS.reasoning_on : CATEGORY_MODELS.reasoning_off
                  )
                    ? "hover:bg-purple-50 dark:hover:bg-purple-950/20"
                    : ""
                }
              >
                <div className="flex items-center justify-between w-full">
                  <div className="flex items-center gap-3">
                    <Brain className="h-4 w-4 text-primary" />
                    <div className="flex flex-col">
                      <div className="flex items-center gap-2">
                        <span className="font-medium">{CATEGORY_CONFIG.reasoning.label}</span>
                        {!hasAccessToModel(
                          thinkingEnabled
                            ? CATEGORY_MODELS.reasoning_on
                            : CATEGORY_MODELS.reasoning_off
                        ) && <Lock className="h-3 w-3 opacity-50" />}
                      </div>
                      <span className="text-xs text-muted-foreground">
                        {CATEGORY_CONFIG.reasoning.description}
                      </span>
                    </div>
                  </div>
                  {(model === CATEGORY_MODELS.reasoning_on ||
                    model === CATEGORY_MODELS.reasoning_off) && (
                    <Check className="h-4 w-4 text-primary" />
                  )}
                </div>
              </DropdownMenuItem>

              {/* Math/Coding Category */}
              <DropdownMenuItem
                key="category-math"
                onClick={() => handleCategorySelect("math")}
                className={
                  !hasAccessToModel(CATEGORY_MODELS.math)
                    ? "hover:bg-purple-50 dark:hover:bg-purple-950/20"
                    : ""
                }
              >
                <div className="flex items-center justify-between w-full">
                  <div className="flex items-center gap-3">
                    <Code className="h-4 w-4 text-primary" />
                    <div className="flex flex-col">
                      <div className="flex items-center gap-2">
                        <span className="font-medium">{CATEGORY_CONFIG.math.label}</span>
                        {!hasAccessToModel(CATEGORY_MODELS.math) && (
                          <Lock className="h-3 w-3 opacity-50" />
                        )}
                      </div>
                      <span className="text-xs text-muted-foreground">
                        {CATEGORY_CONFIG.math.description}
                      </span>
                    </div>
                  </div>
                  {model === CATEGORY_MODELS.math && <Check className="h-4 w-4 text-primary" />}
                </div>
              </DropdownMenuItem>

              <DropdownMenuSeparator />

              {/* Advanced Option */}
              <DropdownMenuItem
                key="category-advanced"
                onClick={() => handleCategorySelect("advanced")}
                onSelect={(e) => {
                  e.preventDefault();
                }}
              >
                <div className="flex items-center gap-3">
                  <ChevronLeft className="h-4 w-4 rotate-180" />
                  <span className="font-medium">Advanced</span>
                </div>
              </DropdownMenuItem>
            </div>

            {/* Advanced Submenu */}
            <div
              className={`absolute top-0 left-0 w-full max-h-[400px] flex flex-col transition-transform duration-300 ease-in-out ${
                showAdvanced ? "translate-x-0" : "translate-x-full"
              }`}
            >
              <DropdownMenuItem
                key="advanced-back"
                onClick={(e) => {
                  e.preventDefault();
                  setShowAdvanced(false);
                }}
                onSelect={(e) => {
                  e.preventDefault();
                }}
              >
                <ChevronLeft className="mr-2 h-4 w-4" />
                <span>Back</span>
              </DropdownMenuItem>
              <DropdownMenuSeparator />
              <DropdownMenuLabel>All Models</DropdownMenuLabel>

              {/* Scrollable container for models */}
              <div className="overflow-y-auto max-h-[300px]">
                {availableModels &&
                  Array.isArray(availableModels) &&
                  [...availableModels]
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
                          } ${isRestricted ? "hover:bg-purple-50 dark:hover:bg-purple-950/20" : ""}`}
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
          </div>
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
