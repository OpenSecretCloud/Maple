import type { BillingStatus } from "./billingApi";

export function hasApiAccess(billingStatus: BillingStatus | null | undefined): boolean {
  const productName = billingStatus?.product_name?.toLowerCase() ?? "";

  return productName.includes("pro") || productName.includes("max") || productName.includes("team");
}
