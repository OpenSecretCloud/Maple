import type { BillingStatus } from "@/billing/billingApi";
import {
  useBillingStatusQuery,
  type BillingStatusQueryDependencies
} from "@/billing/useBillingStatusQuery";

/**
 * Makes mounting the Settings shell the explicit billing refresh boundary.
 * React Query keeps prior data available during the request and deduplicates
 * concurrent observers (including a StrictMode-style in-flight remount).
 */
export function useSettingsBillingRefresh({
  accountId,
  billingStatusAccountId,
  clearBillingStatus,
  dependencies,
  setBillingStatus
}: {
  accountId: string | null;
  billingStatusAccountId: string | null;
  clearBillingStatus: () => void;
  dependencies?: BillingStatusQueryDependencies;
  setBillingStatus: (status: BillingStatus, accountId?: string | null) => void;
}) {
  return useBillingStatusQuery({
    accountId,
    billingStatusAccountId,
    clearBillingStatus,
    ...(dependencies ? { dependencies } : {}),
    refetchOnMount: "always",
    setBillingStatus
  });
}
