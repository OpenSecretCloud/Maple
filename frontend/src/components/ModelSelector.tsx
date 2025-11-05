import {
  ChevronDown,
  Check,
  Lock,
  Camera,
  ChevronLeft,
  Sparkles,
  Zap,
  Brain,
  Code,
  Image
} from "lucide-react";
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
  "gemma-3-27b": {
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
type ModelCategory = "free" | "quick" | "reasoning" | "math" | "image" | "advanced";

const CATEGORY_MODELS = {
  free: "llama-3.3-70b",
  quick: "gpt-oss-120b",
  reasoning_on: "deepseek-r1-0528", // R1 with thinking
  reasoning_off: "deepseek-v31-terminus", // V3.1 without thinking
  math: "qwen3-coder-480b",
  image: "gemma-3-27b" // Gemma for image analysis
};

const CATEGORY_INFO = {
  free: {
    label: "Free",
    icon: Sparkles,
    description: "Fast & capable"
  },
  quick: {
    label: "Quick",
    icon: Zap,
    description: "Fastest responses"
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
  },
  image: {
    label: "Image Analysis",
    icon: Image,
    description: "Vision & analysis"
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

  // Auto-switch to image analysis when images are uploaded
  useEffect(() => {
    if (chatHasImages && hasAccessToModel(CATEGORY_MODELS.image)) {
      // Only auto-switch if not already on a vision-capable model
      const currentModelConfig = MODEL_CONFIG[model];
      if (!currentModelConfig?.supportsVision) {
        setModel(CATEGORY_MODELS.image);
      }
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [chatHasImages]);

  // Get current category based on selected model
  const getCurrentCategory = (): string => {
    if (model === CATEGORY_MODELS.free) return "Free";
    if (model === CATEGORY_MODELS.quick) return "Quick";
    if (model === CATEGORY_MODELS.reasoning_on || model === CATEGORY_MODELS.reasoning_off) {
      return "Reasoning";
    }
    if (model === CATEGORY_MODELS.math) return "Math/Coding";
    if (model === CATEGORY_MODELS.image) return "Image Analysis";
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
    if (category === "reasoning") {
      // Use thinking state to pick R1 vs V3.1
      targetModel = thinkingEnabled ? CATEGORY_MODELS.reasoning_on : CATEGORY_MODELS.reasoning_off;
    } else if (category === "image") {
      targetModel = CATEGORY_MODELS.image;
    } else {
      targetModel = CATEGORY_MODELS[category as keyof typeof CATEGORY_MODELS];
    }

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
    const planName = billingStatus?.product_name?.toLowerCase() || "";
    const isStarter = planName.includes("starter");

    // Gemma: "starter" for starter users, "pro" for others
    if (modelId === "gemma-3-27b" || modelId === "leon-se/gemma-3-27b-it-fp8-dynamic") {
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

  // Show current category or model name in the collapsed view
  const modelDisplay = (
    <div className="flex items-center gap-1">
      <div className="text-xs font-medium">{getCurrentCategory()}</div>
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
            className="h-8 gap-1 px-2"
            data-testid="model-selector-button"
            aria-label={`Current model: ${MODEL_CONFIG[model]?.displayName || model}. Click to change model.`}
          >
            {modelDisplay}
            <ChevronDown className="h-3 w-3 opacity-50" />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="start" className="w-72 overflow-hidden p-0">
          <div className="relative">
            {/* Main Category Menu */}
            <div
              className={`transition-transform duration-300 ease-in-out ${
                showAdvanced ? "-translate-x-full" : "translate-x-0"
              }`}
            >
              <div className="p-1">
                {/* Category options */}
                {(["free", "quick", "reasoning", "math", "image"] as const).map((category) => {
                  const info = CATEGORY_INFO[category];
                  const Icon = info.icon;
                  const isActive =
                    (category === "free" && model === CATEGORY_MODELS.free) ||
                    (category === "quick" && model === CATEGORY_MODELS.quick) ||
                    (category === "reasoning" &&
                      (model === CATEGORY_MODELS.reasoning_on ||
                        model === CATEGORY_MODELS.reasoning_off)) ||
                    (category === "math" && model === CATEGORY_MODELS.math) ||
                    (category === "image" && model === CATEGORY_MODELS.image);

                  // Check if user has access to this category's model
                  const targetModel =
                    category === "reasoning"
                      ? thinkingEnabled
                        ? CATEGORY_MODELS.reasoning_on
                        : CATEGORY_MODELS.reasoning_off
                      : CATEGORY_MODELS[category];
                  const hasAccess = hasAccessToModel(targetModel);
                  const targetModelConfig = MODEL_CONFIG[targetModel];
                  const requiresUpgrade = !hasAccess;

                  // Disable non-vision categories if chat has images
                  const isDisabledDueToImages = chatHasImages && !targetModelConfig?.supportsVision;
                  const isDisabled = isDisabledDueToImages || targetModelConfig?.disabled;

                  return (
                    <DropdownMenuItem
                      key={category}
                      onClick={() => handleCategorySelect(category)}
                      className={`flex items-center gap-3 px-3 py-2.5 cursor-pointer ${
                        isDisabled ? "opacity-50 cursor-not-allowed" : ""
                      } ${requiresUpgrade ? "hover:bg-purple-50 dark:hover:bg-purple-950/20" : ""}`}
                      disabled={isDisabled}
                    >
                      <Icon className="h-5 w-5 opacity-70" />
                      <div className="flex-1">
                        <div className="flex items-center gap-2">
                          <span className="font-medium">{info.label}</span>
                          {requiresUpgrade && <Lock className="h-3 w-3 opacity-50" />}
                        </div>
                        <div className="text-xs text-muted-foreground">{info.description}</div>
                      </div>
                      {isActive && <Check className="h-4 w-4" />}
                    </DropdownMenuItem>
                  );
                })}

                <DropdownMenuSeparator />

                {/* Advanced option */}
                <DropdownMenuItem
                  onSelect={(e) => {
                    e.preventDefault();
                    setShowAdvanced(true);
                  }}
                  className="flex items-center gap-3 px-3 py-2.5 cursor-pointer"
                >
                  <ChevronLeft className="h-5 w-5 opacity-70 rotate-180" />
                  <div className="flex-1">
                    <span className="font-medium">Advanced</span>
                    <div className="text-xs text-muted-foreground">All models</div>
                  </div>
                </DropdownMenuItem>
              </div>
            </div>

            {/* Advanced Models Panel */}
            <div
              className={`absolute top-0 left-0 w-full transition-transform duration-300 ease-in-out ${
                showAdvanced ? "translate-x-0" : "translate-x-full"
              }`}
            >
              <div className="p-1">
                {/* Back button */}
                <DropdownMenuItem
                  onSelect={(e) => {
                    e.preventDefault();
                    setShowAdvanced(false);
                  }}
                  className="flex items-center gap-2 px-3 py-2 cursor-pointer mb-1"
                >
                  <ChevronLeft className="h-4 w-4" />
                  <span className="font-medium">Back</span>
                </DropdownMenuItem>

                <DropdownMenuSeparator />

                {/* Scrollable model list */}
                <div className="max-h-[400px] overflow-y-auto">
                  {availableModels &&
                    Array.isArray(availableModels) &&
                    [...availableModels]
                      .filter((m) => MODEL_CONFIG[m.id] !== undefined)
                      // Deduplicate: prefer short names over long names
                      .filter((m) => {
                        if (m.id === "leon-se/gemma-3-27b-it-fp8-dynamic") {
                          return !availableModels.some((model) => model.id === "gemma-3-27b");
                        }
                        if (m.id === "ibnzterrell/Meta-Llama-3.3-70B-Instruct-AWQ-INT4") {
                          return !availableModels.some((model) => model.id === "llama-3.3-70b");
                        }
                        return true;
                      })
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
                              isRestricted ? "hover:bg-purple-50 dark:hover:bg-purple-950/20" : ""
                            }`}
                            disabled={effectivelyDisabled}
                          >
                            <div className="flex items-center gap-2 flex-1">
                              <div className="text-sm">
                                {getDisplayName(availableModel.id, true)}
                              </div>
                            </div>
                            {model === availableModel.id && <Check className="h-4 w-4" />}
                          </DropdownMenuItem>
                        );
                      })}
                </div>
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
