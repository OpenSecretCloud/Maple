import type { BillingStatus } from "@/billing/billingApi";
import {
  BILLING_STATUS_QUERY_KEY,
  NESTED_BILLING_QUERY_MOUNT_POLICY,
  useBillingStatusQuery,
  type BillingStatusQueryDependencies
} from "@/billing/useBillingStatusQuery";

export {
  BILLING_STATUS_QUERY_KEY,
  NESTED_BILLING_QUERY_MOUNT_POLICY as NESTED_SETTINGS_BILLING_QUERY_OPTIONS
};
export type SettingsBillingRefreshDependencies = BillingStatusQueryDependencies;

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
  dependencies?: SettingsBillingRefreshDependencies;
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
