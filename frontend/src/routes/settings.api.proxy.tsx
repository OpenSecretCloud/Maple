import { createFileRoute, Navigate } from "@tanstack/react-router";
import { LocalProxySettings } from "@/components/settings/api/LocalProxySettings";
import { isTauriDesktop } from "@/utils/platform";

export const Route = createFileRoute("/settings/api/proxy")({
  component: LocalProxyRoute
});

function LocalProxyRoute() {
  if (!isTauriDesktop()) {
    return <Navigate to="/settings/api" replace />;
  }

  return <LocalProxySettings />;
}
