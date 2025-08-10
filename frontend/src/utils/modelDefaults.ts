import { BillingStatus } from "@/billing/billingApi";
import { MODEL_CONFIG } from "@/utils/modelConfig";

type PlanTier = "free" | "starter" | "pro";

/**
 * Extracts the plan tier from billing status
 */
function getPlanTier(billingStatus: BillingStatus | null): PlanTier {
  if (!billingStatus) return "free";

  const planName = billingStatus.product_name?.toLowerCase() || "";

  if (planName.includes("pro") || planName.includes("max") || planName.includes("team")) {
    return "pro";
  }

  if (planName.includes("starter")) {
    return "starter";
  }

  return "free";
}

/**
 * Determines the default model for a user based on their plan type and usage history
 */
export function getDefaultModelForUser(
  billingStatus: BillingStatus | null,
  lastUsedModel: string | null,
  previousPlanTier?: PlanTier
): string {
  const currentPlanTier = getPlanTier(billingStatus);

  // Handle plan upgrade scenario: if user upgraded from Free to Pro/Max/Team,
  // set GPT-OSS as default even if they had Llama before
  if (previousPlanTier === "free" && currentPlanTier === "pro") {
    return "gpt-oss-120b";
  }

  // If user has a last used model and it's still valid, prefer that
  if (
    lastUsedModel &&
    MODEL_CONFIG[lastUsedModel] &&
    hasAccessToModel(lastUsedModel, billingStatus)
  ) {
    return lastUsedModel;
  }

  // Default models based on plan tier
  switch (currentPlanTier) {
    case "pro":
      return "gpt-oss-120b";
    case "starter":
      return "google/gemma-3-27b-it";
    case "free":
    default:
      return "llama3-3-70b";
  }
}

/**
 * Checks if a user has access to a specific model based on their billing status
 */
export function hasAccessToModel(modelId: string, billingStatus: BillingStatus | null): boolean {
  const config = MODEL_CONFIG[modelId];

  // If no restrictions, allow access
  if (!config?.requiresPro && !config?.requiresStarter) return true;

  const planTier = getPlanTier(billingStatus);

  // Check if user has required plan tier
  if (config?.requiresPro) {
    return planTier === "pro";
  }

  if (config?.requiresStarter) {
    return planTier === "starter" || planTier === "pro";
  }

  return true;
}
