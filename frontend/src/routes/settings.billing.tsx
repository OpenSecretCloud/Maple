import { createFileRoute } from "@tanstack/react-router";
import { BillingSettings } from "@/components/settings/BillingSettings";

export const Route = createFileRoute("/settings/billing")({
  component: BillingSettings
});
