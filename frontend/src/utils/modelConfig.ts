// Model configuration for display names, badges, and token limits
type ModelCfg = {
  displayName: string;
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
    tokenLimit: 70000
  },
  "llama3-3-70b": {
    displayName: "Llama 3.3 70B",
    tokenLimit: 70000
  },
  "google/gemma-3-27b-it": {
    displayName: "Gemma 3 27B",
    badges: ["Starter"],
    requiresStarter: true,
    tokenLimit: 70000
  },
  "leon-se/gemma-3-27b-it-fp8-dynamic": {
    displayName: "Gemma 3 27B",
    badges: ["Starter"],
    requiresStarter: true,
    supportsVision: true,
    tokenLimit: 70000
  },
  "deepseek-r1-70b": {
    displayName: "DeepSeek R1 70B",
    badges: ["Pro"],
    requiresPro: true,
    tokenLimit: 64000
  },
  "deepseek-r1-0528": {
    displayName: "DeepSeek R1 0528 671B",
    badges: ["Pro", "New"],
    requiresPro: true,
    tokenLimit: 130000
  },
  "gpt-oss-120b": {
    displayName: "OpenAI GPT-OSS 120B",
    badges: ["Pro", "New"],
    requiresPro: true,
    tokenLimit: 128000
  },
  "mistral-small-3-1-24b": {
    displayName: "Mistral Small 3.1 24B",
    badges: ["Pro"],
    requiresPro: true,
    supportsVision: true,
    tokenLimit: 128000
  },
  "qwen2-5-72b": {
    displayName: "Qwen 2.5 72B",
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
