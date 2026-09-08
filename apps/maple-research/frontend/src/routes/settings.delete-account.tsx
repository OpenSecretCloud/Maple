import { createFileRoute } from "@tanstack/react-router";
import { DeleteAccountSettings } from "@/components/settings/DeleteAccountSettings";

export const Route = createFileRoute("/settings/delete-account")({
  component: DeleteAccountSettings
});
