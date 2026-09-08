import { createFileRoute, Navigate } from "@tanstack/react-router";
import { AgentConnectionsSettings } from "@/components/settings/AgentConnectionsSettings";
import { useAgentConnectionsAvailability } from "@/components/settings/useAgentConnectionsAvailability";

export const Route = createFileRoute("/settings/agent-connections")({
  component: AgentConnectionsRoute
});

function AgentConnectionsRoute() {
  const availability = useAgentConnectionsAvailability();

  if (availability === "checking") return null;

  if (availability === "unavailable") {
    return <Navigate to="/settings/account" replace />;
  }

  return <AgentConnectionsSettings />;
}
