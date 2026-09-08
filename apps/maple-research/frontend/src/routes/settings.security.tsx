import { createFileRoute } from "@tanstack/react-router";
import { SecuritySettings } from "@/components/settings/SecuritySettings";

export const Route = createFileRoute("/settings/security")({
  component: SecuritySettings
});
