import { BillingStatus } from "@/billing/billingApi";
import { MODEL_CONFIG } from "@/components/ModelSelector";

/**
 * Determines the default model for a user based on their plan type and usage history
 */
export function getDefaultModelForUser(
  billingStatus: BillingStatus | null,
  lastUsedModel: string | null
): string {
  // If user has a last used model, prefer that
  if (lastUsedModel && MODEL_CONFIG[lastUsedModel]) {
    return lastUsedModel;
  }

  // If no billing status, assume free plan
  if (!billingStatus) {
    return "llama3-3-70b"; // Llama 3.3 70B for free users
  }

  const planName = billingStatus.product_name?.toLowerCase() || "";

  // Pro, Max, or Team plan users get GPT-OSS 120B
  if (planName.includes("pro") || planName.includes("max") || planName.includes("team")) {
    return "gpt-oss-120b";
  }

  // Starter plan users get Gemma 3
  if (planName.includes("starter")) {
    return "google/gemma-3-27b-it";
  }

  // Free plan users get Llama 3.3 (default fallback)
  return "llama3-3-70b";
}

/**
 * Checks if a user has access to a specific model based on their billing status
 */
export function hasAccessToModel(modelId: string, billingStatus: BillingStatus | null): boolean {
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
}
