import { createFileRoute, Navigate } from "@tanstack/react-router";
import { AgentConnectionsSettings } from "@/components/settings/AgentConnectionsSettings";
import { isAgentConnectionsAvailable } from "@/services/agentConnectionsAvailability";

export const Route = createFileRoute("/settings/agent-connections")({
  component: AgentConnectionsRoute
});

function AgentConnectionsRoute() {
  if (!isAgentConnectionsAvailable()) {
    return <Navigate to="/settings/account" replace />;
  }

  return <AgentConnectionsSettings />;
}
