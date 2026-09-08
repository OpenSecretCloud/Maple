import { createFileRoute } from "@tanstack/react-router";
import { CreateApiKeySettings } from "@/components/settings/api/CreateApiKeySettings";

export const Route = createFileRoute("/settings/api/keys/new")({
  component: CreateApiKeySettings
});
