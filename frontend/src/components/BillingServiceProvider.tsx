import { useOpenSecret } from "@opensecret/react";
import { useEffect } from "react";
import { initBillingService } from "@/billing/billingService";

export function BillingServiceProvider({ children }: { children: React.ReactNode }) {
  const os = useOpenSecret();

  useEffect(() => {
    // Initialize billing service when OpenSecret is available
    initBillingService(os);
  }, [os]);

  return <>{children}</>;
}
