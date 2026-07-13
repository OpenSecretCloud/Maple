import { POWERFUL_MODEL_ALIAS, QUICK_MODEL_ALIAS } from "@/utils/utils";

export const DEFAULT_AGENT_MODEL = "glm-5-2";
export const PRIMARY_AGENT_MODEL_IDS = [DEFAULT_AGENT_MODEL, QUICK_MODEL_ALIAS] as const;

type AgentModelReference = {
  id: string;
  capabilities?: {
    vision?: boolean;
  };
};

type AgentModelAliasReference = {
  id: string;
  target_model?: string;
  capabilities?: {
    vision?: boolean;
  };
};

export function fallbackAgentModel(models: AgentModelReference[]): string {
  return models.some((model) => model.id === DEFAULT_AGENT_MODEL)
    ? DEFAULT_AGENT_MODEL
    : QUICK_MODEL_ALIAS;
}

export function reconcileAgentModel(currentModel: string, models: AgentModelReference[]): string {
  if (!currentModel) return fallbackAgentModel(models);
  if (currentModel === QUICK_MODEL_ALIAS || currentModel === POWERFUL_MODEL_ALIAS) {
    return currentModel;
  }
  if (models.some((model) => model.id === currentModel)) return currentModel;
  return fallbackAgentModel(models);
}

export function resolveAgentModelVisionCapability(
  modelId: string,
  models: AgentModelReference[],
  aliases: AgentModelAliasReference[]
): boolean {
  const alias = aliases.find((candidate) => candidate.id === modelId);
  if (alias) {
    const target = alias.target_model
      ? models.find((candidate) => candidate.id === alias.target_model)
      : undefined;
    return target?.capabilities?.vision ?? alias.capabilities?.vision ?? false;
  }

  return models.find((candidate) => candidate.id === modelId)?.capabilities?.vision ?? false;
}
