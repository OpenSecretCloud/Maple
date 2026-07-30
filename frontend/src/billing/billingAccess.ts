import type { BillingStatus } from "./billingApi";

export function isKnownFreePlan(billingStatus: BillingStatus | null | undefined): boolean {
  if (!billingStatus) return false;

  const productName = billingStatus.product_name?.trim().toLowerCase() ?? "";
  return productName === "free";
}

export function hasApiAccess(billingStatus: BillingStatus | null | undefined): boolean {
  const productName = billingStatus?.product_name?.toLowerCase() ?? "";

  return productName.includes("pro") || productName.includes("max") || productName.includes("team");
}
