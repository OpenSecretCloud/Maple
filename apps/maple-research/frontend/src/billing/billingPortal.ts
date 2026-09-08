import { getBillingService } from "@/billing/billingService";
import { openExternalUrl } from "@/utils/openUrl";

export async function openBillingPortal(): Promise<void> {
  const billingService = getBillingService();
  const url = await billingService.getPortalUrl();
  await openExternalUrl(url);
}
