import { createFileRoute, Navigate } from "@tanstack/react-router";
import { AgentConnectionsSettings } from "@/components/settings/AgentConnectionsSettings";
import { isLinux, isMacOS, isTauriDesktop } from "@/utils/platform";

export const Route = createFileRoute("/settings/agent-connections")({
  component: AgentConnectionsRoute
});

function AgentConnectionsRoute() {
  if (!isTauriDesktop() || (!isMacOS() && !isLinux())) {
    return <Navigate to="/settings/account" replace />;
  }

  return <AgentConnectionsSettings />;
}
