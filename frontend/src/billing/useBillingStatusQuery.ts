import { useEffect, useLayoutEffect } from "react";
import { useQuery } from "@tanstack/react-query";
import type { BillingStatus } from "@/billing/billingApi";
import { getBillingService } from "@/billing/billingService";

export const BILLING_STATUS_QUERY_KEY = ["billingStatus"] as const;
export const NESTED_BILLING_QUERY_MOUNT_POLICY = {
  refetchOnMount: false
} as const;

export type BillingStatusQueryDependencies = {
  getBillingStatus: () => Promise<BillingStatus>;
};

type BillingStatusSetter = (status: BillingStatus, accountId?: string | null) => void;

const DEFAULT_DEPENDENCIES: BillingStatusQueryDependencies = {
  getBillingStatus: () => getBillingService().getBillingStatus()
};

const lastPublishedBySetter = new WeakMap<
  BillingStatusSetter,
  { accountId: string; status: BillingStatus }
>();

/**
 * Shares billing requests while publishing results only to the account that
 * started them. Query data remains available during a refresh; late results
 * from an account that logged out or changed cannot update LocalState.
 */
export function useBillingStatusQuery({
  accountId,
  billingStatusAccountId,
  clearBillingStatus,
  dependencies = DEFAULT_DEPENDENCIES,
  refetchOnMount,
  setBillingStatus
}: {
  accountId: string | null;
  billingStatusAccountId: string | null;
  clearBillingStatus: () => void;
  dependencies?: BillingStatusQueryDependencies;
  refetchOnMount: boolean | "always";
  setBillingStatus: BillingStatusSetter;
}) {
  const query = useQuery({
    // Account scope keeps a late response in the cache that initiated it. The
    // ["billingStatus"] prefix still lets existing invalidations match every
    // account-scoped entry.
    queryKey: [...BILLING_STATUS_QUERY_KEY, accountId],
    queryFn: dependencies.getBillingStatus,
    enabled: !!accountId,
    refetchOnMount
  });

  // LocalState intentionally survives route and account-surface remounts so a
  // same-account refresh can retain its Usage content. Withhold that snapshot
  // before paint only when it belongs to a different (or logged-out) account.
  useLayoutEffect(() => {
    if (billingStatusAccountId === null || billingStatusAccountId === accountId) return;
    lastPublishedBySetter.delete(setBillingStatus);
    clearBillingStatus();
  }, [accountId, billingStatusAccountId, clearBillingStatus, setBillingStatus]);

  useEffect(() => {
    const status = query.data;
    if (!status || !accountId) return;

    const lastPublished = lastPublishedBySetter.get(setBillingStatus);
    if (lastPublished?.accountId === accountId && lastPublished.status === status) return;
    lastPublishedBySetter.set(setBillingStatus, { accountId, status });
    setBillingStatus(status, accountId);
  }, [accountId, query.data, setBillingStatus]);

  return query;
}
