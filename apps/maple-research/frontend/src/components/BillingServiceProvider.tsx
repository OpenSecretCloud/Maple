import { useOpenSecret } from "@opensecret/react";
import { initBillingService } from "@/billing/billingService";

export function BillingServiceProvider({ children }: { children: React.ReactNode }) {
  const os = useOpenSecret();

  // Billing queries can mount during the same render pass as this provider.
  // Initialize synchronously so `getBillingService()` is available before any
  // route or sidebar query function runs.
  initBillingService(os);

  return <>{children}</>;
}
