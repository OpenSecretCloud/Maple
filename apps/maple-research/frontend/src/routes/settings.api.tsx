import { createFileRoute } from "@tanstack/react-router";
import { ApiSettingsLayout } from "@/components/settings/api/ApiSettingsLayout";

export const Route = createFileRoute("/settings/api")({
  component: ApiSettingsLayout
});
