import { createFileRoute } from "@tanstack/react-router";
import { ApiKeysSettings } from "@/components/settings/api/ApiKeysSettings";

export const Route = createFileRoute("/settings/api/keys/")({
  component: ApiKeysSettings
});
