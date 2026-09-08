import { createFileRoute } from "@tanstack/react-router";
import { PreferencesSettings } from "@/components/settings/PreferencesSettings";

export const Route = createFileRoute("/settings/preferences")({
  component: PreferencesSettings
});
